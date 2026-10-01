//! Graph sidebar row model and item renderers (#864).

use std::collections::HashSet;

use gpui::{div, prelude::*, px, rgb, Context, Entity, SharedString, UniformListScrollHandle};
use gpui_component::input::InputState;
use gpui_component::tooltip::Tooltip;

use kagi_git::CommitId;

#[cfg(test)]
use super::sidebar_rows::{
    build_sidebar_rows, group_by_prefix, group_key, group_remotes, remote_group_key, remote_key,
    split_first_segment, GroupRow, RemoteRow,
};
use super::theme::{self, theme};
use super::{BranchDrag, BranchDragGhost, KagiApp, Msg};

/// Uniform row height (unscaled) used for **every** virtualized sidebar row.
///
/// `uniform_list` requires a single fixed row height — it measures the first
/// item and applies that height to all of them. Every sidebar row (section
/// header, group header, branch/remote/tag/worktree/stash leaf, and the
/// placeholder rows) is therefore pinned to this height so the virtualized
/// list scrolls correctly regardless of which row happens to be first.
pub(super) const SIDEBAR_ROW_H: f32 = 24.0;

/// Default sidebar width in pixels (T023). Previously `mod.rs::SIDEBAR_DEFAULT`.
///
/// 240, not the original 200: branch names under LOCAL BRANCHES are grouped by
/// prefix and indented, so the common `feature/...` leaf used to be ellipsised
/// at the default width (user request). Well inside `SIDEBAR_MIN..SIDEBAR_MAX`,
/// and this is only the starting width — the divider drag still owns it for the
/// rest of the session (it is not written to `settings.json`).
const SIDEBAR_DEFAULT_WIDTH: f32 = 240.0;

/// Consolidated Repository-Navigator (left sidebar) state.
///
/// App-global (not per-tab), preserved across repository reloads. `new()`
/// restores the six-pane weights and collapsed sections from settings once;
/// the filter `InputState` is created lazily on first focus.
pub struct SidebarState {
    /// Current sidebar width in pixels (T023: user-resizable).
    pub width: f32,
    /// Independent virtual-list scroll positions; one per fixed Graph pane.
    pub scroll_handles: [UniformListScrollHandle; 6],
    /// Cached navigator rows. Six virtual lists index contiguous ranges of
    /// this single Vec, so unchanged frames only construct visible leaves.
    pub rows: Vec<SidebarRow>,
    /// Contiguous header+leaf ranges in `rows`, recomputed only on a row rebuild.
    pub pane_ranges: [std::ops::Range<usize>; 6],
    /// Relative heights for expanded pane bodies; collapse never mutates them.
    pub pane_weights: [u16; 6],
    /// Measured window-space bounds for drag calculations, including zoom.
    pub pane_geom: [std::rc::Rc<std::cell::Cell<(f32, f32)>>; 6],
    /// T-PERF-RENDER-002 (ADR-0116 Wave 2): fingerprint of the inputs that
    /// produced the cached `rows`. `render` hashes cheap input revisions,
    /// collapse sets and filter text; unchanged frames do not rebuild refs.
    /// A rebuild also refreshes the six pane ranges.
    pub rows_fingerprint: u64,
    /// #354: each row's 1-based `(position, size)` among its tree siblings,
    /// for the accessibility tree. Derived from every row, so it is cached
    /// for the `rows_fingerprint` it was computed at (`tree_positions_for`)
    /// and a scroll batch only reads it; `refresh_tree_positions` recomputes
    /// when `rows` has been rebuilt since.
    pub(super) tree_positions: Vec<(usize, usize)>,
    tree_positions_for: Option<u64>,
    /// Sole runtime owner of section collapse, restored and persisted as a mask.
    pub collapsed: HashSet<&'static str>,
    /// Lazy `InputState` for the filter input (gpui-component IME 対応); created
    /// on first click of the filter area (requires `&mut Window`).
    pub filter: Option<Entity<InputState>>,
    /// Whether the navigator is shown (View → Toggle Sidebar). Default `true`.
    pub visible: bool,
    /// Transient pointer gesture, never repository data (ADR-0199).
    pub swipe: kagi_domain::sidebar_swipe::SidebarSwipe,
    /// Generation of the running settle animation. Bumped when a settle is
    /// superseded or abandoned so its frame loop retires instead of ticking
    /// the next gesture twice as fast.
    pub settle_gen: u64,
}

impl SidebarState {
    pub fn new() -> Self {
        let layout = super::settings::Settings::load().sidebar_pane_layout();
        let mut collapsed = HashSet::new();
        for (index, section) in super::sidebar_panes::SECTIONS.iter().enumerate() {
            if layout.collapsed_mask & (1 << index) != 0 {
                collapsed.insert(*section);
            }
        }
        Self {
            width: SIDEBAR_DEFAULT_WIDTH,
            scroll_handles: std::array::from_fn(|_| UniformListScrollHandle::new()),
            rows: Vec::new(),
            pane_ranges: std::array::from_fn(|_| 0..0),
            pane_weights: layout.weights,
            pane_geom: std::array::from_fn(|_| Default::default()),
            rows_fingerprint: u64::MAX,
            tree_positions: Vec::new(),
            tree_positions_for: None,
            collapsed,
            filter: None,
            visible: true,
            swipe: Default::default(),
            settle_gen: 0,
        }
    }

