//! W30-CONFLICT-UI: Conflict Mode UI — persistent banner, conflict file list,
//! per-file choose buttons, and Result preview.
//!
//! This module is the **UI half** of the conflict feature.  All git logic lives
//! in the `kagi::git::conflicts` / `resolution` backend (W26): this file only
//! *renders* a [`ConflictMode`] snapshot and wires its buttons to the `KagiApp`
//! handlers (which in turn call the backend `plan_*` / `ResolutionBuffer` API).
//! No `git2` calls happen here.
//!
//! # MVP scope (this lane)
//!
//! - Banner under the header: operation name + `N/M resolved` progress + Continue
//!   (disabled until every file is resolved AND there is no marker residue),
//!   Abort, and Skip (sequencer ops only).
//! - File list with unresolved / resolved / needs-review status, a `ConflictKind`
//!   tag, and prev/next unresolved navigation.
//! - Per-file choose buttons (file granularity): Keep current / Take incoming /
//!   Keep both (current first).  Binary files: choose-only, no preview.
//! - Result preview: the resolved file's text in a simple scroll box.
//!
//! Deferred to v0.2 (NOT here): 3-pane editor, hunk-level choose, manual text
//! editing in-app, blame-of-sides, undo/redo UI, external tool launch.
//!
//! Terminology (ADR-0058): every side label comes from `side_labels` — the words
//! "ours"/"theirs" never appear.

use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};

use kagi::git::conflicts::{ConflictKind, ConflictOp, ConflictStatus, SideLabels};

use super::i18n::Msg;
use super::theme::{self, theme};
use super::KagiApp;

/// In-memory Conflict Mode state held by [`KagiApp`].
///
/// Pure UI-side data: the `ConflictSession` describes the in-progress operation
/// + files; the `ResolutionBuffer` holds the in-memory Result drafts (the
/// repository is untouched until Continue/Abort execute through the plan
/// pipeline).  `current_branch` is captured once at detection time for the
/// `side_labels` left role.
#[derive(Clone)]
pub struct ConflictMode {
    /// The detected conflict session (operation + files), with per-file `status`
    /// recomputed from the buffer at detection time.
    pub session: kagi::git::conflicts::ConflictSession,
    /// The resolution buffer (in-memory Result drafts + materialized sides).
    pub buffer: kagi::git::resolution::ResolutionBuffer,
    /// Current branch short name, for the `side_labels` left role.
    pub current_branch: String,
    /// Index into `session.files` of the file whose detail/preview is open.
    pub selected_file: Option<usize>,
}

impl ConflictMode {
    /// Number of files with a resolution draft in the buffer.
    pub fn resolved_count(&self) -> usize {
        self.session
            .files
            .iter()
            .filter(|f| f.status != ConflictStatus::Unresolved)
            .count()
    }

    /// Whether Continue is allowed: every file resolved AND no marker residue.
    pub fn can_continue(&self) -> bool {
        let all_resolved = self
            .session
            .files
            .iter()
            .all(|f| self.buffer.has_resolution(&f.path));
        all_resolved && self.buffer.files_with_marker_residue().is_empty()
    }

    /// The role labels for the current operation (ADR-0058, never ours/theirs).
    pub fn labels(&self) -> SideLabels {
        kagi::git::conflicts::side_labels(&self.session.op, &self.current_branch)
    }
}

