//! Commit Inspector panel — W2-INSPECTOR / ADR-0015 / W7-INSPECTOR2
//!
//! W7-INSPECTOR2 layout (top → bottom):
//!   1. Title          (commit summary, wraps to 2 lines max + truncate)
//!   2. Meta row       (avatar · author name · committed date · short-hash chip)
//!   3. Actions row    (Create branch · Cherry-pick · Copy SHA — compact)
//!   4. Message box    (independent vertical scroll, resizable)
//!   ── InspectorSplit divider (drag to change message:files ratio) ──
//!   5. Counts row     (N modified · N added · N deleted · N renamed)
//!   6. Changed files  (tree or flat path list, Path⇄Tree toggle, scroll)
//!
//! The message box and files box default to a 1:1 height ratio
//! (`KagiApp.inspector_split = 0.5`); the divider clamps it to 0.2..=0.8.
//! The hash chip shows the full SHA in a tooltip and copies it on click
//! (replacing the old always-on Metadata column of Parents / full SHA).

use gpui::{App, Bounds, Context, IntoElement, Pixels, SharedString, Window, canvas, div, prelude::*, px, relative, rgb};

use kagi::git::{ChangeKind, CommitId, FileStatus};

use super::{
    CompareView, DividerDrag, DividerGhost, DividerKind, KagiApp,
    avatar::{avatar_color, avatar_initial},
    commit_list::{BadgeKind, RefBadge},
    context_menu::CommitAction,
    detail_panel::CommitDetail,
    file_tree,
};

// W9-THEME: all colours come from `theme()` (see theme.rs). No local palette.
use super::theme::{self, theme};

const MAX_FILES: usize = 100;
const MAX_BADGE_CHARS: usize = 20;

// W7-INSPECTOR2: message/files split clamp bounds (mirrors mod.rs; the drag
// handler in mod.rs is the source of truth — these guard the flex_basis only).
const INSPECTOR_SPLIT_MIN: f32 = 0.2;
const INSPECTOR_SPLIT_MAX: f32 = 0.8;
pub(super) const INSPECTOR_SPLIT_DIVIDER_H: f32 = 4.0;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ─────────────────────────────────────────────────────────────────────────────

