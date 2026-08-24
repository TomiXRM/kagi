//! Branch Cleanup pane + operations (ADR-0128).
//!
//! A center-takeover table of merged/stale branch candidates. The rows live
//! in `active_view.cleanup_rows` (per-tab) but are **not** snapshot-derived
//! any more (ADR-0128 follow-up, 2026-07-22): classifying every branch walks
//! main's first-parent history plus one `merge_base` per branch, which
//! measured over a second on repos with many long-lived unmerged branches.
//! Running that synchronously inside every `snapshot()` blocked the UI
//! thread after *every* git operation (stash, commit, checkout, ...), not
//! just Branch Cleanup ones. `start_branch_cleanup_scan` now recomputes it on
//! a background thread after each reload (same shape as the Ecosystem mine,
//! `ecosystem.rs`), so the table is a beat behind a reload instead of costing
//! every reload its time. This module owns the pane's open flag, the render,
//! the copy actions, and the plan → confirm → execute pipeline glue (the ops
//! live in `kagi_git::ops::branch_cleanup`).
//!
//! Delete affordances follow the domain classification: `FullyMerged` rows
//! join the bulk action, `SquashMergedLikely` rows are individually deletable,
//! `MergedThenGrown` (WARN) and stale-only rows render with **no** delete
//! button at all — the UI physically can't build a deletion out of them
//! (`BranchCleanupRow::delete_target` returns `None`).

use gpui::prelude::*;
use gpui::ClipboardItem;

use kagi_git::ops::{copy_all_text, BranchCleanupRow, CleanupDeleteTarget, MergedBranchStatus};

use super::modals::BranchCleanupModal;
use super::*;

// ────────────────────────────────────────────────────────────
// KagiApp glue: open/close, copy, plan, execute
// ────────────────────────────────────────────────────────────

