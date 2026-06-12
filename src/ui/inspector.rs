//! Commit Inspector panel — W2-INSPECTOR / ADR-0015
//!
//! Extracted from `mod.rs` `render_detail_panel` and restructured as:
//!   1. Summary  (title · short SHA · Copy button · ref badges)
//!   2. Metadata (author/authored · committer/committed · parents · full SHA · message)
//!   3. Contextual Actions  (Create branch · Cherry-pick · Copy SHA)
//!   4. Changed Files  (tree or flat path list, Path⇄Tree toggle)

use gpui::{Context, IntoElement, SharedString, div, prelude::*, px, rgb};

use kagi::git::{ChangeKind, CommitId, FileStatus};

use super::{
    KagiApp,
    commit_list::{BadgeKind, RefBadge},
    context_menu::CommitAction,
    detail_panel::CommitDetail,
    file_tree,
};

// ── Catppuccin Mocha palette — mirrors mod.rs (kept local to avoid pub(super)) ──
const BG_BASE:      u32 = 0x1e1e2e;
const BG_SURFACE:   u32 = 0x313244;
const BG_SELECTED:  u32 = 0x45475a;
const BG_PANEL:     u32 = 0x181825;
const TEXT_MAIN:    u32 = 0xcdd6f4;
const TEXT_SUB:     u32 = 0xa6adc8;
const TEXT_MUTED:   u32 = 0x585b70;
const TEXT_LABEL:   u32 = 0x6c7086;
const COLOR_HEAD:   u32 = 0xf38ba8;
const COLOR_BRANCH: u32 = 0x89b4fa;
const COLOR_REMOTE: u32 = 0xa6e3a1;
const COLOR_TAG:    u32 = 0xfab387;

// ── Change-kind badge colours ─────────────────────────────────────────────
const COLOR_ADDED:      u32 = 0xa6e3a1;
const COLOR_MODIFIED:   u32 = 0xf9e2af;
const COLOR_DELETED:    u32 = 0xf38ba8;
const COLOR_RENAMED:    u32 = 0x89b4fa;
const COLOR_TYPECHANGE: u32 = 0x585b70;
const COLOR_DIR:        u32 = 0x6c7086;

const MAX_FILES: usize = 100;
const MAX_BADGE_CHARS: usize = 20;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ─────────────────────────────────────────────────────────────────────────────

