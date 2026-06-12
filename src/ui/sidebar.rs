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

// ──────────────────────────────────────────────────────────────
// Colour palette (re-exported from mod.rs; keep in sync)
// ──────────────────────────────────────────────────────────────

const BG_SIDEBAR: u32 = 0x11111b;
const BG_SURFACE: u32 = 0x313244;
const TEXT_MAIN: u32 = 0xcdd6f4;
const TEXT_MUTED: u32 = 0x585b70;
const TEXT_SUB: u32 = 0xa6adc8;
const COLOR_SUCCESS: u32 = 0xa6e3a1;
const COLOR_WARNING: u32 = 0xf9e2af;
const COLOR_REMOTE: u32 = 0xa6e3a1;
const COLOR_TAG: u32 = 0xfab387;

// ──────────────────────────────────────────────────────────────
// Section keys (static strings used in sidebar_collapsed)
// ──────────────────────────────────────────────────────────────

pub const SECTION_LOCAL: &str = "local";
pub const SECTION_REMOTE: &str = "remote";
pub const SECTION_TAGS: &str = "tags";
pub const SECTION_WORKTREES: &str = "worktrees";
pub const SECTION_STASHES: &str = "stashes";

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
                .hover(|s| s.bg(rgb(BG_SURFACE)))
                .child(
                    div()
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .px_2()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .bg(rgb(0x1e1e2e))
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
                .text_color(rgb(TEXT_MUTED))
                .on_click(toggle_local)
                .hover(|s| s.bg(rgb(BG_SURFACE)))
                .child(header_label),
        );

        if !local_collapsed {
            for (branch_name, is_head) in &local_filtered {
                let is_head = *is_head;
                // Upstream info: ↑A ↓B
                let upstream_label: Option<SharedString> = if let Some(u) = branch_upstream_info.get(branch_name.as_str()) {
                    if u.ahead > 0 || u.behind > 0 {
                        Some(SharedString::from(format!("\u{2191}{} \u{2193}{}", u.ahead, u.behind)))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let label = if is_head {
                    SharedString::from(format!("\u{2713} {}", branch_name))
                } else {
                    SharedString::from(branch_name.clone())
                };
                let text_color = if is_head { COLOR_SUCCESS } else { TEXT_MAIN };
                let branch_for_click = branch_name.clone();

                let full_name = SharedString::from(branch_name.to_string());
                let row: gpui::AnyElement = if is_head {
                    div()
                        .id(SharedString::from(format!("sidebar-branch-{}", branch_name)))
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
                        .child(div().flex_1().truncate().child(label))
                        .when_some(upstream_label, |el, ul| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .ml_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_SUB))
                                    .child(ul),
                            )
                        })
                        .into_any()
                } else {
                    let branch_for_delete = branch_name.clone();
                    let click_handler = cx.listener(move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.open_plan_modal(branch_for_click.clone());
                        } else {
                            this.jump_to_branch(&branch_for_click);
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
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(BG_SURFACE)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(label))
                        .when_some(upstream_label, |el, ul| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .ml_2()
                                    .text_xs()
                                    .text_color(rgb(TEXT_SUB))
                                    .child(ul),
                            )
                        })
                        // ✕ delete button: always visible (small, muted) for non-HEAD branches.
                        // Implementation note: gpui hover-group is not stable API in 0.2.x;
                        // always-visible small ✕ in muted colour is the safe choice.
                        .child(
                            div()
                                .id(SharedString::from(format!("sidebar-delete-{}", branch_name)))
                                .flex_shrink_0()
                                .ml_1()
                                .px_1()
                                .text_xs()
                                .text_color(rgb(TEXT_MUTED))
                                .on_click(delete_handler)
                                .hover(|s| s.text_color(rgb(0xf38ba8)))
                                .child(SharedString::from("\u{00d7}")),
                        )
                        .into_any()
                };

                col = col.child(row);
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
                .text_color(rgb(TEXT_MUTED))
                .on_click(toggle_remote)
                .hover(|s| s.bg(rgb(BG_SURFACE)))
                .child(header_label),
        );

        if !remote_collapsed {
            for rb in &remote_filtered {
                let display = format!("{}/{}", rb.remote, rb.name);
                let rb_target = rb.target.clone();
                let display_label = SharedString::from(display.clone());

                // Check if this remote commit is in our row index (for jump).
                let can_jump = commit_row_index.contains_key(&rb_target);

                let full_name = SharedString::from(display.clone());
                let row: gpui::AnyElement = if can_jump {
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
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(COLOR_REMOTE))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(BG_SURFACE)))
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(display_label))
                        .into_any()
                } else {
                    div()
                        .id(SharedString::from(format!("sidebar-remote-{}", display)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(rgb(COLOR_REMOTE))
                        .overflow_hidden()
                        .tooltip(name_tooltip(full_name))
                        .child(div().flex_1().truncate().child(display_label))
                        .into_any()
                };

                col = col.child(row);
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
                .text_color(rgb(TEXT_MUTED))
                .on_click(toggle_tags)
                .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                        .text_color(rgb(COLOR_TAG))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(BG_SURFACE)))
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
                        .text_color(rgb(COLOR_TAG))
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
                .text_color(rgb(TEXT_MUTED))
                .on_click(toggle_worktrees)
                .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                let text_color = if wt.is_current { COLOR_SUCCESS } else { TEXT_SUB };
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
                .text_color(rgb(TEXT_MUTED))
                .on_click(toggle_stashes)
                .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                        .text_color(rgb(COLOR_WARNING))
                        .overflow_hidden()
                        .on_click(click_handler)
                        .hover(|style| style.bg(rgb(BG_SURFACE)))
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
        .bg(rgb(BG_SIDEBAR))
        .child(col)
}