    /// Bring `tree_positions` in line with `rows`. `rows` and
    /// `rows_fingerprint` are only replaced together (the rebuild in
    /// `render`), so a fingerprint or length mismatch means the rows were
    /// rebuilt — after a refresh, collapse / expand, filter edit or session
    /// switch — and the positions are recomputed once, O(rows) without names.
    pub(super) fn refresh_tree_positions(&mut self) {
        if self.tree_positions_for == Some(self.rows_fingerprint)
            && self.tree_positions.len() == self.rows.len()
        {
            return;
        }
        let levels: Vec<usize> = self.rows.iter().map(super::sidebar_a11y::level).collect();
        self.tree_positions = kagi_ui_core::tree_a11y::sibling_positions(&levels);
        self.tree_positions_for = Some(self.rows_fingerprint);
    }
}

impl Default for SidebarState {
    fn default() -> Self {
        Self::new()
    }
}

// W9-THEME: all colours come from `theme()` (see theme.rs).

// ──────────────────────────────────────────────────────────────
// Section keys (static strings used in SidebarState::collapsed)
// ──────────────────────────────────────────────────────────────

pub const SECTION_LOCAL: &str = "local";
pub const SECTION_REMOTE: &str = "remote";
pub const SECTION_TAGS: &str = "tags";
pub const SECTION_WORKTREES: &str = "worktrees";
pub const SECTION_STASHES: &str = "stashes";
/// GitHub Phase 1: open pull requests (from `gh`).
pub const SECTION_PRS: &str = "prs";
/// PR sub-group collapse keys (in `branch_groups_collapsed`).
pub const PR_GROUP_MINE: &str = "prs:mine";
pub const PR_GROUP_REVIEW: &str = "prs:review";
pub const PR_GROUP_OTHERS: &str = "prs:others";

/// Build a `.tooltip(...)` closure showing the full (untruncated) name.
/// Row labels are single-line + ellipsized, so the tooltip is how the user
/// reads a name that doesn't fit the sidebar width.
pub(super) fn name_tooltip(
    full: SharedString,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |window, cx| Tooltip::new(full.clone()).build(window, cx)
}

// ──────────────────────────────────────────────────────────────
// PERF-SIDEBAR-VIRT: flat row model for `uniform_list`
// ──────────────────────────────────────────────────────────────
//
// Flatten all sections once when the input fingerprint changes; store six
// contiguous ranges into that Vec on `SidebarState`. Each Graph pane draws only
// its visible leaf rows via its own `uniform_list` and the common row renderer.
// Moving a divider or scrolling a pane never regroups thousands of refs.