// ────────────────────────────────────────────────────────────
// Unit tests — the pure Conflict Mode gate / status / heading logic.
// (The render functions need a gpui window and are exercised manually.)
// ────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {:?} failed", args);
    }

    /// `git merge` is allowed to exit non-zero (the conflict).
    fn git_allow_fail(dir: &std::path::Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(dir).status();
    }

    /// Build a real merge-conflict repo and return its TempDir.
    fn merge_conflict_repo() -> TempDir {
        let td = TempDir::new().unwrap();
        let p = td.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "t"]);
        std::fs::write(p.join("f.txt"), "line1\nline2\nline3\n").unwrap();
        git(p, &["add", "f.txt"]);
        git(p, &["commit", "-qm", "base"]);
        git(p, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(p.join("f.txt"), "line1\nFEATURE\nline3\n").unwrap();
        git(p, &["commit", "-qam", "feature"]);
        git(p, &["checkout", "-q", "main"]);
        std::fs::write(p.join("f.txt"), "line1\nMAIN\nline3\n").unwrap();
        git(p, &["commit", "-qam", "main"]);
        git_allow_fail(p, &["merge", "feature"]);
        td
    }

    /// Build a ConflictMode from a repo path, mirroring `detect_conflict_mode`.
    fn detect(repo_path: &std::path::Path, branch: &str) -> ConflictMode {
        let repo = git2::Repository::open(repo_path).unwrap();
        let mut session = kagi::git::detect_conflict_session(&repo).expect("conflict session");
        let buffer = kagi::git::ResolutionBuffer::from_repo(&repo).unwrap();
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            f.status = if buffer.has_resolution(&f.path) {
                if residue.contains(&f.path) {
                    ConflictStatus::NeedsReview
                } else {
                    ConflictStatus::Resolved
                }
            } else {
                ConflictStatus::Unresolved
            };
        }
        ConflictMode {
            session,
            buffer,
            current_branch: branch.to_string(),
            selected_file: Some(0),
        }
    }

    #[test]
    fn continue_gate_blocks_until_resolved() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");

        // Detected: one unresolved content conflict → continue is blocked.
        assert_eq!(mode.session.total_count(), 1);
        assert_eq!(mode.resolved_count(), 0);
        assert!(!mode.can_continue(), "gate must block while unresolved");

        // Apply a side choice → resolved, no marker residue → gate opens.
        let path = mode.session.files[0].path.clone();
        mode.buffer
            .apply_choice(&path, kagi::git::ResolutionChoice::Current)
            .unwrap();
        // Recompute status as the UI handler does.
        let residue = mode.buffer.files_with_marker_residue();
        mode.session.files[0].status = if residue.contains(&path) {
            ConflictStatus::NeedsReview
        } else {
            ConflictStatus::Resolved
        };

        assert_eq!(mode.resolved_count(), 1);
        assert!(mode.can_continue(), "gate must open once all files resolved");
    }

    #[test]
    fn marker_residue_keeps_gate_closed() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");
        let path = mode.session.files[0].path.clone();

        // A manual edit that still contains conflict markers is residue.
        mode.buffer
            .set_manual_text(&path, "<<<<<<< HEAD\nMAIN\n=======\nFEATURE\n>>>>>>> feature\n")
            .unwrap();
        assert!(mode.buffer.has_resolution(&path));
        assert!(
            !mode.can_continue(),
            "marker residue must keep the continue gate closed"
        );
    }

    #[test]
    fn heading_uses_roles_not_ours_theirs() {
        let td = merge_conflict_repo();
        let mode = detect(td.path(), "main");
        let heading = op_heading(&mode);
        let lower = heading.to_lowercase();
        assert!(!lower.contains("ours"), "heading leaked 'ours': {}", heading);
        assert!(!lower.contains("theirs"), "heading leaked 'theirs': {}", heading);
        // Merge heading names the current branch verbatim.
        assert!(heading.contains("main"), "heading should name current branch: {}", heading);
    }
}

// ────────────────────────────────────────────────────────────
// Small localized helpers
// ────────────────────────────────────────────────────────────

/// One-line "what is in progress" heading for the banner.
///
/// For rebase this reads "Rebasing <commit> onto <base> — commit step/total"
/// per §2; other ops name the operation + the role's real name.  Branch / commit
/// names are verbatim (not translated).
fn op_heading(mode: &ConflictMode) -> String {
    let labels = mode.labels();
    match &mode.session.op {
        ConflictOp::Rebase { step, total, .. } => format!(
            "{} {} {} {} — {} {}/{}",
            Msg::ConflictRebasing.t(),
            labels.incoming.name,
            Msg::ConflictOnto.t(),
            labels.current.name,
            Msg::ConflictCommit.t(),
            step,
            total
        ),
        ConflictOp::Merge { .. } => format!(
            "{}: {} ← {}",
            Msg::ConflictMerging.t(),
            labels.current.name,
            labels.incoming.name
        ),
        ConflictOp::CherryPick { .. } => {
            format!("{}: {}", Msg::ConflictCherryPicking.t(), labels.incoming.name)
        }
        ConflictOp::Revert { .. } => {
            format!("{}: {}", Msg::ConflictReverting.t(), labels.incoming.name)
        }
    }
}

/// Translated tag text for a conflict kind.
fn kind_tag(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => Msg::ConflictKindContent.t(),
        ConflictKind::RenameDelete => Msg::ConflictKindRenameDelete.t(),
        ConflictKind::ModifyDelete => Msg::ConflictKindModifyDelete.t(),
        ConflictKind::Binary => Msg::ConflictKindBinary.t(),
    }
}

// ────────────────────────────────────────────────────────────
// Banner (persistent, under the header)
// ────────────────────────────────────────────────────────────

