//! Sidebar renderer — W2-SIDEBAR: Repository Navigator
//!
//! Extracted from mod.rs (T013) and extended to a full 4-section navigator:
//! LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES
//!
//! Public surface:
//! - `render_sidebar(...)` — called from `render_body` in mod.rs

use std::collections::HashSet;

use gpui::{
    Context, Entity, SharedString,
    div, prelude::*, px, rgb,
};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;

use kagi::git::{CommitId, RemoteBranch, Stash, Tag, UpstreamInfo, Worktree};

use super::KagiApp;
use super::theme::theme;

// W9-THEME: all colours come from `theme()` (see theme.rs).

// ──────────────────────────────────────────────────────────────
// Section keys (static strings used in sidebar_collapsed)
// ──────────────────────────────────────────────────────────────

pub const SECTION_LOCAL: &str = "local";
pub const SECTION_REMOTE: &str = "remote";
pub const SECTION_TAGS: &str = "tags";
pub const SECTION_WORKTREES: &str = "worktrees";
pub const SECTION_STASHES: &str = "stashes";

// ──────────────────────────────────────────────────────────────
// W13-BRANCHTREE: `/`-prefix grouping of branch names
// ──────────────────────────────────────────────────────────────

/// One entry in a grouped branch listing.
///
/// Grouping is a **single first-level** split on `/` (the ticket explicitly
/// allows stopping after one level — `feat/ui/x` becomes group `feat` + leaf
/// `ui/x`, not a multi-level tree). This keeps the UI shallow and the click
/// model simple while still giving the user collapsible `feat` / `fix` groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupRow<T> {
    /// A collapsible group header for a `/`-prefix, with its child count.
    Group {
        /// The prefix before the first `/`, e.g. `"feat"`.
        prefix: String,
        /// Number of leaves under this group.
        count: usize,
    },
    /// A branch leaf that belongs to the group started by the most recent
    /// preceding [`GroupRow::Group`], displayed with the prefix stripped.
    GroupedLeaf {
        /// The owning group's prefix (for building the collapse key).
        prefix: String,
        /// The remainder of the name after the first `/` (e.g. `"a"` or
        /// `"ui/x"`). This is what the row shows; the original item carries
        /// the full name for click/tooltip behaviour.
        leaf_label: String,
        /// The original item (full branch info), preserved verbatim.
        item: T,
    },
    /// A name with no `/` — rendered at the top level exactly as before.
    TopLevel {
        /// The original item, preserved verbatim.
        item: T,
    },
}

/// Group a list of branch items by the first `/` segment of their name.
///
/// Pure function (no UI/gpui types) so it can be unit-tested. Order is
/// preserved from the input: groups appear in first-seen order, leaves within
/// a group in input order, top-level names interleaved at the position of
/// their group's first member (groups) or their own position (top-level).
///
/// `name_of` extracts the grouping name from each item (chars-based split, no
/// byte indexing). Items whose name has no `/` (or an empty prefix, e.g. a
/// leading `/`) become [`GroupRow::TopLevel`].
fn group_by_prefix<T: Clone>(items: &[T], name_of: impl Fn(&T) -> &str) -> Vec<GroupRow<T>> {
    // First pass: collect group order + counts (first-seen order).
    let mut group_order: Vec<String> = Vec::new();
    let mut group_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for it in items {
        if let Some((prefix, _rest)) = split_first_segment(name_of(it)) {
            if !group_count.contains_key(&prefix) {
                group_order.push(prefix.clone());
            }
            *group_count.entry(prefix).or_insert(0) += 1;
        }
    }

    // Second pass: emit rows. A group header is emitted just before the first
    // leaf that belongs to it; subsequent leaves of the same group follow.
    let mut out: Vec<GroupRow<T>> = Vec::new();
    let mut emitted_header: std::collections::HashSet<String> = std::collections::HashSet::new();
    for it in items {
        match split_first_segment(name_of(it)) {
            Some((prefix, rest)) => {
                if emitted_header.insert(prefix.clone()) {
                    out.push(GroupRow::Group {
                        prefix: prefix.clone(),
                        count: *group_count.get(&prefix).unwrap_or(&0),
                    });
                }
                out.push(GroupRow::GroupedLeaf {
                    prefix,
                    leaf_label: rest,
                    item: it.clone(),
                });
            }
            None => out.push(GroupRow::TopLevel { item: it.clone() }),
        }
    }
    out
}