impl KagiApp {
    /// Toggle the Branch Cleanup takeover from the sidebar entry.
    pub fn toggle_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        if self.branch_cleanup_open {
            self.close_branch_cleanup_view(cx);
        } else {
            self.open_branch_cleanup_view(cx);
        }
    }

    /// Open the Branch Cleanup table. No-op when no repository is open — the
    /// rows come from the snapshot, so there is nothing to compute here.
    pub fn open_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        if self.repo_path.is_none() {
            return;
        }
        // Same reason as Analyze: File History and Analyze both outrank the
        // cleanup table, so opening it under one of them looked like nothing
        // happened.
        self.close_file_history();
        self.close_ecosystem_view();
        self.pr_mode = None;
        self.branch_cleanup_open = true;
        klog!("branch-cleanup: opened");
        cx.notify();
    }

    /// Tick / untick one row.
    pub fn toggle_cleanup_selection(&mut self, name: String, cx: &mut Context<Self>) {
        if !self.cleanup_selected.remove(&name) {
            self.cleanup_selected.insert(name);
        }
        cx.notify();
    }

    /// Header checkbox: tick every deletable row, or clear the selection when
    /// everything already is.
    pub fn toggle_cleanup_select_all(&mut self, cx: &mut Context<Self>) {
        let all: Vec<String> = self
            .active_view
            .cleanup_rows
            .iter()
            .filter(|r| r.delete_target().is_some())
            .map(|r| r.name.clone())
            .collect();
        if all.iter().all(|n| self.cleanup_selected.contains(n)) {
            self.cleanup_selected.clear();
        } else {
            self.cleanup_selected = all.into_iter().collect();
        }
        cx.notify();
    }

    /// Plan a delete for exactly the ticked rows.
    pub fn delete_selected_cleanup_branches(&mut self, cx: &mut Context<Self>) {
        let targets: Vec<CleanupDeleteTarget> = self
            .active_view
            .cleanup_rows
            .iter()
            .filter(|r| self.cleanup_selected.contains(&r.name))
            .filter_map(|r| r.delete_target())
            .collect();
        self.open_branch_cleanup_plan(targets, cx);
    }

    /// Close the Branch Cleanup table.
    pub fn close_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        self.branch_cleanup_open = false;
        self.cleanup_selected.clear();
        cx.notify();
    }

    /// Recompute the Branch Cleanup table for the current repo on a
    /// background thread, updating `active_view.cleanup_rows` in place when
    /// it finishes (ADR-0128 follow-up). Call after every reload — same
    /// "cheap to call repeatedly" shape as `ensure_startup_repo_io`'s
    /// sub-tasks: no-op with no repo open, and a superseded scan (repo
    /// changed, or a newer scan started) just drops its result instead of
    /// clobbering a fresher one.
    pub fn start_branch_cleanup_scan(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        self.cleanup_gen += 1;
        self.cleanup_scanning = true;
        let my_gen = self.cleanup_gen;
        let now = now_secs();

        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&bg_path).map_err(|e| e.to_string())?;
            let rows = backend
                .collect_branch_cleanup(now)
                .map_err(|e| e.to_string())?;
            // Merged PRs ride along on the same background thread. The rows are
            // branches that are already merged, so their PRs are never in the
            // sidebar's *open* list — the two questions need two calls. Empty
            // when `gh` is unavailable, which just leaves the columns blank.
            let prs = if kagi_git::github::gh_available() {
                backend.list_merged_prs(MERGED_PR_LIMIT).unwrap_or_default()
            } else {
                Vec::new()
            };
            Ok::<_, String>((rows, prs))
        });

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                // Drop the result if superseded: the repo changed under us, or
                // a newer scan (another reload) already started.
                let still_ours = app.cleanup_gen == my_gen
                    && app.repo_path.as_deref() == Some(repo_path.as_path());
                if !still_ours {
                    return;
                }
                app.cleanup_scanning = false;
                match result {
                    Ok((rows, prs)) => {
                        app.cleanup_prs = prs;
                        // Drop ticks for branches that are no longer listed:
                        // a stale name would either do nothing or, worse,
                        // match a re-created branch the user never ticked.
                        let live: std::collections::HashSet<&str> =
                            rows.iter().map(|r| r.name.as_str()).collect();
                        app.cleanup_selected.retain(|n| live.contains(n.as_str()));
                        // Same contract line ADR-0128 originally emitted from
                        // build_tab_view — moved here since this is where the
                        // counts are actually known now.
                        use kagi_git::ops::MergedBranchStatus as S;
                        let full = rows.iter().filter(|r| r.status == S::FullyMerged).count();
                        let squash = rows
                            .iter()
                            .filter(|r| r.status == S::SquashMergedLikely)
                            .count();
                        let warn = rows
                            .iter()
                            .filter(|r| matches!(r.status, S::MergedThenGrown { .. }))
                            .count();
                        let stale = rows.iter().filter(|r| r.stale).count();
                        klog!(
                            "merged-branches: {} full, {} squash?, {} warn, {} stale",
                            full,
                            squash,
                            warn,
                            stale
                        );
                        app.active_view.cleanup_rows = rows;
                        cx.notify();
                    }
                    Err(e) => {
                        klog!("branch-cleanup: scan failed: {}", e);
                    }
                }
            });
        })
        .detach();
    }

    /// Copy every listed branch name (newline-joined) to the clipboard.
    pub fn copy_branch_cleanup_names(&mut self, cx: &mut Context<Self>) {
        let text = copy_all_text(&self.active_view.cleanup_rows);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.push_toast(ToastKind::Info, Msg::CleanupNamesCopied.t(), cx);
    }

    /// Build the delete plan for `targets` and open the confirmation modal.
    /// Used by both the per-row trash button (one target) and the header bulk
    /// button (every `bulk_deletable` row).
    pub fn open_branch_cleanup_plan(
        &mut self,
        targets: Vec<CleanupDeleteTarget>,
        cx: &mut Context<Self>,
    ) {
        if targets.is_empty() {
            return;
        }
        if self.busy_op.is_some() {
            // Was a bare `return`: no modal, no toast, no oplog — a dead
            // button. The toolbar disables its actions with this same reason.
            self.push_toast(ToastKind::Info, Msg::OpInProgress.t(), cx);
            return;
        }
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "branch-cleanup: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_delete_merged_branches(now_secs(), &targets) {
            Ok(plan) => {
                klog!(
                    "plan: branch-cleanup targets={} blockers={}",
                    targets.len(),
                    plan.blockers.len()
                );
                self.set_branch_cleanup_modal(BranchCleanupModal {
                    targets,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
                cx.notify();
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "branch-cleanup plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_branch_cleanup_modal(&mut self) {
        self.clear_branch_cleanup_modal();
    }

    /// Confirm the cleanup: execute in the background (remote deletion is a
    /// network write), then oplog + reload. Per-branch failures come back in
    /// the outcome and are surfaced without discarding the successes.
    pub fn confirm_branch_cleanup(&mut self, cx: &mut Context<Self>) {
        let modal = match self.branch_cleanup_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            self.record_op(
                "branch-cleanup",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        if self.busy_op.is_some() {
            return;
        }
        self.busy_op = Some("branch-cleanup");

        let bg_path = repo_path.clone();
        let plan = modal.plan.clone();
        let targets = modal.targets.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&bg_path)
                .and_then(|b| b.execute_delete_merged_branches(&plan, &targets))
        });

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(outcome) => {
                        klog!(
                            "executed: branch-cleanup deleted={} failed={}",
                            outcome.deleted.len(),
                            outcome.failed.len()
                        );
                        app.clear_branch_cleanup_modal();
                        // The oplog line carries every deleted tip OID — the
                        // recovery contract (ADR-0128): restore with
                        // `git branch <name> <oid>` / `git push origin <oid>:refs/heads/<name>`.
                        let mut parts: Vec<String> = outcome
                            .deleted
                            .iter()
                            .map(|d| {
                                let mut s = d.name.clone();
                                if let Some(l) = &d.local_tip {
                                    s.push_str(&format!(" @{}", l.short()));
                                }
                                if let Some(r) = &d.remote_tip {
                                    s.push_str(&format!(" origin@{}", r.short()));
                                }
                                s
                            })
                            .collect();
                        for (name, reason) in &outcome.failed {
                            parts.push(format!("FAILED {}: {}", name, reason));
                        }
                        let after = kagi_git::ops::StateSummary {
                            head: modal.plan.current.head.clone(),
                            dirty: format!(
                                "deleted {} branch(es): {}",
                                outcome.deleted.len(),
                                parts.join("; ")
                            ),
                        };
                        let outcome_kind = if outcome.failed.is_empty() {
                            kagi_git::oplog::OpOutcome::Success { after }
                        } else if outcome.deleted.is_empty() {
                            kagi_git::oplog::OpOutcome::Failed {
                                error: after.dirty.clone(),
                            }
                        } else {
                            // Partial: record as success (the deletions are
                            // real and recoverable) with the failures in-line.
                            kagi_git::oplog::OpOutcome::Success { after }
                        };
                        app.record_op(
                            "branch-cleanup",
                            modal.plan.current.clone(),
                            outcome_kind,
                            &repo_path,
                            cx,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "branch-cleanup: {} deleted, {} failed",
                            outcome.deleted.len(),
                            outcome.failed.len()
                        )));
                        app.reload(cx);
                    }
                    Err(e) => {
                        // Global refusal (HEAD moved / repo open failure) —
                        // nothing was deleted.
                        let err_msg = i18n::op_failed(i18n::Op::Cleanup, e);
                        app.record_op(
                            "branch-cleanup",
                            modal.plan.current.clone(),
                            kagi_git::oplog::OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        if let Some(m) = self_modal_with_error(&modal, &err_msg) {
                            app.set_branch_cleanup_modal(m);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Rebuild the modal with an error line (keeps plan + targets for a retry).
fn self_modal_with_error(modal: &BranchCleanupModal, err: &str) -> Option<BranchCleanupModal> {
    Some(BranchCleanupModal {
        targets: modal.targets.clone(),
        plan: modal.plan.clone(),
        error: Some(SharedString::from(err.to_string())),
    })
}

/// Wall-clock now in Unix seconds (staleness input for collect/plan).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// Resizable columns (ADR-0128)
// ────────────────────────────────────────────────────────────

/// Left/right table padding (logical px, rendered via `scaled_px`).
pub(super) const CLEANUP_PAD: f32 = 16.0;
/// Width of the inter-column divider strip (doubles as the cell gap).
pub(super) const CLEANUP_GAP: f32 = 4.0;
/// Fixed width of the trailing actions (trash) cell.
const CLEANUP_ACTIONS_W: f32 = 40.0;
/// Locked table row height (uniform_list item height).
const CLEANUP_ROW_H: f32 = 26.0;

/// `(settings key, default, min, max)` per resizable column, indexed by
/// [`DividerKind::CleanupCol`]'s payload (0 = name, 1 = PR, 2 = author,
/// 3 = where, 4 = merged-at, 5 = status).
///
/// PR sits right after the branch name on purpose: a merged branch almost
/// always has a pull request, and `#248 chore: bump to 0.22.0` says what the
/// branch was for in a way `chore/bump-0.22.0` only hints at (user request).
const CLEANUP_COL_SPECS: [(&str, f32, f32, f32); 6] = [
    ("cleanup_name_w", 220.0, 80.0, 600.0),
    ("cleanup_pr_w", 280.0, 80.0, 700.0),
    ("cleanup_author_w", 110.0, 56.0, 260.0),
    ("cleanup_where_w", 100.0, 56.0, 240.0),
    ("cleanup_merged_w", 90.0, 60.0, 240.0),
    ("cleanup_status_w", 150.0, 80.0, 420.0),
];

/// Width of the leading selection-checkbox column.
const CLEANUP_CHECK_W: f32 = 26.0;

/// How many merged PRs to pull for the PR / author columns. Cleanup rows are
/// branches that still exist locally, so they are recent by construction —
/// this is generous, and it is one `gh` call regardless of the number.
const MERGED_PR_LIMIT: usize = 200;

/// Branch Cleanup column widths (logical px), persisted to `settings.json`
/// via `theme::set_col_width` like the commit-list columns (T030).
#[derive(Clone, Copy, Debug)]
pub struct CleanupCols(pub [f32; 6]);

impl Default for CleanupCols {
    fn default() -> Self {
        Self::load()
    }
}

impl CleanupCols {
    /// Read the persisted widths (clamped), falling back to the defaults.
    pub fn load() -> Self {
        let mut w = [0.0f32; 6];
        for (i, (key, default, min, max)) in CLEANUP_COL_SPECS.iter().enumerate() {
            w[i] = theme::read_col_width(key)
                .map(|v| v.clamp(*min, *max))
                .unwrap_or(*default);
        }
        Self(w)
    }

    /// The column's left edge relative to the table's left padding edge.
    fn left_of(&self, idx: usize) -> f32 {
        self.0[..idx].iter().map(|w| w + CLEANUP_GAP).sum()
    }
}

impl KagiApp {
    /// Drag-move handler for a [`DividerKind::CleanupCol`] divider.
    /// `cursor_rel_x` is the cursor in logical px relative to the pane's left
    /// edge (the caller subtracts the sidebar and divides out the zoom).
    pub(super) fn handle_cleanup_col_drag(
        &mut self,
        idx: u8,
        cursor_rel_x: f32,
        cx: &mut Context<Self>,
    ) {
        let idx = (idx as usize).min(CLEANUP_COL_SPECS.len() - 1);
        let (key, _, min, max) = CLEANUP_COL_SPECS[idx];
        let left = CLEANUP_PAD + CLEANUP_CHECK_W + self.cleanup_cols.left_of(idx);
        let new_w = (cursor_rel_x - left - CLEANUP_GAP / 2.0).clamp(min, max);
        if (new_w - self.cleanup_cols.0[idx]).abs() > 0.5 {
            self.cleanup_cols.0[idx] = new_w;
            theme::set_col_width(key, new_w);
            cx.notify();
        }
    }
}

// ────────────────────────────────────────────────────────────
// Render
// ────────────────────────────────────────────────────────────

/// `1_768_003_200 → "2026-01-10"` — UTC civil date without a chrono dep
/// (Howard Hinnant's `civil_from_days`).
fn format_date(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// A tick box. Deliberately a glyph rather than a real control: the table is
/// dense and uniform-height, and the two states have to read at 26px.
fn check_box(
    id: impl Into<gpui::ElementId>,
    checked: bool,
    handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .w(theme::scaled_px(CLEANUP_CHECK_W))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .text_xs()
        .cursor_pointer()
        .text_color(rgb(if checked {
            theme().color_branch
        } else {
            theme().text_muted
        }))
        .hover(|s| s.text_color(rgb(theme().color_branch)))
        .on_click(handler)
        .child(SharedString::from(if checked {
            "\u{2611}"
        } else {
            "\u{2610}"
        }))
        .into_any_element()
}

/// Small clickable header/action button.
fn action_button(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    accent: u32,
    handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(theme::scaled_px(4.))
        .text_xs()
        .text_color(rgb(accent))
        .border_1()
        .border_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(handler)
        .child(label)
        .into_any_element()
}

/// The Branch Cleanup takeover pane (ADR-0128).
pub fn render_branch_cleanup(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let rows = app.active_view.cleanup_rows.clone();
    let cols = app.cleanup_cols;
    let bulk_count = rows.iter().filter(|r| r.bulk_deletable).count();
    let selected_count = app.cleanup_selected.len();
    let selectable: Vec<&str> = rows
        .iter()
        .filter(|r| r.delete_target().is_some())
        .map(|r| r.name.as_str())
        .collect();
    let all_selected =
        !selectable.is_empty() && selectable.iter().all(|n| app.cleanup_selected.contains(*n));

    // ── Header: title + delete + copy-all + close ───────────────
    //
    // One button, two meanings: with rows ticked it deletes exactly those,
    // otherwise it falls back to the bulk "every fully-merged branch". Before
    // the checkboxes the only options were all-or-one-at-a-time, with nothing
    // in between (user request).
    let bulk_button: Option<gpui::AnyElement> = if selected_count > 0 {
        let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.delete_selected_cleanup_branches(cx);
        });
        Some(action_button(
            "cleanup-delete-selected",
            SharedString::from(format!(
                "{} ({})",
                Msg::CleanupDeleteSelected.t(),
                selected_count
            )),
            theme().color_blocker,
            handler,
        ))
    } else {
        (bulk_count > 0).then(|| {
            let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                let targets: Vec<CleanupDeleteTarget> = this
                    .active_view
                    .cleanup_rows
                    .iter()
                    .filter(|r| r.bulk_deletable)
                    .filter_map(|r| r.delete_target())
                    .collect();
                this.open_branch_cleanup_plan(targets, cx);
            });
            action_button(
                "cleanup-bulk-delete",
                SharedString::from(format!("{} ({})", Msg::CleanupDeleteMerged.t(), bulk_count)),
                theme().color_blocker,
                handler,
            )
        })
    };
    let copy_all_button = {
        let handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.copy_branch_cleanup_names(cx);
        });
        action_button(
            "cleanup-copy-all",
            SharedString::from(Msg::CleanupCopyAll.t()),
            theme().color_branch,
            handler,
        )
    };
    let close_button = {
        let handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.close_branch_cleanup_view(cx);
        });
        div()
            .id("cleanup-close")
            .px_2()
            .py_1()
            .rounded(theme::scaled_px(4.))
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .cursor_pointer()
            .hover(|s| {
                s.bg(rgb(theme().surface))
                    .text_color(rgb(theme().text_main))
            })
            .on_click(handler)
            .child(SharedString::from("✕"))
            .into_any_element()
    };

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px(theme::scaled_px(CLEANUP_PAD))
        .py_3()
        .child(
            div()
                .text_xl()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::CleanupTitle.t())),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{}", rows.len()))),
        )
        .child(div().flex_1())
        .children(bulk_button)
        .child(copy_all_button)
        .child(close_button);

    // ── Column header (with drag dividers between the cells) ────
    // Cell widths come from `app.cleanup_cols` (persisted); each divider
    // strip doubles as the cell gap so the header and the rows line up on
    // exactly the same x offsets — which is also what the drag-move math in
    // `handle_cleanup_col_drag` assumes.
    // Same look/feel as the commit list's BRANCH|GRAPH column handles
    // (render_body): panel bg + subtle 1px centre line so the resize boundary
    // is visible without hovering, accent + col-resize cursor on hover.
    let col_divider = |idx: u8| {
        div()
            .id(("cleanup-col-div", idx as usize))
            .w(theme::scaled_px(CLEANUP_GAP))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().panel))
            .flex()
            .justify_center()
            .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
            .hover(|s| s.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::CleanupCol(idx),
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            )
            .into_any_element()
    };
    let col_label = |w: f32, msg: Msg| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .child(SharedString::from(msg.t()))
            .into_any_element()
    };
    let col_header = div()
        .flex()
        .flex_row()
        .items_center()
        .h(theme::scaled_px(24.))
        .px(theme::scaled_px(CLEANUP_PAD))
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(check_box("cleanup-select-all", all_selected, {
            cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                this.toggle_cleanup_select_all(cx);
            })
        }))
        .child(col_label(cols.0[0], Msg::CleanupColBranch))
        .child(col_divider(0))
        .child(col_label(cols.0[1], Msg::CleanupColPr))
        .child(col_divider(1))
        .child(col_label(cols.0[2], Msg::CleanupColAuthor))
        .child(col_divider(2))
        .child(col_label(cols.0[3], Msg::CleanupColWhere))
        .child(col_divider(3))
        .child(col_label(cols.0[4], Msg::CleanupColMergedAt))
        .child(col_divider(4))
        .child(col_label(cols.0[5], Msg::CleanupColStatus))
        .child(col_divider(5))
        .child(div().w(theme::scaled_px(CLEANUP_ACTIONS_W)).flex_shrink_0());

    // ── Rows: uniform_list — fixed row height + real vertical scroll ────
    // (user request: rows were content-sized; lock the height and scroll.)
    let row_count = rows.len();
    let list: gpui::AnyElement = if row_count == 0 {
        div()
            .p_4()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(if app.cleanup_scanning {
                Msg::CleanupScanning.t()
            } else if app.repo_path.is_some() {
                Msg::CleanupEmpty.t()
            } else {
                Msg::CleanupNoRepo.t()
            }))
            .into_any_element()
    } else {
        let scroll_handle = app.cleanup_scroll.clone();
        let scrollbar_handle = scroll_handle.clone();
        super::with_vertical_scrollbar(
            "branch-cleanup-scroll",
            &scrollbar_handle,
            gpui::uniform_list(
                "branch-cleanup-list",
                row_count,
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    let cols = this.cleanup_cols;
                    let prs = this.cleanup_prs.clone();
                    let selected = this.cleanup_selected.clone();
                    range
                        .filter_map(|i| {
                            this.active_view.cleanup_rows.get(i).cloned().map(|row| {
                                let pr = prs.iter().find(|p| p.head == row.name);
                                let ticked = selected.contains(&row.name);
                                build_cleanup_row(&row, i, cols, pr, ticked, cx)
                            })
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&scroll_handle)
            .flex_1()
            .min_h(px(0.)),
            true,
        )
        .into_any_element()
    };

    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        .child(header)
        .child(col_header)
        .child(list)
        .into_any_element()
}

/// One fixed-height table row (uniform_list item).
fn build_cleanup_row(
    row: &BranchCleanupRow,
    i: usize,
    cols: CleanupCols,
    // The merged PR whose head is this branch, when one was found.
    pr: Option<&kagi_domain::github::PullRequest>,
    selected: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Plain (non-draggable) spacer matching the header divider width, so row
    // cells line up with the header cells.
    let gap = || {
        div()
            .w(theme::scaled_px(CLEANUP_GAP))
            .flex_shrink_0()
            .into_any_element()
    };

    // Branch name: click = copy (no separate copy button — user request);
    // truncated with the full name in a tooltip.
    let full_name = SharedString::from(row.name.clone());
    let name_for_copy = row.name.clone();
    let name_cell = div()
        .id(("cleanup-name", i))
        .w(theme::scaled_px(cols.0[0]))
        .flex_shrink_0()
        .min_w(px(0.))
        .overflow_hidden()
        .text_sm()
        .text_color(rgb(theme().text_main))
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(theme().color_branch)))
        .tooltip({
            let full = full_name.clone();
            move |window, cx| Tooltip::new(full.clone()).build(window, cx)
        })
        .on_click(
            cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                super::branch_menu::copy_branch_name(this, name_for_copy.clone(), cx);
            }),
        )
        .child(div().truncate().child(full_name));

    // PR: `#248 chore: bump to 0.22.0`. A merged branch nearly always has one,
    // and its title explains the branch better than the branch name does
    // (user request). Click opens it on GitHub.
    let pr_cell = match pr {
        Some(pr) => {
            let label = SharedString::from(format!("#{} {}", pr.number, pr.title));
            let tip = label.clone();
            let pr_open = pr.clone();
            div()
                .id(("cleanup-pr", i))
                .w(theme::scaled_px(cols.0[1]))
                .flex_shrink_0()
                .min_w(px(0.))
                .overflow_hidden()
                .text_xs()
                .text_color(rgb(theme().text_main))
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme().color_branch)))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(
                    cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                        this.open_pr_in_browser(&pr_open);
                        cx.notify();
                    }),
                )
                .child(div().truncate().child(label))
                .into_any_element()
        }
        None => div()
            .w(theme::scaled_px(cols.0[1]))
            .flex_shrink_0()
            .overflow_hidden()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("\u{2014}"))
            .into_any_element(),
    };

    // Author: the PR's, when known. Blank rather than guessed — the branch tip's
    // committer is often a bot or a rebase artifact, not who wrote the work.
    let author_cell = div()
        .w(theme::scaled_px(cols.0[2]))
        .flex_shrink_0()
        .min_w(px(0.))
        .overflow_hidden()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(div().truncate().child(SharedString::from(match pr {
            Some(pr) if !pr.author.is_empty() => format!("@{}", pr.author),
            _ => "\u{2014}".to_string(),
        })));

    // Where: plain text, no chips (user request: no badge noise).
    let where_text = match (row.local_tip.is_some(), row.remote_tip.is_some()) {
        (true, true) => "local, origin",
        (true, false) => "local",
        (false, true) => "origin",
        (false, false) => "",
    };
    let where_cell = div()
        .w(theme::scaled_px(cols.0[3]))
        .flex_shrink_0()
        .overflow_hidden()
        .text_xs()
        .text_color(rgb(theme().text_main))
        .child(SharedString::from(where_text));

    // Merged-at cell.
    let merged_cell = div()
        .w(theme::scaled_px(cols.0[4]))
        .flex_shrink_0()
        .overflow_hidden()
        .text_xs()
        .text_color(rgb(theme().text_main))
        .child(SharedString::from(
            row.merged_at.map(format_date).unwrap_or_else(|| "—".into()),
        ));

    // Status: one plain colored label, no chips (user request). Stale is
    // appended in the warning color; the grown detail lives in the tooltip.
    let (status_text, status_color) = match &row.status {
        MergedBranchStatus::FullyMerged => (
            Msg::CleanupBadgeMerged.t().to_string(),
            theme().color_success,
        ),
        MergedBranchStatus::SquashMergedLikely => (
            Msg::CleanupBadgeSquash.t().to_string(),
            theme().color_branch,
        ),
        MergedBranchStatus::MergedThenGrown { ahead } => (
            format!("{} +{}", Msg::CleanupBadgeGrown.t(), ahead),
            theme().color_blocker,
        ),
        MergedBranchStatus::NotMerged => (String::new(), theme().text_muted),
    };
    let grown_tooltip = match &row.status {
        MergedBranchStatus::MergedThenGrown { ahead } => Some(SharedString::from(format!(
            "{} +{}",
            Msg::CleanupGrownHint.t(),
            ahead
        ))),
        _ => None,
    };
    let mut status_cell = div()
        .id(("cleanup-status", i))
        .w(theme::scaled_px(cols.0[5]))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .text_xs();
    if !status_text.is_empty() {
        status_cell = status_cell.child(
            div()
                .text_color(rgb(status_color))
                .child(SharedString::from(status_text)),
        );
    }
    if row.stale {
        status_cell = status_cell.child(
            div()
                .text_color(rgb(theme().color_warning))
                .child(SharedString::from(Msg::CleanupBadgeStale.t())),
        );
    }
    if let Some(tip) = grown_tooltip {
        status_cell =
            status_cell.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
    }

    // Actions: trash only, and only when the row can build a target.
    let trash_btn: Option<gpui::AnyElement> = row.delete_target().map(|target| {
        let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.open_branch_cleanup_plan(vec![target.clone()], cx);
        });
        div()
            .id(("cleanup-delete", i))
            .px_1()
            .rounded(theme::scaled_px(4.))
            .text_xs()
            .text_color(rgb(theme().color_blocker))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(handler)
            .child(SharedString::from("🗑"))
            .into_any_element()
    });
    let actions_cell = div()
        .w(theme::scaled_px(CLEANUP_ACTIONS_W))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .children(trash_btn);

    div()
        .h(theme::scaled_px(CLEANUP_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px(theme::scaled_px(CLEANUP_PAD))
        .overflow_hidden()
        .hover(|s| s.bg(rgb(theme().surface)))
        // Only rows that can actually build a delete target get a tick box —
        // ticking one that cannot be deleted would promise something the
        // delete plan then quietly drops.
        .children(row.delete_target().is_some().then(|| {
            let name = row.name.clone();
            check_box(("cleanup-check", i), selected, {
                cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                    this.toggle_cleanup_selection(name.clone(), cx);
                })
            })
        }))
        .children((row.delete_target().is_none()).then(|| {
            div()
                .w(theme::scaled_px(CLEANUP_CHECK_W))
                .flex_shrink_0()
                .into_any_element()
        }))
        .child(name_cell)
        .child(gap())
        .child(pr_cell)
        .child(gap())
        .child(author_cell)
        .child(gap())
        .child(where_cell)
        .child(gap())
        .child(merged_cell)
        .child(gap())
        .child(status_cell)
        .child(gap())
        .child(actions_cell)
        .into_any_element()
}