/// One flattened, virtualized sidebar row. A section header starts each pane's
/// contiguous range and is drawn outside its leaf list so it stays pinned.
/// Every virtualized leaf has the same `SIDEBAR_ROW_H`.
#[derive(Debug, Clone)]
pub enum SidebarRow {
    /// A top-level pane header. `section` is the static collapse key.
    SectionHeader {
        section: &'static str,
        title: &'static str,
        count: usize,
        collapsed: bool,
    },
    /// A `/`-prefix group header inside LOCAL BRANCHES (collapse key
    /// `local:<prefix>`).
    LocalGroupHeader {
        key: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A local branch leaf. `display_label` is what shows (prefix-stripped for
    /// grouped leaves); `name` is the full branch name used for handlers/id.
    LocalBranchLeaf {
        name: String,
        display_label: String,
        is_head: bool,
        indented: bool,
    },
    /// REMOTE BRANCHES level-1 header (a remote name). Collapse key
    /// `remote:<remote>`.
    RemoteHeader {
        key: String,
        remote: String,
        count: usize,
        collapsed: bool,
    },
    /// REMOTE BRANCHES level-2 sub-group header. Collapse key
    /// `remote:<remote>:<prefix>`.
    RemoteSubGroup {
        key: String,
        prefix: String,
        count: usize,
        collapsed: bool,
    },
    /// A remote branch leaf. `display` is the full `origin/…` name (used for
    /// jump/tooltip/id/drag); `display_label` is the prefix-stripped label;
    /// `depth` drives indentation (1 = direct under remote, 2 = under
    /// sub-group).
    RemoteLeaf {
        display: String,
        display_label: String,
        target: CommitId,
        depth: u8,
    },
    /// A tag leaf.
    Tag { name: String, target: CommitId },
    /// A worktree leaf.
    Worktree {
        name: String,
        /// The working-tree path itself. `path_label` is display text — lossy
        /// for non-UTF-8 paths and control-byte sanitized before rendering — so
        /// the menu's path actions use this instead of parsing the label back.
        path: std::path::PathBuf,
        path_label: String,
        is_current: bool,
        is_main: bool,
        locked: bool,
        /// First port of the worktree's stored block, shown as
        /// `localhost:<port>` (#855). `None` when it has none.
        port: Option<u16>,
    },
    /// A stash leaf.
    Stash { index: usize, message: String },
    /// GitHub Phase 1: PR sub-group header (Mine / Review requested / Others).
    PrGroupHeader {
        key: &'static str,
        title: &'static str,
        count: usize,
        collapsed: bool,
    },
    /// GitHub Phase 1: an open pull request. `stacked` = its base is another
    /// open PR's head.
    PullRequest {
        pr: kagi_domain::github::PullRequest,
        stacked: bool,
    },
}

// ──────────────────────────────────────────────────────────────
// Per-row builders (called from the `uniform_list` processor)
// ──────────────────────────────────────────────────────────────

/// Dispatch a single flat row to its renderer. Reads live data from `this`
/// (upstream info, commit index) so handlers see current state, mirroring the
/// commit-list per-row builders.
pub(super) fn build_sidebar_row(
    this: &KagiApp,
    row: &SidebarRow,
    now_secs: i64,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    match row {
        SidebarRow::SectionHeader {
            section,
            title,
            count,
            collapsed,
        } => build_section_header(section, title, *count, *collapsed, cx),
        SidebarRow::LocalGroupHeader {
            key,
            prefix,
            count,
            collapsed,
        } => build_group_header(key, prefix, *count, *collapsed, theme::scaled_px(20.), cx),
        SidebarRow::LocalBranchLeaf {
            name,
            display_label,
            is_head,
            indented,
        } => build_local_branch_leaf(this, name, *is_head, display_label, *indented, now_secs, cx),
        SidebarRow::RemoteHeader {
            key,
            remote,
            count,
            collapsed,
        } => build_group_header(key, remote, *count, *collapsed, theme::scaled_px(20.), cx),
        SidebarRow::RemoteSubGroup {
            key,
            prefix,
            count,
            collapsed,
        } => build_group_header(key, prefix, *count, *collapsed, theme::scaled_px(32.), cx),
        SidebarRow::RemoteLeaf {
            display,
            display_label,
            target,
            depth,
        } => build_remote_leaf(this, display, display_label, target, *depth, now_secs, cx),
        SidebarRow::Tag { name, target } => build_tag_row(this, name, target.clone(), cx),
        SidebarRow::Worktree {
            name,
            path,
            path_label,
            is_current,
            is_main,
            locked,
            port,
        } => super::sidebar_worktree_row::build_worktree_row(
            super::sidebar_worktree_row::WorktreeRowFacts {
                name,
                path,
                path_label,
                is_current: *is_current,
                is_main: *is_main,
                locked: *locked,
                port: *port,
            },
            this,
            cx,
        ),
        SidebarRow::Stash { index, message } => build_stash_row(*index, message, cx),
        SidebarRow::PrGroupHeader {
            key,
            title,
            count,
            collapsed,
        } => build_group_header(key, title, *count, *collapsed, theme::scaled_px(20.), cx),
        SidebarRow::PullRequest { pr, stacked } => build_pr_row(pr, *stacked, cx),
    }
}

/// Section header row (LOCAL BRANCHES / …). Click toggles `sidebar_collapsed`.
fn build_section_header(
    section: &'static str,
    title: &'static str,
    count: usize,
    collapsed: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = SharedString::from(format!(
        "{} {} ({})",
        if collapsed { "\u{25b8}" } else { "\u{25be}" },
        title,
        count
    ));
    let toggle = cx.listener(
        move |this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            this.sidebar.toggle_section(section);
            cx.notify();
        },
    );
    div()
        .id(SharedString::from(format!("sidebar-section-{}", section)))
        .relative()
        .child(super::e2e::measure_inside(section))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_muted))
        .on_click(toggle)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(label)
        .into_any()
}