/// Split a name on its first `/`, returning `(prefix, rest)` where both parts
/// are non-empty. Returns `None` when there is no `/`, or when either side
/// would be empty (e.g. `"/x"` or `"feat/"`), so such names stay top-level.
///
/// chars()-based (no byte slicing) per the project's non-ASCII safety rule.
fn split_first_segment(name: &str) -> Option<(String, String)> {
    let mut prefix = String::new();
    let mut rest = String::new();
    let mut seen_slash = false;
    for ch in name.chars() {
        if !seen_slash && ch == '/' {
            seen_slash = true;
            continue;
        }
        if seen_slash {
            rest.push(ch);
        } else {
            prefix.push(ch);
        }
    }
    if seen_slash && !prefix.is_empty() && !rest.is_empty() {
        Some((prefix, rest))
    } else {
        None
    }
}

/// Build the dynamic collapse key for a group (e.g. `"local:feat"`).
fn group_key(section: &str, prefix: &str) -> String {
    format!("{section}:{prefix}")
}

/// Build a `.tooltip(...)` closure showing the full (untruncated) name.
/// Row labels are single-line + ellipsized, so the tooltip is how the user
/// reads a name that doesn't fit the sidebar width.
fn name_tooltip(
    full: SharedString,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |window, cx| Tooltip::new(full.clone()).build(window, cx)
}

// ──────────────────────────────────────────────────────────────
// render_sidebar — main entry point
// ──────────────────────────────────────────────────────────────