/// Render the Commit Inspector right panel.
///
/// Section order (ADR-0015):
///   Summary → Metadata → Contextual Actions → Changed Files
///
/// `tree_view` — when `true` render the tree; when `false` render flat paths.
#[allow(clippy::too_many_arguments)]
pub fn render_inspector(
    d: CommitDetail,
    at: CommitId,
    badges: Vec<RefBadge>,
    changed_files: Option<Vec<FileStatus>>,
    changed_files_for_click: Option<Vec<FileStatus>>,
    active_file: Option<usize>,
    tree_view: bool,
    panel_width: f32,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Suppress unused warning (kept for symmetry / future diff-on-click).
    let _ = changed_files_for_click;

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

    // ── Parents value ─────────────────────────────────────────────────────
    let parents_value = if d.parent_ids.is_empty() {
        SharedString::from("(root commit)")
    } else {
        SharedString::from(
            d.parent_ids.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("  ")
        )
    };

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
                .text_color(rgb(TEXT_MAIN))
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
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm().text_color(rgb(COLOR_DIR))
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
                            this.open_main_diff_commit(fi);
                            cx.notify();
                        });
                        div()
                            .id(("file-row", fi))
                            .flex().flex_row().items_center().gap_1()
                            .pl(px(indent)).mb_px()
                            .when(active_file == Some(fi), |el| el.bg(rgb(BG_SELECTED)).rounded_sm())
                            .on_click(click)
                            .child(
                                div().w(px(14.)).flex_shrink_0()
                                    .text_sm().text_color(rgb(badge_color))
                                    .child(SharedString::from(badge_char)),
                            )
                            .child(
                                div().flex_1()
                                    .text_sm().text_color(rgb(TEXT_MAIN))
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
                    this.open_main_diff_commit(fi);
                    cx.notify();
                });
                div()
                    .id(("file-flat", fi))
                    .flex().flex_row().items_center().gap_1()
                    .mb_px()
                    .when(active_file == Some(fi), |el| el.bg(rgb(BG_SELECTED)).rounded_sm())
                    .on_click(click)
                    .child(
                        div().w(px(14.)).flex_shrink_0()
                            .text_sm().text_color(rgb(badge_color))
                            .child(SharedString::from(badge_char)),
                    )
                    .child(
                        div().flex_1()
                            .text_sm().text_color(rgb(TEXT_MAIN))
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
        "+ Create branch here",
        COLOR_BRANCH,
        create_branch_click,
    );

    // ── "Cherry-pick onto HEAD" button (T016) ────────────────────────────
    let cherry_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.open_cherry_pick_modal(at_for_cherry.clone());
        cx.notify();
    });
    let cherry_pick_button = action_button(
        "cherry-pick-btn",
        "\u{1f352} Cherry-pick onto HEAD branch",
        0xcba6f7, // Catppuccin mauve
        cherry_click,
    );

    // ── "Copy SHA" button in Actions section ─────────────────────────────
    let copy_sha_button = action_button(
        "copy-sha-actions-btn",
        "Copy SHA",
        0x89dceb, // Catppuccin sky
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
    let path_bg = if path_active { BG_SELECTED } else { BG_SURFACE };
    let tree_bg = if tree_active { BG_SELECTED } else { BG_SURFACE };
    let path_col = if path_active { TEXT_MAIN } else { TEXT_SUB };
    let tree_col = if tree_active { TEXT_MAIN } else { TEXT_SUB };

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

    // ── Files section ─────────────────────────────────────────────────────
    let section_label = match &changed_files {
        None => SharedString::from("Changed files"),
        Some(files) => SharedString::from(format!("Changed files ({})", files.len())),
    };

    let mut files_section = div()
        .flex().flex_col()
        .mt_2()
        .child(
            div().flex().flex_row().items_center().justify_between().mb_1()
                .child(
                    div()
                        .text_sm().text_color(rgb(TEXT_LABEL))
                        .child(section_label),
                )
                .child(toggle_row),
        );

    if changed_files.is_none() {
        files_section = files_section.child(
            div().text_sm().text_color(rgb(TEXT_MUTED))
                .child(SharedString::from("(diff unavailable)")),
        );
    } else {
        for row in tree_element_rows {
            files_section = files_section.child(row);
        }
        if let Some(remaining) = truncated_count {
            files_section = files_section.child(
                div().text_sm().text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from(format!("\u{2026} and {} more", remaining))),
            );
        }
    }

    // ── Summary section ───────────────────────────────────────────────────
    // Title = first line of the commit message.
    let title_text: SharedString = SharedString::from(
        d.full_message.as_ref().lines().next().unwrap_or("").to_string()
    );

    // Copy button (Summary)
    let copy_summary_button = div()
        .id("copy-sha-summary-btn")
        .px_1()
        .rounded_sm()
        .bg(rgb(BG_SURFACE))
        .text_xs()
        .text_color(rgb(TEXT_MUTED))
        .hover(|style| style.bg(rgb(BG_SELECTED)).text_color(rgb(TEXT_MAIN)))
        .on_click(copy_sha_click1)
        .child(SharedString::from("copy"));

    // Ref badges row
    let badges_row = {
        let mut row = div().flex().flex_row().items_center().flex_wrap().gap_1();
        let mut by_prio = badges;
        by_prio.sort_by_key(|b| badge_priority(&b.kind));
        for badge in &by_prio {
            let color = match badge.kind {
                BadgeKind::HeadBranch => COLOR_HEAD,
                BadgeKind::Branch     => COLOR_BRANCH,
                BadgeKind::Remote     => COLOR_REMOTE,
                BadgeKind::Tag        => COLOR_TAG,
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
                    .text_color(rgb(BG_BASE))
                    .text_xs().flex_shrink_0()
                    .child(label),
            );
        }
        row
    };

    let summary_section = div()
        .flex().flex_col()
        .mb_2()
        .child(
            // Title
            div()
                .text_color(rgb(TEXT_MAIN))
                .mb_1()
                .truncate()
                .child(title_text),
        )
        .child(
            // short SHA + Copy button
            div().flex().flex_row().items_center().gap_1().mb_1()
                .child(
                    div().text_sm().text_color(rgb(TEXT_SUB))
                        .child(short_sha),
                )
                .child(copy_summary_button),
        )
        .child(badges_row);

    // ── Assemble scroll content ───────────────────────────────────────────
    // Layout: Summary → Metadata → Actions → Files
    let mut scroll_content = div()
        .flex().flex_col()
        .px_3().py_2()
        // ── 1. SUMMARY ───────────────────────────────────────────────────
        .child(summary_section)
        // ── 2. METADATA ──────────────────────────────────────────────────
        .child(field("Author", d.author_line))
        .when_some(d.committer_line, |el, c| el.child(field("Committer", c)))
        .child(field("Committed", d.committed_date))
        .child(field("Parents", parents_value))
        .child(field("SHA", d.full_sha))
        // Message block
        .child(
            div()
                .flex().flex_col()
                .mb_2()
                .child(
                    div()
                        .text_sm().text_color(rgb(TEXT_LABEL))
                        .mb_1()
                        .child(SharedString::from("Message")),
                ),
        );

    for line_el in message_lines {
        scroll_content = scroll_content.child(line_el);
    }

    scroll_content = scroll_content
        // ── 3. CONTEXTUAL ACTIONS ────────────────────────────────────────
        .child(
            div()
                .flex().flex_col()
                .mt_3().mb_1()
                .child(
                    div()
                        .text_sm().text_color(rgb(TEXT_LABEL))
                        .mb_1()
                        .child(SharedString::from("Actions")),
                )
                .child(create_branch_button)
                .child(cherry_pick_button)
                .child(copy_sha_button),
        )
        // ── 4. CHANGED FILES ─────────────────────────────────────────────
        .child(files_section);

    // ── Outer panel ───────────────────────────────────────────────────────
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex().flex_col()
        .bg(rgb(BG_PANEL))
        .child(
            div()
                .id("detail-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(scroll_content),
        )
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Render one labelled field row (label above, single-line value with truncate).
fn field(label: &'static str, value: SharedString) -> impl IntoElement {
    div()
        .flex().flex_col()
        .mb_2()
        .child(
            div()
                .text_sm().text_color(rgb(TEXT_LABEL))
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
                .truncate()
                .child(value),
        )
}

/// Render a clickable action button.
fn action_button(
    id: &'static str,
    label: &'static str,
    color: u32,
    click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .mb_1()
        .px_2().py_1()
        .rounded_sm()
        .bg(rgb(BG_SURFACE))
        .text_sm()
        .text_color(rgb(color))
        .on_click(click)
        .hover(|style| style.bg(rgb(BG_SELECTED)))
        .child(SharedString::from(label))
}

/// Change-kind badge char and colour.
fn change_badge(change: &ChangeKind) -> (&'static str, u32) {
    match change {
        ChangeKind::Added          => ("A", COLOR_ADDED),
        ChangeKind::Modified       => ("M", COLOR_MODIFIED),
        ChangeKind::Deleted        => ("D", COLOR_DELETED),
        ChangeKind::Renamed { .. } => ("R", COLOR_RENAMED),
        ChangeKind::TypeChange     => ("T", COLOR_TYPECHANGE),
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