/// A `/`-prefix group header (local groups, remote level-1, remote level-2).
/// Click toggles `branch_groups_collapsed` for `key`. `left_pad` sets indent.
fn build_group_header(
    key: &str,
    label_text: &str,
    count: usize,
    collapsed: bool,
    left_pad: gpui::Pixels,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let arrow = if collapsed { "\u{25b8}" } else { "\u{25be}" };
    // issue #414: `label_text` is a remote name / branch prefix (remote-origin).
    let label_text = kagi_domain::text_safety::sanitize_control_bytes(label_text);
    let glabel = SharedString::from(format!("{} {} ({})", arrow, label_text, count));
    let key_for_toggle = key.to_string();
    let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.with_ui(|ui| ui.toggle_branch_group(&key_for_toggle));
        cx.notify();
    });
    div()
        .id(SharedString::from(format!("sidebar-group-{}", key)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .pl(left_pad)
        .pr_3()
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .overflow_hidden()
        .on_click(toggle)
        .hover(|s| s.bg(rgb(theme().surface)))
        .child(div().flex_1().truncate().child(glabel))
        .into_any()
}

/// Bound the root flex width and give the suffix a preferential shrink share.
fn branch_row_meta(
    row: gpui::Stateful<gpui::Div>,
    age: std::borrow::Cow<'static, str>,
    upstream: Option<SharedString>,
    pr: Option<(SharedString, u32)>,
) -> gpui::Stateful<gpui::Div> {
    row.w_full()
        .child(
            div()
                .flex_shrink(1000.)
                .min_w(px(0.))
                .truncate()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("  \u{00b7} {age}"))),
        )
        .when_some(upstream, |el, text| {
            el.child(
                div()
                    .flex_shrink_0()
                    .ml_2()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(text),
            )
        })
        .when_some(pr, |el, (text, color)| {
            el.child(
                div()
                    .flex_shrink_0()
                    .ml_2()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme().selected))
                    .text_xs()
                    .text_color(rgb(color))
                    .child(text),
            )
        })
}

/// A local branch leaf. HEAD is a merge drop target that only jumps on click;
/// a non-HEAD row drags, checks out on double click, and carries the delete.
fn build_local_branch_leaf(
    this: &KagiApp,
    branch_name: &str,
    is_head: bool,
    display_label: &str,
    indented: bool,
    now_secs: i64,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let upstream_label: Option<SharedString> = this
        .view()
        .branch_upstream_info
        .get(branch_name)
        .and_then(|u| match u.counts {
            // #355: a skipped count is unknown — "—", never a hidden 0/0.
            None => Some(SharedString::from("\u{2014}")),
            Some(c) if c.ahead > 0 || c.behind > 0 => Some(SharedString::from(format!(
                "\u{2191}{} \u{2193}{}",
                c.ahead, c.behind
            ))),
            Some(_) => None,
        });

    // Show this session's open PR alongside its branch.
    let pr_badge: Option<(SharedString, u32)> = this
        .ui()
        .github_prs
        .iter()
        .find(|p| p.head == branch_name)
        .map(|p| {
            use kagi_domain::github::CiState;
            let (glyph, color) = match p.ci {
                CiState::Success => ("\u{2713}", theme().color_success),
                CiState::Failure => ("\u{2717}", theme().color_blocker),
                CiState::Pending => ("\u{25CF}", theme().color_warning),
                CiState::None => ("", theme().text_muted),
            };
            (
                SharedString::from(format!("#{} {}", p.number, glyph)),
                color,
            )
        });

    // issue #356: branch names are remote-origin — neutralize control bytes in
    // the visible label (the raw `branch_name` operand stays untouched).
    let display_label = kagi_domain::text_safety::sanitize_control_bytes(display_label);
    let view = this.view();
    let (age, committed) = view.tip_labels(view.branch_targets.get(branch_name), now_secs);
    let label = SharedString::from(if is_head {
        format!("\u{2713} {display_label}")
    } else {
        display_label
    });
    let text_color = if is_head {
        theme().color_success
    } else {
        theme().text_main
    };
    let full_name = SharedString::from(format!("{branch_name}\n{committed}"));
    let left_pad = if indented {
        theme::scaled_px(28.)
    } else {
        theme::scaled_px(12.)
    };

    if is_head {
        let branch_for_click = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let head_click = cx.listener(move |this: &mut KagiApp, _e: &gpui::ClickEvent, _w, cx| {
            this.jump_to_branch(&branch_for_click);
            cx.notify();
        });
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        let drop_handler = cx.listener(
            move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                this.start_merge_from_drag(payload.name.clone(), cx);
                cx.notify();
            },
        );
        div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_sm()
            .text_color(rgb(text_color))
            .overflow_hidden()
            .on_click(head_click)
            .on_mouse_down(gpui::MouseButton::Right, menu_click)
            .drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                style
                    .bg(rgb(theme().selected))
                    .border_color(rgb(theme().color_branch))
            })
            .on_drop::<BranchDrag>(drop_handler)
            .hover(|style| style.bg(rgb(theme().surface)))
            .tooltip(name_tooltip(full_name))
            .child(div().flex_auto().truncate().child(label))
            .map(|el| branch_row_meta(el, age, upstream_label, pr_badge))
            .into_any()
    } else {
        let branch_for_dbl = branch_name.to_string();
        let branch_for_delete = branch_name.to_string();
        let branch_for_menu = branch_name.to_string();
        let branch_for_drag = branch_name.to_string();
        let click_handler = cx.listener(
            move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    this.open_plan_modal(branch_for_dbl.clone());
                } else {
                    this.jump_to_branch(&branch_for_dbl);
                }
                cx.notify();
            },
        );
        let delete_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.open_delete_branch_modal(branch_for_delete.clone(), cx);
                cx.notify();
            },
        );
        let menu_click = cx.listener(
            move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                this.open_local_branch_menu(branch_for_menu.clone(), event.position);
                cx.stop_propagation();
                cx.notify();
            },
        );
        // ADR-0144: a non-HEAD row is both a drag source and a drop target.
        // Dropping here merges into a branch that is not checked out — only
        // its ref moves, the working tree is untouched.
        let branch_for_drop = branch_name.to_string();
        let drop_handler = cx.listener(
            move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                this.start_merge_into_from_drag(payload.name.clone(), branch_for_drop.clone(), cx);
                cx.notify();
            },
        );
        let row = div()
            .id(SharedString::from(format!(
                "sidebar-branch-{}",
                branch_name
            )))
            .h(theme::scaled_px(SIDEBAR_ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .pl(left_pad)
            .pr_3()
            .text_sm()
            .text_color(rgb(text_color))
            .overflow_hidden()
            .on_click(click_handler)
            .on_mouse_down(gpui::MouseButton::Right, menu_click)
            .drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                style
                    .bg(rgb(theme().selected))
                    .border_color(rgb(theme().color_branch))
            })
            .on_drop::<BranchDrag>(drop_handler)
            .on_drag(
                BranchDrag {
                    name: branch_for_drag.clone(),
                },
                move |drag: &BranchDrag, _pos, _window, cx| {
                    let name = SharedString::from(drag.name.clone());
                    cx.new(|_| BranchDragGhost { name })
                },
            )
            .hover(|style| style.bg(rgb(theme().surface)))
            .tooltip(name_tooltip(full_name))
            .child(div().flex_auto().truncate().child(label))
            .map(|el| branch_row_meta(el, age, upstream_label, pr_badge))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-delete-{}",
                        branch_name
                    )))
                    .flex_shrink_0()
                    .ml_1()
                    .px_1()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .on_click(delete_handler)
                    .hover(|s| s.text_color(rgb(theme().color_blocker)))
                    .child(SharedString::from("\u{00d7}")),
            )
            .into_any();
        super::e2e::measure_control(format!("sidebar-local-{branch_name}"), row)
    }
}