/// Render the left sidebar as a 4-section Repository Navigator.
///
/// Sections: LOCAL BRANCHES / REMOTE BRANCHES / TAGS / STASHES
/// - Each section header shows item count and a collapse toggle (▸/▾).
/// - A filter input at the top filters all sections by partial name match.
/// - Existing click/jump/dblclick behaviour is preserved exactly.
///
/// New state fields required on `KagiApp` (added in mod.rs):
/// - `sidebar_collapsed: HashSet<&'static str>`
/// - `sidebar_filter: Option<Entity<InputState>>`
/// - `remote_branches: Vec<RemoteBranch>`
/// - `tags: Vec<Tag>`
/// - `branch_upstream_info: std::collections::HashMap<String, UpstreamInfo>`
#[allow(clippy::too_many_arguments)]
pub fn render_sidebar(
    branches: &[(String, bool)],
    remote_branches: &[RemoteBranch],
    tags: &[Tag],
    stashes: &[Stash],
    worktrees: &[Worktree],
    branch_upstream_info: &std::collections::HashMap<String, UpstreamInfo>,
    commit_row_index: &std::collections::HashMap<CommitId, usize>,
    collapsed: &HashSet<&'static str>,
    groups_collapsed: &HashSet<String>,
    filter_input: Option<Entity<InputState>>,
    width: f32,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // ── Derive filter text from InputState (if present) ──────────
    let filter_text: String = if let Some(ref ent) = filter_input {
        ent.read(cx).value().to_lowercase()
    } else {
        String::new()
    };
    let has_filter = !filter_text.is_empty();

    // ── Filter helper ─────────────────────────────────────────────
    let matches = |name: &str| -> bool {
        if has_filter {
            name.to_lowercase().contains(&filter_text)
        } else {
            true
        }
    };

    // ── Count filtered items per section ─────────────────────────
    let local_filtered: Vec<&(String, bool)> = branches
        .iter()
        .filter(|(n, _)| matches(n))
        .collect();
    let remote_filtered: Vec<&RemoteBranch> = remote_branches
        .iter()
        .filter(|rb| matches(&rb.name) || matches(&format!("{}/{}", rb.remote, rb.name)))
        .collect();
    let tags_filtered: Vec<&Tag> = tags
        .iter()
        .filter(|t| matches(&t.name))
        .collect();
    let stashes_filtered: Vec<&Stash> = stashes
        .iter()
        .filter(|s| matches(&s.message))
        .collect();
    let worktrees_filtered: Vec<&Worktree> = worktrees
        .iter()
        .filter(|w| matches(&w.name) || matches(w.path.to_string_lossy().as_ref()))
        .collect();

    // ── Scrollable inner column ───────────────────────────────────
    let mut col = div()
        .id("sidebar-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .py_1();

    // ── Filter input row ─────────────────────────────────────────
    {
        let filter_area: gpui::AnyElement = if let Some(ref input_entity) = filter_input {
            div()
                .px_2()
                .py_1()
                .flex_shrink_0()
                .child(
                    Input::new(input_entity)
                        .xsmall()
                        .appearance(true),
                )
                .into_any_element()
        } else {
            // Placeholder: clicking creates the InputState (requires Window).
            let create_handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, window, cx| {
                this.ensure_sidebar_filter(window, cx);
                cx.notify();
            });
            div()
                .id("sidebar-filter-placeholder")
                .px_2()
                .py_1()
                .flex_shrink_0()
                .on_click(create_handler)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(
                    div()
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .px_2()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .bg(rgb(theme().bg_base))
                        .rounded(px(4.))
                        .child(SharedString::from("filter…")),
                )
                .into_any_element()
        };
        col = col.child(filter_area);
    }

    // ── LOCAL BRANCHES section ────────────────────────────────────
    {
        let local_collapsed = collapsed.contains(SECTION_LOCAL);
        let local_count = branches.len();
        let header_label = SharedString::from(format!(
            "{} LOCAL BRANCHES ({})",
            if local_collapsed { "▸" } else { "▾" },
            local_count
        ));
        let toggle_local = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar_collapsed.contains(SECTION_LOCAL) {
                this.sidebar_collapsed.remove(SECTION_LOCAL);
            } else {
                this.sidebar_collapsed.insert(SECTION_LOCAL);
            }
            cx.notify();
        });
        col = col.child(
            div()
                .id("sidebar-section-local")
                .px_3()
                .py_1()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .on_click(toggle_local)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(header_label),
        );

        if !local_collapsed {
            // W13-BRANCHTREE: build a single leaf-row builder so grouped
            // leaves and top-level leaves share *exactly* the same behaviour
            // (click=jump / dblclick=checkout / ✕ delete / ✓ current /
            // ↑↓ upstream / truncate+tooltip). `display_label` is the visible
            // text (prefix-stripped for grouped leaves); all click handlers,
            // the row id and the tooltip use the full `branch_name`.
            let local_leaf_row = |branch_name: &str,
                                      is_head: bool,
                                      display_label: &str,
                                      indented: bool,
                                      cx: &mut Context<KagiApp>|
             -> gpui::AnyElement {
                let upstream_label: Option<SharedString> = if let Some(u) = branch_upstream_info.get(branch_name) {
                    if u.ahead > 0 || u.behind > 0 {
                        Some(SharedString::from(format!("\u{2191}{} \u{2193}{}", u.ahead, u.behind)))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let label = if is_head {
                    SharedString::from(format!("\u{2713} {}", display_label))
                } else {
                    SharedString::from(display_label.to_string())
                };
                let text_color = if is_head { theme().color_success } else { theme().text_main };
                let branch_for_click = branch_name.to_string();
                let full_name = SharedString::from(branch_name.to_string());
                // Grouped leaves get extra left padding to read as a child.
                let left_pad = if indented { px(28.) } else { px(12.) };

                if is_head {
                    let head_click = cx.listener(move |this: &mut KagiApp, _e: &gpui::ClickEvent, _w, cx| {
                        this.jump_to_branch(&branch_for_click);
                        cx.notify();
                    });
                    div()
                        .id(SharedString::from(format!("sidebar-branch-{}", branch_name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .pl(left_pad)
                        .pr_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .on_click(head_click)
                        .hover(|style| style.bg(rgb(theme().surface)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label))
                        .when_some(upstream_label, |el, ul| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .ml_2()
                                    .text_xs()
                                    .text_color(rgb(theme().text_sub))
                                    .child(ul),
                            )
                        })
                        .into_any()
                } else {
                    let branch_for_dbl = branch_name.to_string();
                    let branch_for_delete = branch_name.to_string();
                    let click_handler = cx.listener(move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.open_plan_modal(branch_for_dbl.clone());
                        } else {
                            this.jump_to_branch(&branch_for_dbl);
                        }
                        cx.notify();
                    });
                    let delete_handler = cx.listener(move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                        this.open_delete_branch_modal(branch_for_delete.clone());
                        cx.notify();
                    });
                    div()
                        .id(SharedString::from(format!("sidebar-branch-{}", branch_name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .pl(left_pad)
                        .pr_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(theme().surface)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label))
                        .when_some(upstream_label, |el, ul| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .ml_2()
                                    .text_xs()
                                    .text_color(rgb(theme().text_sub))
                                    .child(ul),
                            )
                        })
                        // ✕ delete button: always visible (small, muted) for non-HEAD branches.
                        .child(
                            div()
                                .id(SharedString::from(format!("sidebar-delete-{}", branch_name)))
                                .flex_shrink_0()
                                .ml_1()
                                .px_1()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .on_click(delete_handler)
                                .hover(|s| s.text_color(rgb(theme().color_blocker)))
                                .child(SharedString::from("\u{00d7}")),
                        )
                        .into_any()
                }
            };

            // Group the (filtered) local branches by `/`-prefix.
            let local_owned: Vec<(String, bool)> =
                local_filtered.iter().map(|(n, h)| (n.clone(), *h)).collect();
            let grouped = group_by_prefix(&local_owned, |(n, _)| n.as_str());

            for row in &grouped {
                match row {
                    GroupRow::Group { prefix, count } => {
                        let key = group_key(SECTION_LOCAL, prefix);
                        // Filter active ⇒ auto-expand so matching leaves show.
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        let arrow = if group_collapsed { "\u{25b8}" } else { "\u{25be}" };
                        let glabel = SharedString::from(format!("{} {} ({})", arrow, prefix, count));
                        let key_for_toggle = key.clone();
                        let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                            if this.branch_groups_collapsed.contains(&key_for_toggle) {
                                this.branch_groups_collapsed.remove(&key_for_toggle);
                            } else {
                                this.branch_groups_collapsed.insert(key_for_toggle.clone());
                            }
                            cx.notify();
                        });
                        col = col.child(
                            div()
                                .id(SharedString::from(format!("sidebar-group-{}", key)))
                                .flex()
                                .flex_row()
                                .items_center()
                                .flex_shrink_0()
                                .pl(px(20.))
                                .pr_3()
                                .py_1()
                                .text_sm()
                                .text_color(rgb(theme().text_sub))
                                .overflow_hidden()
                                .on_click(toggle)
                                .hover(|s| s.bg(rgb(theme().surface)))
                                .child(div().flex_1().truncate().child(glabel)),
                        );
                    }
                    GroupRow::GroupedLeaf { prefix, leaf_label, item } => {
                        let key = group_key(SECTION_LOCAL, prefix);
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        if !group_collapsed {
                            let (name, is_head) = item;
                            col = col.child(local_leaf_row(name, *is_head, leaf_label, true, cx));
                        }
                    }
                    GroupRow::TopLevel { item } => {
                        let (name, is_head) = item;
                        col = col.child(local_leaf_row(name, *is_head, name, false, cx));
                    }
                }
            }
        }
    }

    // ── REMOTE BRANCHES section ───────────────────────────────────
    {
        let remote_collapsed = collapsed.contains(SECTION_REMOTE);
        let remote_count = remote_branches.len();
        let header_label = SharedString::from(format!(
            "{} REMOTE BRANCHES ({})",
            if remote_collapsed { "▸" } else { "▾" },
            remote_count
        ));
        let toggle_remote = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar_collapsed.contains(SECTION_REMOTE) {
                this.sidebar_collapsed.remove(SECTION_REMOTE);
            } else {
                this.sidebar_collapsed.insert(SECTION_REMOTE);
            }
            cx.notify();
        });
        col = col.child(
            div()
                .id("sidebar-section-remote")
                .px_3()
                .pt_2()
                .pb_1()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .on_click(toggle_remote)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(header_label),
        );

        if !remote_collapsed {
            // W13-BRANCHTREE: remote rows are grouped by their first `/`
            // segment — which is the *remote name* (`origin/feat/x` → group
            // `origin`, leaf `feat/x`). Single-level grouping by remote name,
            // per the ticket. Jump/tooltip/id all use the full display name.
            let remote_leaf_row = |display: &str,
                                   display_label: &str,
                                   rb_target: CommitId,
                                   indented: bool,
                                   cx: &mut Context<KagiApp>|
             -> gpui::AnyElement {
                let can_jump = commit_row_index.contains_key(&rb_target);
                let full_name = SharedString::from(display.to_string());
                let label = SharedString::from(display_label.to_string());
                let left_pad = if indented { px(28.) } else { px(12.) };
                if can_jump {
                    let click_handler = cx.listener(move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                        this.jump_to_commit(&rb_target);
                        cx.notify();
                    });
                    div()
                        .id(SharedString::from(format!("sidebar-remote-{}", display)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .pl(left_pad)
                        .pr_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(theme().color_remote))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(theme().surface)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label))
                        .into_any()
                } else {
                    div()
                        .id(SharedString::from(format!("sidebar-remote-{}", display)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .pl(left_pad)
                        .pr_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(theme().color_remote))
                        .overflow_hidden()
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label))
                        .into_any()
                }
            };

            // Build (display, target) tuples then group by `/`-prefix.
            let remote_owned: Vec<(String, CommitId)> = remote_filtered
                .iter()
                .map(|rb| (format!("{}/{}", rb.remote, rb.name), rb.target.clone()))
                .collect();
            let grouped = group_by_prefix(&remote_owned, |(d, _)| d.as_str());

            for row in &grouped {
                match row {
                    GroupRow::Group { prefix, count } => {
                        let key = group_key(SECTION_REMOTE, prefix);
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        let arrow = if group_collapsed { "\u{25b8}" } else { "\u{25be}" };
                        let glabel = SharedString::from(format!("{} {} ({})", arrow, prefix, count));
                        let key_for_toggle = key.clone();
                        let toggle = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                            if this.branch_groups_collapsed.contains(&key_for_toggle) {
                                this.branch_groups_collapsed.remove(&key_for_toggle);
                            } else {
                                this.branch_groups_collapsed.insert(key_for_toggle.clone());
                            }
                            cx.notify();
                        });
                        col = col.child(
                            div()
                                .id(SharedString::from(format!("sidebar-group-{}", key)))
                                .flex()
                                .flex_row()
                                .items_center()
                                .flex_shrink_0()
                                .pl(px(20.))
                                .pr_3()
                                .py_1()
                                .text_sm()
                                .text_color(rgb(theme().text_sub))
                                .overflow_hidden()
                                .on_click(toggle)
                                .hover(|s| s.bg(rgb(theme().surface)))
                                .child(div().flex_1().truncate().child(glabel)),
                        );
                    }
                    GroupRow::GroupedLeaf { prefix, leaf_label, item } => {
                        let key = group_key(SECTION_REMOTE, prefix);
                        let group_collapsed = !has_filter && groups_collapsed.contains(&key);
                        if !group_collapsed {
                            let (display, target) = item;
                            col = col.child(remote_leaf_row(display, leaf_label, target.clone(), true, cx));
                        }
                    }
                    GroupRow::TopLevel { item } => {
                        let (display, target) = item;
                        col = col.child(remote_leaf_row(display, display, target.clone(), false, cx));
                    }
                }
            }
        }
    }

    // ── TAGS section ──────────────────────────────────────────────
    {
        let tags_collapsed = collapsed.contains(SECTION_TAGS);
        let tags_count = tags.len();
        let header_label = SharedString::from(format!(
            "{} TAGS ({})",
            if tags_collapsed { "▸" } else { "▾" },
            tags_count
        ));
        let toggle_tags = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar_collapsed.contains(SECTION_TAGS) {
                this.sidebar_collapsed.remove(SECTION_TAGS);
            } else {
                this.sidebar_collapsed.insert(SECTION_TAGS);
            }
            cx.notify();
        });
        col = col.child(
            div()
                .id("sidebar-section-tags")
                .px_3()
                .pt_2()
                .pb_1()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .on_click(toggle_tags)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(header_label),
        );

        if !tags_collapsed {
            for tag in &tags_filtered {
                let tag_target = tag.target.clone();
                let tag_label = SharedString::from(tag.name.clone());
                let can_jump = commit_row_index.contains_key(&tag_target);

                let full_name = SharedString::from(tag.name.clone());
                let row: gpui::AnyElement = if can_jump {
                    let click_handler = cx.listener(move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                        this.jump_to_commit(&tag_target);
                        cx.notify();
                    });
                    div()
                        .id(SharedString::from(format!("sidebar-tag-{}", tag.name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(theme().color_tag))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(theme().surface)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(tag_label))
                        .into_any()
                } else {
                    div()
                        .id(SharedString::from(format!("sidebar-tag-{}", tag.name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(theme().color_tag))
                        .overflow_hidden()
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(tag_label))
                        .into_any()
                };

                col = col.child(row);
            }
        }
    }

    // ── WORKTREES section ───────────────────────────────────────
    {
        let worktrees_collapsed = collapsed.contains(SECTION_WORKTREES);
        let worktrees_count = worktrees.len();
        let header_label = SharedString::from(format!(
            "{} WORKTREES ({})",
            if worktrees_collapsed { "▸" } else { "▾" },
            worktrees_count
        ));
        let toggle_worktrees = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar_collapsed.contains(SECTION_WORKTREES) {
                this.sidebar_collapsed.remove(SECTION_WORKTREES);
            } else {
                this.sidebar_collapsed.insert(SECTION_WORKTREES);
            }
            cx.notify();
        });
        col = col.child(
            div()
                .id("sidebar-section-worktrees")
                .px_3()
                .pt_2()
                .pb_1()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .on_click(toggle_worktrees)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(header_label),
        );

        if !worktrees_collapsed {
            for wt in &worktrees_filtered {
                let path_label = wt.path.display().to_string();
                let label = if wt.is_current {
                    SharedString::from(format!("\u{2713} {}  {}", wt.name, path_label))
                } else {
                    SharedString::from(format!("{}  {}", wt.name, path_label))
                };
                let full_name = label.clone();
                let text_color = if wt.is_current { theme().color_success } else { theme().text_sub };
                col = col.child(
                    div()
                        .id(SharedString::from(format!("sidebar-worktree-{}", wt.name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label)),
                );
            }
        }
    }

    // ── STASHES section ───────────────────────────────────────────
    {
        let stashes_collapsed = collapsed.contains(SECTION_STASHES);
        let stashes_count = stashes.len();
        let header_label = SharedString::from(format!(
            "{} STASHES ({})",
            if stashes_collapsed { "▸" } else { "▾" },
            stashes_count
        ));
        let toggle_stashes = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            if this.sidebar_collapsed.contains(SECTION_STASHES) {
                this.sidebar_collapsed.remove(SECTION_STASHES);
            } else {
                this.sidebar_collapsed.insert(SECTION_STASHES);
            }
            cx.notify();
        });
        col = col.child(
            div()
                .id("sidebar-section-stashes")
                .px_3()
                .pt_2()
                .pb_1()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .on_click(toggle_stashes)
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(header_label),
        );

        if !stashes_collapsed {
            for stash in &stashes_filtered {
                let idx = stash.index;
                let raw_label = format!("stash@{{{}}}: {}", idx, stash.message);
                let full_name = SharedString::from(raw_label.clone());

                let click_handler = cx.listener(move |this: &mut KagiApp, _event: &gpui::ClickEvent, _window, cx| {
                    this.open_stash_apply_modal(idx);
                    cx.notify();
                });

                col = col.child(
                    div()
                        .id(("sidebar-stash", idx))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(theme().color_warning))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(theme().surface)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(SharedString::from(raw_label))),
                );
            }
        }
    }

    // ── Fixed-width outer shell ───────────────────────────────────
    div()
        .w(px(width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .child(col)
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
                GroupRow::GroupedLeaf { leaf_label, item, .. } => ("L", leaf_label.clone(), item.clone()),
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
        assert_eq!(split_first_segment("feat/a"), Some(("feat".into(), "a".into())));
        assert_eq!(split_first_segment("feat/ui/x"), Some(("feat".into(), "ui/x".into())));
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
    fn group_key_format() {
        assert_eq!(group_key(SECTION_LOCAL, "feat"), "local:feat");
        assert_eq!(group_key(SECTION_REMOTE, "origin"), "remote:origin");
    }

    #[test]
    fn no_groups_all_top_level() {
        let rows = group(&["main", "dev", "trunk"]);
        assert!(rows.iter().all(|r| matches!(r, GroupRow::TopLevel { .. })));
        assert_eq!(rows.len(), 3);
    }
}