/// Render the Commit Inspector right panel.
///
/// Section order (W7-INSPECTOR2):
///   Title → Meta row → Actions → Message box │ divider │ Counts → Changed Files
///
/// `tree_view` — when `true` render the tree; when `false` render flat paths.
/// `inspector_split` — message:files height ratio (0.5 = 1:1).
#[allow(clippy::too_many_arguments)]
pub fn render_inspector(
    d: CommitDetail,
    at: CommitId,
    badges: Vec<RefBadge>,
    changed_files: Option<Vec<FileStatus>>,
    compare_view: Option<CompareView>,
    active_file: Option<usize>,
    tree_view: bool,
    inspector_split: f32,
    // Measured (top, bottom) of the message+files region, written at paint
    // time and read by the InspectorSplit drag handler in mod.rs.
    split_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    panel_width: f32,
    // W11-AVATAR: resolved GitHub avatar images keyed by author email.
    avatar_images: &std::collections::HashMap<String, std::sync::Arc<gpui::Image>>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // ── Truncate input files before building the tree (T018 policy) ──────
    let truncated_files: Option<Vec<FileStatus>> = changed_files.as_ref().map(|files| {
        files.iter().take(MAX_FILES).cloned().collect()
    });
    let total_files = changed_files.as_ref().map(|f| f.len()).unwrap_or(0);
    let truncated_count = if total_files > MAX_FILES {
        Some(total_files - MAX_FILES)
    } else {
        None
    };

    // ── Short SHA (first 8 hex chars) ────────────────────────────────────
    let short_sha: SharedString = SharedString::from(
        d.full_sha.chars().take(8).collect::<String>()
    );

    // ── Copy SHA handler (full raw SHA — no ZWSP) ─────────────────────────
    let copy_target1 = at.clone();
    let copy_sha_click1 = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CopySha, copy_target1.clone(), _window, cx);
    });

    // ── Copy SHA handler for Actions section ─────────────────────────────
    let copy_target2 = at.clone();
    let copy_sha_click2 = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CopySha, copy_target2.clone(), _window, cx);
    });

    // ── Author name + email (parsed from `author_line`) ───────────────────
    // `author_line` format: "name  <email>  YYYY-MM-DD HH:MM" (detail_panel).
    // We only need the display name (meta row) and email (avatar colour).
    let (author_name, author_email) = parse_author(d.author_line.as_ref());

    // ── Message lines (split on '\n') ─────────────────────────────────────
    let message_lines: Vec<_> = d.full_message
        .as_ref()
        .split('\n')
        .map(|line| {
            let text = if line.is_empty() {
                SharedString::from("\u{00A0}") // NBSP spacer
            } else {
                SharedString::from(line.to_string())
            };
            div()
                .flex().flex_row().w_full()
                .flex_shrink_0()
                .text_color(rgb(theme().text_main))
                .text_sm()
                .truncate()
                .child(text)
                .into_any()
        })
        .collect();

    // ── Tree rows ─────────────────────────────────────────────────────────
    let tree_rows = truncated_files.as_ref().map(|files| {
        file_tree::build_file_tree(files)
    });

    let tree_element_rows: Vec<_> = if tree_view {
        match &tree_rows {
            None => vec![],
            Some(rows) => rows.iter().map(|row| {
                match row {
                    file_tree::TreeRow::Dir { depth, name } => {
                        let indent = (*depth as f32) * 12.0;
                        div()
                            .id(SharedString::from(format!("tree-dir-{}", name.as_ref())))
                            .flex().flex_row().items_center()
                            .pl(px(indent)).mb_px()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm().text_color(rgb(theme().change_dir))
                                    .truncate()
                                    .child(name.clone()),
                            )
                            .into_any()
                    }
                    file_tree::TreeRow::File { depth, name, file_index, change } => {
                        let indent = (*depth as f32) * 12.0;
                        let (badge_char, badge_color) = change_badge(change);
                        let fi = *file_index;
                        let click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                            this.open_main_diff_inspector_file(fi);
                            cx.notify();
                        });
                        div()
                            .id(("file-row", fi))
                            .flex().flex_row().items_center().gap_1()
                            .pl(px(indent)).mb_px()
                            .flex_shrink_0()
                            .when(active_file == Some(fi), |el| el.bg(rgb(theme().selected)).rounded_sm())
                            .on_click(click)
                            .child(
                                div().w(px(14.)).flex_shrink_0()
                                    .text_sm().text_color(rgb(badge_color))
                                    .child(SharedString::from(badge_char)),
                            )
                            .child(
                                div().flex_1()
                                    .text_sm().text_color(rgb(theme().text_main))
                                    .truncate()
                                    .child(name.clone()),
                            )
                            .into_any()
                    }
                }
            }).collect(),
        }
    } else {
        // ── Flat path list ─────────────────────────────────────────────────
        match truncated_files.as_ref() {
            None => vec![],
            Some(files) => files.iter().enumerate().map(|(fi, fs)| {
                let (badge_char, badge_color) = change_badge(&fs.change);
                let path_text = SharedString::from(
                    fs.path.to_string_lossy().into_owned()
                );
                let click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                    this.open_main_diff_inspector_file(fi);
                    cx.notify();
                });
                div()
                    .id(("file-flat", fi))
                    .flex().flex_row().items_center().gap_1()
                    .mb_px()
                    .flex_shrink_0()
                    .when(active_file == Some(fi), |el| el.bg(rgb(theme().selected)).rounded_sm())
                    .on_click(click)
                    .child(
                        div().w(px(14.)).flex_shrink_0()
                            .text_sm().text_color(rgb(badge_color))
                            .child(SharedString::from(badge_char)),
                    )
                    .child(
                        div().flex_1()
                            .text_sm().text_color(rgb(theme().text_main))
                            .truncate()
                            .child(path_text),
                    )
                    .into_any()
            }).collect(),
        }
    };

    // ── "Create branch here" button ──────────────────────────────────────
    let at_for_create = at.clone();
    let at_for_cherry = at.clone();
    let create_branch_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CreateBranchHere, at_for_create.clone(), _window, cx);
        cx.notify();
    });
    let create_branch_button = action_button(
        "create-branch-btn",
        "+ Branch here",
        theme().color_branch,
        create_branch_click,
    );

    // ── "Cherry-pick onto HEAD" button (T016) ────────────────────────────
    let cherry_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.dispatch_commit_action(CommitAction::CherryPick, at_for_cherry.clone(), _window, cx);
        cx.notify();
    });
    let cherry_pick_button = action_button(
        "cherry-pick-btn",
        "\u{1f352} Cherry-pick",
        theme::theme().accent, // accent (cherry-pick)
        cherry_click,
    );

    // ── "Copy SHA" button in Actions section ─────────────────────────────
    let copy_sha_button = action_button(
        "copy-sha-actions-btn",
        "Copy SHA",
        theme::theme().accent_alt, // accent (copy-sha)
        copy_sha_click2,
    );

    // ── Path⇄Tree toggle ─────────────────────────────────────────────────
    // Each button sets its mode explicitly (a shared toggle would make the
    // active button flip the view to the other mode on click).
    let toggle_click_a = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.inspector_tree_view = false;
        cx.notify();
    });
    let toggle_click_b = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.inspector_tree_view = true;
        cx.notify();
    });
    let path_active = !tree_view;
    let tree_active =  tree_view;
    let path_bg = if path_active { theme().selected } else { theme().surface };
    let tree_bg = if tree_active { theme().selected } else { theme().surface };
    let path_col = if path_active { theme().text_main } else { theme().text_sub };
    let tree_col = if tree_active { theme().text_main } else { theme().text_sub };

    let toggle_row = div()
        .id("files-toggle-row")
        .flex().flex_row().items_center().gap_1()
        .child(
            div()
                .id("toggle-path")
                .px_2().py_px()
                .rounded_sm()
                .bg(rgb(path_bg))
                .text_xs().text_color(rgb(path_col))
                .on_click(toggle_click_a)
                .child(SharedString::from("Path")),
        )
        .child(
            div()
                .id("toggle-tree")
                .px_2().py_px()
                .rounded_sm()
                .bg(rgb(tree_bg))
                .text_xs().text_color(rgb(tree_col))
                .on_click(toggle_click_b)
                .child(SharedString::from("Tree")),
        );

    // ── Counts row (ChangeKind tally; 0-count kinds omitted) ──────────────
    // Renamed is matched with `{ .. }` because it carries a `from` path.
    let counts_row = changed_files.as_ref().map(|files| {
        let mut modified = 0usize;
        let mut added = 0usize;
        let mut deleted = 0usize;
        let mut renamed = 0usize;
        let mut typechange = 0usize;
        for fs in files {
            match fs.change {
                ChangeKind::Modified       => modified += 1,
                ChangeKind::Added          => added += 1,
                ChangeKind::Deleted        => deleted += 1,
                ChangeKind::Renamed { .. } => renamed += 1,
                ChangeKind::TypeChange     => typechange += 1,
            }
        }
        let mut parts: Vec<String> = Vec::new();
        if modified > 0   { parts.push(format!("{} modified", modified)); }
        if added > 0      { parts.push(format!("{} added", added)); }
        if deleted > 0    { parts.push(format!("{} deleted", deleted)); }
        if renamed > 0    { parts.push(format!("{} renamed", renamed)); }
        if typechange > 0 { parts.push(format!("{} type-change", typechange)); }
        let text = if parts.is_empty() {
            SharedString::from("No file changes")
        } else {
            SharedString::from(parts.join("  \u{00B7}  "))
        };
        div()
            .flex().flex_row().items_center()
            .mb_1()
            .text_xs().text_color(rgb(theme().text_sub))
            .truncate()
            .child(text)
    });

    let compare_banner = compare_view.as_ref().map(|view| {
        let close_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.close_compare_view();
            cx.notify();
        });
        div()
            .id("compare-banner")
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .mb_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(format!("Comparing: {}", view.title.as_ref()))),
            )
            .child(
                div()
                    .id("compare-close")
                    .px_1()
                    .rounded_sm()
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(close_click)
                    .child(SharedString::from("×")),
            )
    });

    // ── Files header: Path⇄Tree toggle (compare banner above when comparing) ─
    let files_header = div()
        .flex().flex_row().items_center().justify_end().mb_1()
        .child(toggle_row);

    // ── Scrolling file list (own scroll, independent of message box) ──────
    let mut files_list = div()
        .id("inspector-files-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex().flex_col();

    if changed_files.is_none() {
        files_list = files_list.child(
            div().text_sm().text_color(rgb(theme().text_muted))
                .child(SharedString::from("(diff unavailable)")),
        );
    } else {
        for row in tree_element_rows {
            files_list = files_list.child(row);
        }
        if let Some(remaining) = truncated_count {
            files_list = files_list.child(
                div().text_sm().text_color(rgb(theme().text_muted))
                    .child(SharedString::from(format!("\u{2026} and {} more", remaining))),
            );
        }
    }

    // ── Bottom box: compare banner / counts / toggle / files list ─────────
    let files_box = div()
        .flex().flex_col().min_h(px(0.))
        .flex_basis(relative((1.0 - inspector_split).clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX)))
        .flex_shrink()
        .px_3()
        .children(compare_banner)
        .children(counts_row)
        .child(files_header)
        .child(files_list);

    // ── Title (commit summary, up to 2 wrapped lines + truncate) ──────────
    let title_text: SharedString = SharedString::from(
        d.full_message.as_ref().lines().next().unwrap_or("").to_string()
    );
    let title_el = div()
        .text_color(rgb(theme().text_main))
        .font_weight(gpui::FontWeight::MEDIUM)
        .mb_1()
        .line_clamp(2)
        .child(title_text);

    // ── Meta row: avatar · author name · committed date · short-hash chip ──
    // W11-AVATAR: show the resolved GitHub avatar image when available; else
    // the initial-on-colour circle (T020 fallback).
    let avatar_hsla = avatar_color(&author_email);
    let initial = SharedString::from(avatar_initial(&author_name));
    let avatar_el = {
        let circle = div()
            .w(px(18.)).h(px(18.)).flex_shrink_0()
            .rounded_full()
            .overflow_hidden();
        match avatar_images.get(&author_email).cloned() {
            Some(image) => circle.child(
                gpui::img(gpui::ImageSource::Image(image))
                    .size_full()
                    .rounded_full(),
            ),
            None => circle
                .flex().items_center().justify_center()
                .bg(avatar_hsla)
                .text_xs().text_color(rgb(theme().bg_base))
                .child(initial),
        }
    };

    let author_name_short: SharedString = if author_name.chars().count() > 24 {
        let s: String = author_name.chars().take(23).collect();
        SharedString::from(format!("{}\u{2026}", s))
    } else {
        SharedString::from(author_name.clone())
    };

    // Short-hash chip: tooltip = full SHA (+ committer when it differs from the
    // author), click = Copy SHA (dispatch).  This replaces the old always-on
    // Parents / full-SHA / Committer metadata column.
    let tooltip_text: SharedString = match &d.committer_line {
        Some(c) => SharedString::from(format!("{}\nCommitter: {}", d.full_sha.as_ref(), c.as_ref())),
        None => d.full_sha.clone(),
    };
    let hash_chip = div()
        .id("inspector-hash-chip")
        .flex_shrink_0()
        .px_1().rounded_sm()
        .bg(rgb(theme().surface))
        .text_xs().text_color(rgb(theme().text_sub))
        .hover(|s| s.bg(rgb(theme().selected)).text_color(rgb(theme().text_main)).cursor_pointer())
        .on_click(copy_sha_click1)
        .tooltip(move |_window, cx| {
            cx.new(|_| HashTooltip { sha: tooltip_text.clone() }).into()
        })
        .child(short_sha);

    let meta_row = div()
        .flex().flex_row().items_center().gap_2().mb_2()
        .child(avatar_el)
        .child(
            div().flex_1().min_w(px(0.))
                .flex().flex_row().items_center().gap_2()
                .child(
                    div().flex_shrink().min_w(px(0.))
                        .text_sm().text_color(rgb(theme().text_main))
                        .truncate()
                        .child(author_name_short),
                )
                .child(
                    div().flex_shrink_0()
                        .text_xs().text_color(rgb(theme().text_muted))
                        .child(d.committed_date),
                ),
        )
        .child(hash_chip);

    // ── Ref badges row ────────────────────────────────────────────────────
    let badges_row = {
        let mut row = div().flex().flex_row().items_center().flex_wrap().gap_1().mb_1();
        let mut by_prio = badges;
        by_prio.sort_by_key(|b| badge_priority(&b.kind));
        for badge in &by_prio {
            let color = match badge.kind {
                BadgeKind::HeadBranch => theme().color_head,
                BadgeKind::Branch     => theme().color_branch,
                BadgeKind::Remote     => theme().color_remote,
                BadgeKind::Tag        => theme().color_tag,
            };
            let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
                let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
                SharedString::from(format!("{}\u{2026}", s))
            } else {
                badge.label.clone()
            };
            row = row.child(
                div()
                    .px_1().rounded_sm()
                    .bg(rgb(color))
                    .text_color(rgb(theme().bg_base))
                    .text_xs().flex_shrink_0()
                    .child(label),
            );
        }
        row
    };

    // ── Compact Actions row (single row) ──────────────────────────────────
    let actions_row = div()
        .flex().flex_row().items_center().gap_1().flex_wrap().mb_2()
        .child(create_branch_button)
        .child(cherry_pick_button)
        .child(copy_sha_button);

    // ── Message box (independent scroll, top of the split) ────────────────
    let mut message_inner = div().flex().flex_col();
    for line_el in message_lines {
        message_inner = message_inner.child(line_el);
    }
    let message_box = div()
        .id("inspector-message-scroll")
        .flex().flex_col().min_h(px(0.))
        .flex_basis(relative(inspector_split.clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX)))
        .flex_shrink()
        .overflow_y_scroll()
        .px_3()
        .child(message_inner);

    // ── InspectorSplit divider (absolute-coordinate ratio; see mod.rs) ────
    let split_divider = div()
        .id("inspector-split-divider")
        .h(px(INSPECTOR_SPLIT_DIVIDER_H))
        .flex_shrink_0()
        .w_full()
        .bg(rgb(theme().surface))
        .hover(|s| s.bg(rgb(theme().color_branch)).cursor_row_resize())
        .cursor_row_resize()
        .on_drag(
            DividerDrag { kind: DividerKind::InspectorSplit },
            |_drag, _pos, _window, cx| cx.new(|_| DividerGhost),
        );

    // ── Fixed header region (title, meta, badges, actions) — not scrolled ──
    let header_region = div()
        .flex().flex_col().flex_shrink_0()
        .px_3().pt_2().pb_1()
        .child(title_el)
        .child(meta_row)
        .child(badges_row)
        .child(actions_row);

    // ── Split region: message │ divider │ files ───────────────────────────
    // Grouped under one flex_1 column so the split ratio is relative to the
    // *remaining* height (excluding the variable-height header), and so a
    // measuring canvas can record the region's real window coordinates for
    // the drag handler (static offsets miss the header height — that was the
    // user-visible "jumps ~2cm on drag start" bug).
    let measure = {
        let geom = split_geom.clone();
        canvas(
            move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
            move |bounds: Bounds<Pixels>, _prepaint: (), _window: &mut Window, _cx: &mut App| {
                let top = f32::from(bounds.origin.y);
                let bottom = top + f32::from(bounds.size.height);
                geom.set((top, bottom));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };
    let split_region = div()
        .flex_1()
        .min_h(px(0.))
        .relative()
        .flex()
        .flex_col()
        .child(measure)
        .child(message_box)
        .child(split_divider)
        .child(files_box);

    // ── Outer panel: header │ split region ────────────────────────────────
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex().flex_col()
        .bg(rgb(theme().panel))
        .child(header_region)
        .child(split_region)
}

/// Parse `name  <email>  date` (detail_panel format) into `(name, email)`.
///
/// `chars()`-safe: only splits on ASCII markers (`  <`, `>`), never byte-slices
/// into multi-byte sequences, so Japanese / CJK names are preserved.  Falls back
/// to the whole string as the name (and empty email) if the markers are absent.
fn parse_author(line: &str) -> (String, String) {
    if let Some(lt) = line.find("  <") {
        let name = line[..lt].trim().to_string();
        let rest = &line[lt + 3..];
        let email = match rest.find('>') {
            Some(gt) => rest[..gt].to_string(),
            None => String::new(),
        };
        (name, email)
    } else {
        (line.trim().to_string(), String::new())
    }
}

/// Tooltip entity for the short-hash chip — shows the full 40-hex SHA.
struct HashTooltip {
    sha: SharedString,
}

impl gpui::Render for HashTooltip {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div()
            .flex().flex_col()
            .px_2().py_1()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .text_xs().text_color(rgb(theme().text_main));
        for line in self.sha.as_ref().split('\n') {
            col = col.child(div().child(SharedString::from(line.to_string())));
        }
        col
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Render a clickable action button.
fn action_button(
    id: &'static str,
    label: &'static str,
    color: u32,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px(px(6.))
        .py(px(2.))
        .rounded_sm()
        .bg(rgb(theme().surface))
        .text_xs()
        .text_color(rgb(color))
        .on_click(click)
        .hover(|style| style.bg(rgb(theme().selected)))
        .child(SharedString::from(label))
}

/// Change-kind badge char and colour.
fn change_badge(change: &ChangeKind) -> (&'static str, u32) {
    match change {
        ChangeKind::Added          => ("A", theme().change_added),
        ChangeKind::Modified       => ("M", theme().change_modified),
        ChangeKind::Deleted        => ("D", theme().change_deleted),
        ChangeKind::Renamed { .. } => ("R", theme().change_renamed),
        ChangeKind::TypeChange     => ("T", theme().change_typechange),
    }
}

/// Sort key for badge priority: HeadBranch=0, Branch=1, Tag=2, Remote=3.
fn badge_priority(kind: &BadgeKind) -> u8 {
    match kind {
        BadgeKind::HeadBranch => 0,
        BadgeKind::Branch     => 1,
        BadgeKind::Tag        => 2,
        BadgeKind::Remote     => 3,
    }
}