/// A remote branch leaf: a merge drag source, clickable while its tip is loaded.
fn build_remote_leaf(
    this: &KagiApp,
    display: &str,
    display_label: &str,
    rb_target: &CommitId,
    depth: u8,
    now_secs: i64,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let can_jump = this.view().commit_row_index.contains_key(rb_target);
    let (age, committed) = this.view().tip_labels(Some(rb_target), now_secs);
    // issue #414: remote branch names are the most literally remote-derived
    // strings in the sidebar. Neutralize control bytes in the *displayed* label
    // and tooltip; the raw `display` operand (drag/menu/id) stays untouched.
    let safe_display = kagi_domain::text_safety::sanitize_control_bytes(display);
    let full_name = SharedString::from(format!("{safe_display}\n{committed}"));
    let label = kagi_domain::text_safety::sanitize_control_bytes(display_label);
    let drag_name = display.to_string();
    let left_pad = match depth {
        0 => theme::scaled_px(12.),
        1 => theme::scaled_px(28.),
        _ => theme::scaled_px(44.),
    };
    let display_for_menu = display.to_string();
    let target_for_menu = rb_target.clone();
    let menu_click = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_remote_branch_menu(
                display_for_menu.clone(),
                target_for_menu.clone(),
                event.position,
            );
            cx.stop_propagation();
            cx.notify();
        },
    );

    let base = div()
        .id(SharedString::from(format!("sidebar-remote-{}", display)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .pl(left_pad)
        .pr_3()
        .text_sm()
        .text_color(rgb(theme().color_remote))
        .overflow_hidden()
        .on_mouse_down(gpui::MouseButton::Right, menu_click)
        .cursor_grab()
        .on_drag(
            BranchDrag {
                name: drag_name.clone(),
            },
            move |drag: &BranchDrag, _pos, _window, cx| {
                let name = SharedString::from(drag.name.clone());
                cx.new(|_| BranchDragGhost { name })
            },
        )
        .hover(|style| style.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(full_name))
        .child(div().flex_auto().truncate().child(label))
        .map(|el| branch_row_meta(el, age, None, None));

    if can_jump {
        let target_for_jump = rb_target.clone();
        let click_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.jump_to_commit(&target_for_jump);
                cx.notify();
            },
        );
        base.on_click(click_handler).into_any()
    } else {
        base.into_any()
    }
}

/// A tag leaf — click jumps to the tag's target when it is in the loaded graph.
fn build_tag_row(
    this: &KagiApp,
    tag_name: &str,
    tag_target: CommitId,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // issue #356: tag names are remote-origin — neutralize control bytes in
    // the visible label (the raw `tag_name` operand stays untouched).
    let tag_label = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(tag_name));
    let full_name = SharedString::from(tag_name.to_string());
    let can_jump = this.view().commit_row_index.contains_key(&tag_target);
    // Right-click → the tag menu (ADR-0140). Tags were the only sidebar ref
    // with no menu, so publishing one meant leaving kagi for a terminal.
    let menu_name = tag_name.to_string();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.open_tag_menu(menu_name.clone(), e.position);
            cx.stop_propagation();
            cx.notify();
        },
    );
    let base = div()
        .id(SharedString::from(format!("sidebar-tag-{}", tag_name)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().color_tag))
        .overflow_hidden()
        .tooltip(name_tooltip(full_name))
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .child(div().flex_1().truncate().child(tag_label));
    if can_jump {
        let click_handler = cx.listener(
            move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                this.jump_to_commit(&tag_target);
                cx.notify();
            },
        );
        base.hover(|style| style.bg(rgb(theme().surface)))
            .on_click(click_handler)
            .into_any()
    } else {
        base.into_any()
    }
}