/// Render the persistent conflict banner shown directly under the header.
pub fn render_banner(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let total = mode.session.total_count();
    let resolved = mode.resolved_count();
    let can_continue = mode.can_continue();
    let is_sequencer = mode.session.op.is_sequencer();

    let heading = op_heading(mode);
    let progress = format!("{} {}/{}", Msg::ConflictResolved.t(), resolved, total);

    let continue_handler = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_continue(cx);
        cx.notify();
    });
    let abort_handler = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_abort(cx);
        cx.notify();
    });

    let continue_btn = banner_button(
        Msg::ConflictContinue.t(),
        theme().color_success,
        can_continue,
        if can_continue {
            Some(continue_handler)
        } else {
            None
        },
    );
    let abort_btn = banner_button(
        Msg::ConflictAbort.t(),
        theme().color_blocker,
        true,
        Some(abort_handler),
    );

    div()
        .id("conflict-banner")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(6.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().color_warning))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .gap_1()
                .child(
                    div()
                        .text_size(theme::scaled_px(13.))
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(heading)),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(11.))
                        .text_color(if can_continue {
                            rgb(theme().color_success)
                        } else {
                            rgb(theme().text_sub)
                        })
                        .child(SharedString::from(progress)),
                ),
        )
        .child(continue_btn)
        .when(is_sequencer, |el| {
            let skip_handler = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.conflict_skip(cx);
                cx.notify();
            });
            el.child(banner_button(
                Msg::ConflictSkip.t(),
                theme().color_warning,
                true,
                Some(skip_handler),
            ))
        })
        .child(abort_btn)
        .into_any_element()
}

/// A small banner button.  `enabled == false` renders muted and attaches no
/// click handler (the Continue gate).
fn banner_button<H>(
    label: &str,
    accent: u32,
    enabled: bool,
    handler: Option<H>,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let label = label.to_string();
    let mut btn = div()
        .id(SharedString::from(format!("conflict-btn-{}", label)))
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(4.))
        .rounded_md()
        .border_1()
        .text_size(theme::scaled_px(12.))
        .child(SharedString::from(label));

    if enabled {
        btn = btn
            .border_color(rgb(accent))
            .text_color(rgb(accent))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme().selected)));
        if let Some(h) = handler {
            btn = btn.on_click(h);
        }
    } else {
        btn = btn
            .border_color(rgb(theme().text_muted))
            .text_color(rgb(theme().text_muted));
    }
    btn
}

// ────────────────────────────────────────────────────────────
// Full Conflict Mode body (file list + choose + preview)
// ────────────────────────────────────────────────────────────

/// Render the Conflict Mode main pane: a file list on the left, the selected
/// file's choose buttons + Result preview on the right.  Replaces the normal
/// commit-graph body while a conflict session is active.
pub fn render_body(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .size_full()
        .bg(rgb(theme().bg_base))
        .child(render_file_list(mode, cx))
        .child(render_detail(mode, cx))
        .into_any_element()
}

/// The left file list with status + kind tag + prev/next unresolved nav.
fn render_file_list(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let prev_handler = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_nav_unresolved(-1);
        cx.notify();
    });
    let next_handler = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_nav_unresolved(1);
        cx.notify();
    });

    let mut list = div()
        .id("conflict-file-list")
        .flex()
        .flex_col()
        .w(theme::scaled_px(320.))
        .h_full()
        .border_r_1()
        .border_color(rgb(theme().surface))
        .bg(rgb(theme().sidebar))
        .overflow_y_scroll();

    // Prev/next unresolved navigation header (KDiff3 style).
    list = list.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px(theme::scaled_px(10.))
            .py(theme::scaled_px(6.))
            .border_b_1()
            .border_color(rgb(theme().surface))
            .child(
                div()
                    .flex_grow()
                    .text_size(theme::scaled_px(11.))
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(Msg::ConflictFiles.t())),
            )
            .child(nav_button("‹", prev_handler))
            .child(nav_button("›", next_handler)),
    );

    for (idx, file) in mode.session.files.iter().enumerate() {
        let selected = mode.selected_file == Some(idx);
        let (status_color, status_text) = match file.status {
            ConflictStatus::Unresolved => (theme().color_blocker, Msg::ConflictUnresolved.t()),
            ConflictStatus::Resolved => (theme().color_success, Msg::ConflictResolvedShort.t()),
            ConflictStatus::NeedsReview => (theme().color_warning, Msg::ConflictNeedsReview.t()),
        };
        let path_str = file.path.to_string_lossy().into_owned();
        let kind = file.kind;

        let row_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
            this.conflict_select_file(idx);
            cx.notify();
        });

        list = list.child(
            div()
                .id(SharedString::from(format!("conflict-file-{}", idx)))
                .flex()
                .flex_col()
                .gap_1()
                .px(theme::scaled_px(10.))
                .py(theme::scaled_px(6.))
                .cursor_pointer()
                .when(selected, |s| s.bg(rgb(theme().selected)))
                .hover(|s| s.bg(rgb(theme().bg_row_alt)))
                .on_click(row_click)
                .child(
                    div()
                        .text_size(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(path_str)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .text_size(theme::scaled_px(10.))
                                .text_color(rgb(status_color))
                                .child(SharedString::from(status_text)),
                        )
                        .child(
                            div()
                                .text_size(theme::scaled_px(10.))
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(kind_tag(kind))),
                        ),
                ),
        );
    }

    list.into_any_element()
}