/// A stash leaf — left-click **pops** (apply + remove); right-click opens a
/// menu (Apply / Drop). User request: clicking a stash should consume it.
fn build_stash_row(index: usize, message: &str, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    // issue #414: a stash message embeds a commit subject, which is remote-origin
    // after a pull. Neutralize control bytes before display.
    let message = kagi_domain::text_safety::sanitize_control_bytes(message);
    let raw_label = format!("stash@{{{}}}: {}", index, message);
    let full_name = SharedString::from(raw_label.clone());
    // #236: click PEEKS (read-only), same as the graph's stash row; Pop lives
    // in the context menu only.
    let click_handler = cx.listener(
        move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
            this.open_stash_peek(index, cx);
            cx.notify();
        },
    );
    let msg_for_menu = message.to_string();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_stash_menu(index, msg_for_menu.clone(), event.position);
            cx.stop_propagation();
            cx.notify();
        },
    );
    div()
        .id(("sidebar-stash", index))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(theme().color_warning))
        .overflow_hidden()
        .on_click(click_handler)
        .on_mouse_down(gpui::MouseButton::Right, menu_handler)
        .hover(|style| style.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(full_name))
        .child(
            div()
                .flex_1()
                .truncate()
                .child(SharedString::from(raw_label)),
        )
        .into_any()
}

// ──────────────────────────────────────────────────────────────
// W13-BRANCHTREE: unit tests for the pure grouping helpers
// ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Compact view of a GroupRow for assertions: ("G", prefix, count) for a
    /// group header, ("L", leaf_label, item) for a grouped leaf, and
    /// ("T", item, item) for a top-level item.
    fn summarize(rows: &[GroupRow<String>]) -> Vec<(&'static str, String, String)> {
        rows.iter()
            .map(|r| match r {
                GroupRow::Group { prefix, count } => ("G", prefix.clone(), count.to_string()),
                GroupRow::GroupedLeaf {
                    leaf_label, item, ..
                } => ("L", leaf_label.clone(), item.clone()),
                GroupRow::TopLevel { item } => ("T", item.clone(), item.clone()),
            })
            .collect()
    }

    fn group(names: &[&str]) -> Vec<GroupRow<String>> {
        let owned: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        group_by_prefix(&owned, |s| s.as_str())
    }

    #[test]
    fn split_basic() {
        assert_eq!(
            split_first_segment("feat/a"),
            Some(("feat".into(), "a".into()))
        );
        assert_eq!(
            split_first_segment("feat/ui/x"),
            Some(("feat".into(), "ui/x".into()))
        );
        assert_eq!(split_first_segment("main"), None);
        // Empty halves stay top-level.
        assert_eq!(split_first_segment("/x"), None);
        assert_eq!(split_first_segment("feat/"), None);
    }

    #[test]
    fn split_non_ascii() {
        // chars()-based: multibyte prefixes must not panic or mis-split.
        assert_eq!(
            split_first_segment("機能/あ"),
            Some(("機能".into(), "あ".into()))
        );
    }

    #[test]
    fn groups_and_top_level() {
        // feat/a, feat/b → group feat(2); fix/c → group fix(1); main → top.
        let rows = group(&["feat/a", "feat/b", "fix/c", "main"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "feat".into(), "2".into()),
                ("L", "a".into(), "feat/a".into()),
                ("L", "b".into(), "feat/b".into()),
                ("G", "fix".into(), "1".into()),
                ("L", "c".into(), "fix/c".into()),
                ("T", "main".into(), "main".into()),
            ]
        );
    }

    #[test]
    fn multi_segment_leaf_keeps_remainder() {
        // Single first-level split: feat/ui/x → group feat, leaf "ui/x".
        let rows = group(&["feat/ui/x"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "feat".into(), "1".into()),
                ("L", "ui/x".into(), "feat/ui/x".into()),
            ]
        );
    }

    #[test]
    fn remote_grouped_by_remote_name() {
        // origin/feat/x → group origin, leaf "feat/x".
        let rows = group(&["origin/main", "origin/feat/x", "upstream/dev"]);
        assert_eq!(
            summarize(&rows),
            vec![
                ("G", "origin".into(), "2".into()),
                ("L", "main".into(), "origin/main".into()),
                ("L", "feat/x".into(), "origin/feat/x".into()),
                ("G", "upstream".into(), "1".into()),
                ("L", "dev".into(), "upstream/dev".into()),
            ]
        );
    }

    #[test]
    fn no_groups_all_top_level() {
        let rows = group(&["main", "dev", "trunk"]);
        assert!(rows.iter().all(|r| matches!(r, GroupRow::TopLevel { .. })));
        assert_eq!(rows.len(), 3);
    }

    // ── W19-REMOTE-TREE: two-level remote grouping ──────────────────

    /// Compact view of a RemoteRow: ("R", remote, count), ("S", prefix, count),
    /// ("RL", remote, leaf), ("SL", prefix, leaf).
    fn summarize_remote(
        rows: &[RemoteRow<(String, String)>],
    ) -> Vec<(&'static str, String, String)> {
        rows.iter()
            .map(|r| match r {
                RemoteRow::Remote { remote, count } => ("R", remote.clone(), count.to_string()),
                RemoteRow::SubGroup { prefix, count, .. } => {
                    ("S", prefix.clone(), count.to_string())
                }
                RemoteRow::RemoteLeaf { leaf_label, .. } => {
                    ("RL", leaf_label.clone(), String::new())
                }
                RemoteRow::SubGroupedLeaf {
                    prefix, leaf_label, ..
                } => ("SL", prefix.clone(), leaf_label.clone()),
            })
            .collect()
    }

    /// Build remote rows from (remote, name) pairs.
    fn group_rem(pairs: &[(&str, &str)]) -> Vec<RemoteRow<(String, String)>> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(r, n)| (r.to_string(), n.to_string()))
            .collect();
        group_remotes(&owned, |(r, _)| r.as_str(), |(_, n)| n.as_str())
    }

    #[test]
    fn remote_two_levels_basic() {
        // origin/main → origin ▸ main (direct leaf)
        // origin/feat/x → origin ▸ feat ▸ x (sub-grouped leaf)
        let rows = group_rem(&[("origin", "main"), ("origin", "feat/x")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "2".into()),
                ("RL", "main".into(), String::new()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "x".into()),
            ]
        );
    }

    #[test]
    fn remote_multiple_remotes_independent() {
        // origin and upstream group independently, in first-seen order.
        let rows = group_rem(&[
            ("origin", "feat/a"),
            ("origin", "feat/b"),
            ("upstream", "feat/c"),
            ("upstream", "dev"),
        ]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "2".into()),
                ("S", "feat".into(), "2".into()),
                ("SL", "feat".into(), "a".into()),
                ("SL", "feat".into(), "b".into()),
                ("R", "upstream".into(), "2".into()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "c".into()),
                ("RL", "dev".into(), String::new()),
            ]
        );
    }

    #[test]
    fn remote_deep_name_keeps_remainder() {
        // origin/feat/ui/x → origin ▸ feat ▸ ui/x (single sub-level split).
        let rows = group_rem(&[("origin", "feat/ui/x")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "1".into()),
                ("S", "feat".into(), "1".into()),
                ("SL", "feat".into(), "ui/x".into()),
            ]
        );
    }

    #[test]
    fn remote_collapse_keys_unique_and_no_collision() {
        // Level-1 remote header vs level-2 sub-group vs local must all differ.
        assert_eq!(remote_key("origin"), "remote:origin");
        assert_eq!(remote_group_key("origin", "feat"), "remote:origin:feat");
        assert_eq!(remote_group_key("upstream", "feat"), "remote:upstream:feat");
        // Two remotes with the same sub-group prefix get distinct keys.
        assert_ne!(
            remote_group_key("origin", "feat"),
            remote_group_key("upstream", "feat")
        );
        // The remote header key (2 segments) never equals any sub-group key
        // (3 segments), and never matches a local key.
        assert_ne!(remote_key("origin"), remote_group_key("origin", "feat"));
        assert_ne!(
            remote_group_key("origin", "feat"),
            group_key(SECTION_LOCAL, "feat")
        );
    }

    #[test]
    fn remote_non_ascii_subgroup() {
        let rows = group_rem(&[("origin", "機能/あ")]);
        assert_eq!(
            summarize_remote(&rows),
            vec![
                ("R", "origin".into(), "1".into()),
                ("S", "機能".into(), "1".into()),
                ("SL", "機能".into(), "あ".into()),
            ]
        );
    }

    /// Rebuild `rows` the way `render` does (rows and fingerprint together),
    /// then let the list processor's cache catch up.
    fn rebuild(state: &mut SidebarState, fingerprint: u64, groups: &HashSet<String>, filter: &str) {
        let branches = [
            ("main".to_string(), true),
            ("feat/a".to_string(), false),
            ("feat/b".to_string(), false),
        ];
        let rows = build_sidebar_rows(
            &branches,
            &[],
            None,
            &[],
            &[],
            &[],
            &[],
            Default::default(),
            &state.collapsed,
            groups,
            filter,
        );
        state.rows = rows;
        state.rows_fingerprint = fingerprint;
        state.refresh_tree_positions();
    }

    fn branch_position(state: &SidebarState, branch: &str) -> Option<(usize, usize)> {
        let i = state.rows.iter().position(
            |r| matches!(r, SidebarRow::LocalBranchLeaf { name, .. } if name == branch),
        )?;
        Some(state.tree_positions[i])
    }

    #[test]
    fn tree_positions_follow_each_rows_rebuild() {
        let mut state = SidebarState::new();
        let no_groups = HashSet::new();
        rebuild(&mut state, 1, &no_groups, "");
        assert_eq!(branch_position(&state, "feat/b"), Some((2, 2)));

        // Filter edit: only `feat/b` is left under its parent.
        rebuild(&mut state, 2, &no_groups, "b");
        assert_eq!(state.tree_positions.len(), state.rows.len());
        assert_eq!(branch_position(&state, "feat/b"), Some((1, 1)));

        // Group collapse: its leaves leave the list, the cache follows.
        let collapsed: HashSet<String> = [group_key(SECTION_LOCAL, "feat")].into();
        rebuild(&mut state, 3, &collapsed, "");
        assert_eq!(state.tree_positions.len(), state.rows.len());
        assert_eq!(branch_position(&state, "feat/b"), None);

        // Section collapse leaves only the header.
        state.collapsed.insert(SECTION_LOCAL);
        rebuild(&mut state, 4, &no_groups, "");
        assert_eq!(state.tree_positions.len(), state.rows.len());
        assert_eq!(branch_position(&state, "main"), None);

        // Expanding again restores the positions.
        state.collapsed.remove(SECTION_LOCAL);
        rebuild(&mut state, 5, &no_groups, "");
        assert_eq!(branch_position(&state, "feat/b"), Some((2, 2)));
    }
}