fn nav_button<H>(label: &str, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(format!("conflict-nav-{}", label)))
        .px(theme::scaled_px(6.))
        .py(theme::scaled_px(2.))
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().surface))
        .text_size(theme::scaled_px(12.))
        .text_color(rgb(theme().text_sub))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(SharedString::from(label.to_string()))
        .on_click(handler)
}

/// The right detail pane: choose buttons + Result preview for the selected file.
fn render_detail(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let Some(idx) = mode.selected_file else {
        return div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .child(
                div()
                    .text_size(theme::scaled_px(13.))
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::ConflictSelectFile.t())),
            )
            .into_any_element();
    };
    let Some(file) = mode.session.files.get(idx) else {
        return div().size_full().into_any_element();
    };

    let labels = mode.labels();
    let is_binary = file.kind == ConflictKind::Binary;
    let path = file.path.clone();

    // Choose buttons (file granularity).  Labels embed the role's real name.
    let keep_current_label = format!("{} ({})", Msg::ConflictKeepCurrent.t(), labels.current.name);
    let take_incoming_label =
        format!("{} ({})", Msg::ConflictTakeIncoming.t(), labels.incoming.name);
    let keep_both_label = Msg::ConflictKeepBoth.t().to_string();

    let p1 = path.clone();
    let keep_current = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_apply_choice(&p1, kagi::git::resolution::ResolutionChoice::Current);
        cx.notify();
    });
    let p2 = path.clone();
    let take_incoming = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_apply_choice(&p2, kagi::git::resolution::ResolutionChoice::Incoming);
        cx.notify();
    });
    let p3 = path.clone();
    let keep_both = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.conflict_apply_choice(
            &p3,
            kagi::git::resolution::ResolutionChoice::BothCurrentFirst,
        );
        cx.notify();
    });

    let mut choose_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(choose_button(keep_current_label, theme().color_branch, keep_current))
        .child(choose_button(take_incoming_label, theme().color_remote, take_incoming));

    // "Keep both" needs both sides present; only offer it for content conflicts.
    if !is_binary && file.kind == ConflictKind::Content {
        choose_row = choose_row.child(choose_button(keep_both_label, theme().text_sub, keep_both));
    }

    let preview = render_preview(mode, &path, is_binary);

    div()
        .flex()
        .flex_col()
        .size_full()
        .child(choose_row)
        .child(preview)
        .into_any_element()
}

fn choose_button<H>(label: String, accent: u32, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(format!("conflict-choose-{}", label)))
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(5.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(12.))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(SharedString::from(label))
        .on_click(handler)
}

/// Result preview scroll box (MVP: plain text, no syntax / diff coloring).
fn render_preview(
    mode: &ConflictMode,
    path: &std::path::Path,
    is_binary: bool,
) -> gpui::AnyElement {
    let body: gpui::AnyElement = if is_binary {
        div()
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(Msg::ConflictBinaryNoPreview.t()))
            .into_any_element()
    } else if let Some(text) = mode.buffer.resolved_text(path) {
        let mut col = div().flex().flex_col();
        for line in text.split('\n') {
            col = col.child(
                div()
                    .text_size(theme::scaled_px(12.))
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(line.to_string())),
            );
        }
        col.into_any_element()
    } else {
        div()
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(Msg::ConflictPreviewHint.t()))
            .into_any_element()
    };

    div()
        .id("conflict-preview")
        .flex()
        .flex_col()
        .flex_grow()
        .w_full()
        .overflow_y_scroll()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .pb(px(4.))
                .child(SharedString::from(Msg::ConflictResultPreview.t())),
        )
        .child(body)
        .into_any_element()
}