// ──────────────────────────────────────────────────────────────
// PULL REQUESTS row (GitHub Phase 1)
// ──────────────────────────────────────────────────────────────

/// `#N title` with a CI glyph and review/draft cues. Click jumps the graph to
/// the head branch; right-click opens the PR menu (open on GitHub / copy URL).
fn build_pr_row(
    pr: &kagi_domain::github::PullRequest,
    stacked: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use kagi_domain::github::{CiState, ReviewState};
    let (ci_glyph, ci_color) = match pr.ci {
        CiState::Success => ("\u{2713}", theme().color_success),
        CiState::Failure => ("\u{2717}", theme().color_blocker),
        CiState::Pending => ("\u{25CF}", theme().color_warning),
        CiState::None => ("\u{25CB}", theme().text_muted),
    };
    let review_glyph = match pr.review {
        ReviewState::Approved => Some(("\u{2714}", theme().color_success)),
        ReviewState::ChangesRequested => Some(("\u{21BA}", theme().color_warning)),
        ReviewState::ReviewRequired | ReviewState::None => None,
    };
    // #356: the title is GitHub-origin; neutralize control bytes like every
    // other PR title surface (and the row's accessible name).
    let label = format!(
        "#{} {}",
        pr.number,
        kagi_domain::text_safety::sanitize_control_bytes(&pr.title)
    );
    let mut tip = format!(
        "#{} {} \u{2190} {}\n@{}",
        pr.number, pr.head, pr.base, pr.author
    );
    if pr.is_draft {
        tip.push_str(&format!("\n{}", Msg::PrDraft.t()));
    }
    if stacked {
        tip.push_str(&format!("\n{} {}", Msg::PrStacked.t(), pr.base));
    }
    let pr_click = pr.clone();
    let click_handler = cx.listener(
        move |this: &mut KagiApp, _e: &gpui::ClickEvent, _window, cx| {
            this.jump_to_pr_head(&pr_click, cx);
            cx.notify();
        },
    );
    let pr_menu = pr.clone();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _window, cx| {
            this.with_ui(|ui| ui.pr_menu = Some((pr_menu.clone(), e.position)));
            cx.stop_propagation();
            cx.notify();
        },
    );
    div()
        .id(("sidebar-pr", pr.number as usize))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .text_sm()
        .overflow_hidden()
        .on_click(click_handler)
        .on_mouse_down(gpui::MouseButton::Right, menu_handler)
        .hover(|style| style.bg(rgb(theme().surface)))
        .tooltip(name_tooltip(SharedString::from(tip)))
        // Drafts read dimmed; stacked PRs get a small indent so the chain is
        // visible at a glance (Graphite-style, without the full tree yet).
        .when(pr.is_draft, |el| el.opacity(0.55))
        .when(stacked, |el| el.pl(theme::scaled_px(24.)))
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(12.))
                .text_color(rgb(ci_color))
                .child(SharedString::from(ci_glyph)),
        )
        .child(
            div()
                .flex_1()
                .truncate()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(label)),
        )
        .children(review_glyph.map(|(g, c)| {
            div()
                .flex_shrink_0()
                .text_color(rgb(c))
                .child(SharedString::from(g))
        }))
        .into_any()
}
