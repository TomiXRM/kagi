//! Graph sidebar: five independently scrolling, vertically resizable navigator panes (#864).

use std::ops::Range;

use gpui::{
    canvas, div, prelude::*, px, rgb, uniform_list, App, Bounds, Context, EntityInputHandler,
    Pixels, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::Input,
    ActiveTheme as _, Icon, IconName, Sizable as _,
};

use super::sidebar::{
    build_sidebar_row, SidebarRow, SidebarState, SECTION_WORKTREES, SIDEBAR_ROW_H,
    SIDEBAR_SECTION_HEADER_H, SIDEBAR_WORKTREE_ROW_H,
};
use super::theme::{self, theme};
use super::{DividerDrag, DividerGhost, DividerKind, KagiApp, Msg};

pub const SECTIONS: [&str; 5] = [
    super::sidebar::SECTION_LOCAL,
    super::sidebar::SECTION_REMOTE,
    SECTION_WORKTREES,
    super::sidebar::SECTION_TAGS,
    super::sidebar::SECTION_STASHES,
];

const PANE_IDS: [&str; 5] = [
    "sidebar-local",
    "sidebar-remote",
    "sidebar-worktrees",
    "sidebar-tags",
    "sidebar-stashes",
];
const SCROLL_IDS: [&str; 5] = [
    "sidebar-local-scroll",
    "sidebar-remote-scroll",
    "sidebar-worktrees-scroll",
    "sidebar-tags-scroll",
    "sidebar-stashes-scroll",
];
const PANE_DIVIDER_IDS: [&str; 4] = [
    "sidebar-local-divider",
    "sidebar-remote-divider",
    "sidebar-worktrees-divider",
    "sidebar-tags-divider",
];
const PANE_LINE_IDS: [&str; 4] = [
    "sidebar-local-divider-line",
    "sidebar-remote-divider-line",
    "sidebar-worktrees-divider-line",
    "sidebar-tags-divider-line",
];
const PANE_LABELS: [Msg; 5] = [
    Msg::A11ySidebarLocal,
    Msg::A11ySidebarRemote,
    Msg::A11ySidebarWorktrees,
    Msg::A11ySidebarTags,
    Msg::A11ySidebarStashes,
];

/// Unscaled uniform row height of one pane's leaf list. `uniform_list` sizes a
/// list from its first row, and lists are per pane, so only the WORKTREES list
/// (worktree rows only) is taller; every other list uses the header's slot.
fn body_row_h(index: usize) -> f32 {
    if SECTIONS[index] == SECTION_WORKTREES {
        SIDEBAR_WORKTREE_ROW_H
    } else {
        SIDEBAR_ROW_H
    }
}

/// Ranges share the existing flattened row cache; a drag never clones or
/// regroups refs. Each range starts at a section header.
pub(super) fn pane_ranges(rows: &[SidebarRow]) -> [Range<usize>; 5] {
    let mut ranges = std::array::from_fn(|_| 0..0);
    let mut previous: Option<usize> = None;
    for (row_index, row) in rows.iter().enumerate() {
        if let SidebarRow::SectionHeader { section, .. } = row {
            if let Some(index) = previous {
                ranges[index].end = row_index;
            }
            if let Some(index) = SECTIONS.iter().position(|name| name == section) {
                ranges[index].start = row_index;
                previous = Some(index);
            }
        }
    }
    if let Some(index) = previous {
        ranges[index].end = rows.len();
    }
    ranges
}

impl SidebarState {
    /// Only an explicit pane toggle or divider drag writes the layout. The
    /// HashSet remains the sole in-memory collapse owner.
    pub(super) fn persist_panes(&self) {
        let collapsed_mask = SECTIONS.iter().enumerate().fold(0, |mask, (index, key)| {
            if self.collapsed.contains(key) {
                mask | (1 << index)
            } else {
                mask
            }
        });
        let value = super::settings::SidebarPaneLayout {
            weights: self.pane_weights,
            collapsed_mask,
        }
        .encode();
        super::settings::write_setting("sidebar_panes", Some(&value));
    }

    pub(super) fn toggle_section(&mut self, section: &'static str) {
        if !self.collapsed.remove(section) {
            self.collapsed.insert(section);
        }
        self.persist_panes();
    }

    /// Adjust the two nearest expanded panes around this divider. The flex
    /// weights size whole panes (including headers), so measured whole-pane
    /// heights and the cursor position must use that same coordinate system.
    pub(super) fn resize_pane_pair(&mut self, index: usize, cursor_y: f32, zoom: f32) -> bool {
        if index + 1 >= SECTIONS.len() || self.collapsed.contains(SECTIONS[index]) {
            return false;
        }
        let Some(next) =
            (index + 1..SECTIONS.len()).find(|&n| !self.collapsed.contains(SECTIONS[n]))
        else {
            return false;
        };
        let (top, first_bottom) = self.pane_geom[index].get();
        let (second_top, bottom) = self.pane_geom[next].get();
        let pair_height = (first_bottom - top) + (bottom - second_top);
        if pair_height <= 2. * SIDEBAR_SECTION_HEADER_H * zoom + 1. || second_top < first_bottom {
            return false;
        }
        // Each pane keeps its header plus one of its own body rows visible.
        let first_minimum =
            ((SIDEBAR_SECTION_HEADER_H + body_row_h(index)) * zoom).min(pair_height / 2.);
        let second_minimum =
            ((SIDEBAR_SECTION_HEADER_H + body_row_h(next)) * zoom).min(pair_height / 2.);
        let first_height =
            (cursor_y - top - 2. * zoom).clamp(first_minimum, pair_height - second_minimum);
        let total_weight = u32::from(self.pane_weights[index]) + u32::from(self.pane_weights[next]);
        let weight = ((first_height / pair_height) * total_weight as f32).round() as u32;
        let lower = total_weight.saturating_sub(u16::MAX as u32).max(1);
        let upper = total_weight.saturating_sub(1).min(u16::MAX as u32);
        let weight = weight.clamp(lower, upper);
        if u32::from(self.pane_weights[index]) == weight {
            return false;
        }
        self.pane_weights[index] = weight as u16;
        self.pane_weights[next] = (total_weight - weight) as u16;
        self.persist_panes();
        true
    }
}

/// A pane keeps its header fixed while only its leaves scroll. All five Tree
/// wrappers exist, even for empty sections.
pub(super) fn render_pane(
    app: &KagiApp,
    index: usize,
    compact: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let range = &app.sidebar.pane_ranges[index];
    let Some(header) = app.sidebar.rows.get(range.start) else {
        return div().into_any_element();
    };
    let pane_id = PANE_IDS[index];
    let collapsed = app.sidebar.collapsed.contains(SECTIONS[index]);
    let (reveal, natural_body, closing_count) = {
        let motion = app.sidebar.motion.borrow();
        let pane = &motion.panes[index];
        (
            pane.reveal
                .sample(px(pane.natural_body), kagi_ui_core::motion::now()),
            pane.natural_body,
            pane.closing_rows.len(),
        )
    };
    let header_spec = super::sidebar_a11y::tree_item(header);
    let header_el = build_sidebar_row(app, header, super::commit_list::now_unix_secs(), cx);
    let header_el = super::list_a11y::tree_item(
        pane_id,
        super::sidebar_focus::header(app, index, collapsed, header_el, cx),
        0,
        &header_spec,
        (1, 1),
    );
    let body_start = range.start + 1;
    let body_count = if collapsed {
        closing_count
    } else {
        range.end.saturating_sub(body_start)
    };
    let scroll_handle = app.sidebar.scroll_handles[index].clone();
    let row_h = theme::scaled_px(body_row_h(index));
    let list = super::with_vertical_scrollbar(
        SCROLL_IDS[index],
        &scroll_handle,
        uniform_list(
            pane_id,
            body_count,
            cx.processor(move |this, visible: Range<usize>, window, cx| {
                let pitch = this.sidebar.scroll_handles[index]
                    .0
                    .borrow()
                    .last_item_size
                    .filter(|_| body_count > 0)
                    .map_or(row_h, |size| size.contents.height / body_count as f32);
                let now_secs = super::commit_list::now_unix_secs();
                if collapsed {
                    // Closing content is real, retained row data, but never a
                    // logical row, focus target, tooltip or accessibility node.
                    this.sidebar.focus.drawn[index] = 0..0;
                    let motion = this.sidebar.motion.borrow();
                    return visible
                        .filter_map(|position| {
                            let row = motion.panes[index].closing_rows.get(position)?;
                            Some(
                                super::sidebar::probe_row_motion(
                                    div().relative().w_full().h(row_h).child(
                                        super::sidebar::build_sidebar_row_presentation(
                                            this, row, now_secs, false, cx,
                                        ),
                                    ),
                                    row,
                                    index,
                                    "closing",
                                )
                                .into_any_element(),
                            )
                        })
                        .collect::<Vec<_>>();
                }
                this.sidebar.refresh_tree_positions();
                // What the pane's keyboard Tab stop may land on (#981). The
                // range can change after this frame's stop was picked (a
                // wheel, a divider, a resize): the rows drawn now take the
                // stop at once, and a frame is asked for so the focus of a
                // row that scrolled out moves on (#987 review).
                let focus = &mut this.sidebar.focus;
                if focus.seen[index] != visible {
                    focus.seen[index] = visible.clone();
                    window.request_animation_frame();
                }
                focus.drawn[index] = visible.clone();
                let rows = focus.lists[index]
                    .as_deref()
                    .map(|rows| rows.as_drawn(&visible));
                let selected = rows.as_ref().and_then(|rows| {
                    rows.focused_stop(window, cx).filter(|&at| {
                        rows.focus_handle(at).is_focused(window) && window.last_input_was_keyboard()
                    })
                });
                let moving = this.sidebar.motion.borrow_mut().panes[index].sync_wash(selected);
                kagi_ui_core::motion::request_frame(window, moving);
                let visible_start = visible.start;
                let visible_end = visible.end;
                visible
                    .filter_map(|position| {
                        let absolute = body_start + position;
                        let row = this.sidebar.rows.get(absolute).cloned()?;
                        let leaf = match &row {
                            SidebarRow::LocalBranchLeaf { .. } | SidebarRow::Stash { .. } => true,
                            SidebarRow::RemoteLeaf { target, .. }
                            | SidebarRow::Tag { target, .. } => {
                                this.view().commit_row_index.contains_key(target)
                            }
                            SidebarRow::Worktree { .. } => this.remote_view.is_none(),
                            _ => false,
                        };
                        // Peek inspects without leaving the stash tree; keyboard
                        // users must retain its OID-keyed row focus.
                        let dismiss_keyboard_selection =
                            compact && leaf && !matches!(&row, SidebarRow::Stash { .. });
                        let worktree =
                            (compact && leaf && matches!(&row, SidebarRow::Worktree { .. }))
                                .then(|| row.clone());
                        let spec = super::sidebar_a11y::tree_item(&row);
                        let el = build_sidebar_row(this, &row, now_secs, cx);
                        let motion = this.sidebar.motion.borrow();
                        let pane = &motion.panes[index];
                        let sample = pane.row_sample(position, pitch);
                        let ring_opacity = pane.wash(position);
                        let wash = super::sidebar::probe_row_motion(
                            div().absolute().size_full().opacity(ring_opacity),
                            &row,
                            index,
                            "ring",
                        );
                        let el = super::sidebar::probe_row_motion(
                            div()
                                .relative()
                                .w_full()
                                .h(row_h)
                                .top(sample.offset)
                                .opacity(sample.opacity)
                                .child(wash)
                                .child(el),
                            &row,
                            index,
                            "row",
                        )
                        // Claim only this real translated body's semantic
                        // events, after its child callbacks. Fixed slots must
                        // not activate covered siblings or inert edge content.
                        .id(("sidebar-real-content", absolute))
                        .on_click(
                            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                if event.is_keyboard() {
                                    return;
                                }
                                if compact && leaf {
                                    if let Some(row) = &worktree {
                                        super::compact_sidebar::select_worktree(this, row, cx);
                                    }
                                    super::compact_sidebar::dismiss_selection(window, cx);
                                }
                                cx.stop_propagation();
                            }),
                        )
                        .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .into_any_element();
                        // Both real and ghost content share under_ring's origin;
                        // the fixed focus border must not shift a retained row.
                        let mut presentation = div().relative().w_full().h(row_h).child(el);
                        for ghost in pane.ghosts.iter().filter(|ghost| {
                            ghost
                                .mount
                                .max(visible_start)
                                .min(visible_end.saturating_sub(1))
                                == position
                        }) {
                            if let Some(ghost_sample) = pane.ghost_sample(ghost, pitch) {
                                let offset = pitch * (ghost.mount as f32 - position as f32)
                                    + ghost_sample.offset
                                    + if ghost_sample.after.is_some() {
                                        pane.row_sample(ghost.mount, pitch).offset
                                    } else {
                                        px(0.)
                                    };
                                let content = super::sidebar::build_sidebar_row_presentation(
                                    this, &ghost.row, now_secs, false, cx,
                                );
                                presentation =
                                    presentation.child(super::sidebar::probe_row_motion(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(offset)
                                            .w_full()
                                            .h(row_h)
                                            .opacity(ghost_sample.opacity)
                                            // Painted after the real anchor:
                                            // a ghost must not borrow its menu.
                                            .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| {
                                                cx.stop_propagation();
                                            })
                                            .child(content),
                                        &ghost.row,
                                        index,
                                        "ghost",
                                    ));
                            }
                        }
                        if position == visible_start {
                            // Final slots determine keyboard/AX visibility immediately.
                            // A splice can paint a neighboring final slot into this
                            // viewport; draw only that bounded fringe as inert content.
                            if sample.offset > px(0.) {
                                for extra in (0..visible_start).rev() {
                                    let extra_sample = pane.row_sample(extra, pitch);
                                    let top = pitch * (extra as f32 - position as f32)
                                        + extra_sample.offset;
                                    if top + pitch <= px(0.) {
                                        break;
                                    }
                                    if let Some(row) = this.sidebar.rows.get(body_start + extra) {
                                        let content =
                                            super::sidebar::build_sidebar_row_presentation(
                                                this, row, now_secs, false, cx,
                                            );
                                        presentation = presentation.child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .top(top)
                                                .w_full()
                                                .h(row_h)
                                                .opacity(extra_sample.opacity)
                                                .child(content),
                                        );
                                    }
                                }
                            }
                            if visible_end > visible_start
                                && pane.row_sample(visible_end - 1, pitch).offset < px(0.)
                            {
                                for extra in visible_end..body_count {
                                    let extra_sample = pane.row_sample(extra, pitch);
                                    let top = pitch * (extra as f32 - position as f32)
                                        + extra_sample.offset;
                                    if top >= pitch * (visible_end - visible_start) as f32 {
                                        break;
                                    }
                                    if let Some(row) = this.sidebar.rows.get(body_start + extra) {
                                        let content =
                                            super::sidebar::build_sidebar_row_presentation(
                                                this, row, now_secs, false, cx,
                                            );
                                        presentation = presentation.child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .top(top)
                                                .w_full()
                                                .h(row_h)
                                                .opacity(extra_sample.opacity)
                                                .child(content),
                                        );
                                    }
                                }
                            }
                        }
                        let el = presentation.into_any_element();
                        let slot = div().id((pane_id, absolute)).relative().w_full().h(row_h);
                        drop(motion);
                        let slot = match &row {
                            SidebarRow::Tag { name, target } => slot.when_some(
                                super::sidebar_ref_rows::tag_navigation_reason(this, target),
                                |slot, reason| {
                                    slot.aria_description(super::e2e::sidebar_labels::description(
                                        name,
                                        gpui::SharedString::from(reason.t()),
                                    ))
                                },
                            ),
                            _ => slot,
                        };
                        let slot = match rows.as_ref().filter(|rows| position < rows.len()) {
                            Some(rows) => super::sidebar_focus::row(
                                this,
                                rows,
                                position,
                                slot,
                                ring_opacity,
                                (row, el),
                                cx,
                                window,
                            ),
                            None => slot.child(el),
                        };
                        let slot = if dismiss_keyboard_selection {
                            slot.on_click(cx.listener(
                                move |_this, event: &gpui::ClickEvent, window, cx| {
                                    if event.is_keyboard() {
                                        super::compact_sidebar::dismiss_selection(window, cx);
                                    }
                                },
                            ))
                        } else {
                            slot
                        };
                        Some(
                            super::list_a11y::tree_item(
                                pane_id,
                                slot,
                                position + 1,
                                &spec,
                                this.sidebar.tree_positions[absolute],
                            )
                            .into_any_element(),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h(px(0.)),
        false,
    );
    // ↑/↓ are the rows' only while the focus is in this pane's list.
    let list = match app.sidebar.focus.lists[index].as_deref() {
        Some(rows) => rows
            .list(list)
            .key_context("KagiRowList KagiSidebarPaneRows"),
        None => list,
    };
    let list = if !reveal.done {
        kagi_ui_core::motion::clip(
            ("sidebar-pane-body-clip", index),
            kagi_ui_core::motion::Axis::Vertical,
            kagi_ui_core::motion::Anchor::Start,
            px(natural_body),
            reveal.fraction,
            list.h(px(natural_body)).flex_none(),
        )
        .w_full()
        .into_any_element()
    } else {
        list.into_any_element()
    };
    let tree = super::list_a11y::tree(
        pane_id,
        div()
            .id(("sidebar-pane-tree", index))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(header_el)
            .when(reveal.retain_content, |el| el.child(list)),
        PANE_LABELS[index].t(),
    );
    let geom = app.sidebar.pane_geom[index].clone();
    let measure = canvas(
        move |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |bounds: Bounds<Pixels>, _prepaint: (), _window: &mut Window, _cx: &mut App| {
            let top = f32::from(bounds.origin.y);
            geom.set((top, top + f32::from(bounds.size.height)));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .id(("sidebar-pane", index))
        .on_action(cx.listener(
            move |this, _: &super::sidebar_focus::SidebarTreeLeft, window, cx| {
                this.sidebar_tree_left(index, window, cx);
            },
        ))
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .min_h(theme::scaled_px(SIDEBAR_SECTION_HEADER_H))
        .when(collapsed || !reveal.done, |el| {
            el.h(theme::scaled_px(SIDEBAR_SECTION_HEADER_H) + reveal.height)
                .flex_shrink_0()
        })
        .when(!collapsed && reveal.done, |el| {
            el.flex_basis(px(0.))
                .flex_grow(f32::from(app.sidebar.pane_weights[index]))
                .flex_shrink(1.)
        })
        .child(measure)
        .child(tree)
        .into_any_element()
}

fn pane_divider(app: &KagiApp, index: usize) -> gpui::AnyElement {
    let active = !app.sidebar.collapsed.contains(SECTIONS[index])
        && SECTIONS[index + 1..]
            .iter()
            .any(|key| !app.sidebar.collapsed.contains(key));
    div()
        .id(("sidebar-pane-divider", index))
        .group(PANE_DIVIDER_IDS[index])
        .relative()
        .child(super::e2e::measure_inside(PANE_DIVIDER_IDS[index]))
        .w_full()
        .h(theme::scaled_px(4.))
        .flex_shrink_0()
        .bg(rgb(theme().sidebar))
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(1.))
                .flex_shrink_0()
                .bg(rgb(theme().surface))
                .when(active, |line| {
                    line.group_hover(PANE_DIVIDER_IDS[index], |s| s.bg(rgb(theme().ui_accent())))
                        .group_drag_over::<DividerDrag>(PANE_DIVIDER_IDS[index], |s| {
                            s.bg(rgb(theme().ui_accent()))
                        })
                })
                .when(cfg!(feature = "gui-e2e"), |line| {
                    line.child(super::e2e::measure_inside(PANE_LINE_IDS[index]))
                }),
        )
        .when(active, |el| {
            el.cursor_row_resize()
                .hover(|s| s.bg(rgb(theme().ui_accent())).cursor_row_resize())
                .drag_over::<DividerDrag>(move |style, drag, _, _| {
                    if drag.kind == DividerKind::SidebarPane(index as u8) {
                        style.bg(rgb(theme().ui_accent()))
                    } else {
                        style
                    }
                })
                .on_drag(
                    DividerDrag {
                        kind: DividerKind::SidebarPane(index as u8),
                    },
                    |_drag, _pos, _window, cx| cx.new(|_| DividerGhost),
                )
        })
        .into_any_element()
}

// ──────────────────────────────────────────────────────────────
// render_sidebar — main entry point
// ──────────────────────────────────────────────────────────────

/// Render five Graph navigator sections with pinned headers and independent
/// virtualized bodies. Filter and branch cleanup stay above the panes; worktree
/// inspection is each local worktree row's hover card, not a pane here.
/// `render` owns the cached flat rows and ranges, so drawing another workspace
/// page cannot start a Git read or rebuild refs.
pub fn render_sidebar(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let filter_area = render_filter(app, cx);
    // The five pane bodies share `sidebar.rows`, never copied on layout changes.
    // ADR-0128: the badge counts merged-class rows only (stale-only rows are
    // listed in the table but don't count as "merged").
    let cleanup_count = app
        .view()
        .cleanup_rows
        .iter()
        .filter(|r| r.status != kagi_git::ops::MergedBranchStatus::NotMerged)
        .count();

    // ── Branch Cleanup entry (ADR-0128, pinned above the list) ────
    // "Merged branches (N)" — N counts the merged-class rows (full / squash?
    // / grown); stale-only rows are in the table but not in the badge.
    let cleanup_entry: gpui::AnyElement = {
        let open_handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _window, cx| {
            this.toggle_branch_cleanup_view(cx);
        });
        let count_color = if cleanup_count > 0 {
            theme().color_branch
        } else {
            theme().text_muted
        };
        div()
            .id("sidebar-cleanup-entry")
            .mx_2()
            .my_1()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .rounded(theme::scaled_px(4.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(open_handler)
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::CleanupTitle.t())),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(count_color))
                    .child(SharedString::from(format!("({})", cleanup_count))),
            )
            .into_any_element()
    };

    let viewport_height = app.sidebar.pane_viewport_height.clone();
    let measure_viewport = canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), _, _| {
            viewport_height.set(f32::from(bounds.size.height));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let panes = (0..SECTIONS.len()).fold(
        div()
            .id("sidebar-panes")
            .relative()
            .child(super::e2e::measure_inside("sidebar-panes"))
            .child(measure_viewport)
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            // Five scaled headers may exceed a short Graph viewport; retain
            // every pane and let the stack scroll instead of painting past the
            // sidebar's bottom edge. Leaf lists still scroll independently.
            .overflow_y_scroll(),
        |container, index| {
            let container = container.child(render_pane(app, index, false, cx));
            if index + 1 < SECTIONS.len() {
                container.child(pane_divider(app, index))
            } else {
                container
            }
        },
    );

    // ── Graph page content (the shell around it is the pages renderer) ──
    div()
        .relative()
        .child(super::e2e::measure_inside("worktree-sidebar"))
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .child(filter_area)
        .child(cleanup_entry)
        .child(panes)
        .into_any_element()
}

/// The same filter chrome in wide panes and compact popovers.
pub(super) fn render_filter(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let input_role = gpui::Role::TextInput;
    div()
        .id("sidebar-filter")
        .relative()
        .child(super::e2e::measure_inside("sidebar-filter"))
        .px_2()
        .py_1()
        .flex_shrink_0()
        .when_some(app.sidebar.filter.as_ref(), |row, input| {
            row.child(
                Input::new(input)
                    .context_menu(kagi_ui_core::input::context_menu(input, false))
                    .aria_label(Msg::A11ySidebarFilter.t())
                    .role(input_role)
                    .xsmall()
                    .appearance(true)
                    .cleanable(false)
                    .when(input.read(cx).text().len() != 0, |field| {
                        let input = input.clone();
                        field.suffix(
                            Button::new("sidebar-filter-clear")
                                .icon(Icon::new(IconName::CircleX))
                                .ghost()
                                .xsmall()
                                .tab_stop(false)
                                .text_color(cx.theme().muted_foreground)
                                .aria_label(Msg::A11ySidebarClearSearch.t())
                                .on_click(move |_, window, cx| {
                                    input.update(cx, |state, cx| {
                                        // clean is private on the current pin. Preserve its
                                        // Change event and ordinary typing-history grouping.
                                        let len = state.text().chars().map(char::len_utf16).sum();
                                        state.replace_text_in_range(Some(0..len), "", window, cx);
                                        state.focus(window, cx);
                                    });
                                }),
                        )
                    }),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_sections_still_have_a_header_and_each_pane_owns_only_its_section() {
        let branches = vec![("main".to_string(), true)];
        let rows = super::super::sidebar_rows::build_sidebar_rows(
            &branches,
            &[],
            &[],
            &[],
            &[],
            Default::default(),
            &HashSet::new(),
            &HashSet::new(),
            "",
        );
        let ranges = pane_ranges(&rows);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[SECTIONS.len() - 1].end, rows.len());
        for (index, section) in SECTIONS.iter().enumerate() {
            assert!(matches!(
                &rows[ranges[index].start],
                SidebarRow::SectionHeader { section: actual, .. } if actual == section
            ));
            if index + 1 < SECTIONS.len() {
                assert_eq!(ranges[index].end, ranges[index + 1].start);
            }
        }
        assert!(matches!(
            &rows[ranges[0].start + 1],
            SidebarRow::LocalBranchLeaf { name, .. } if name == "main"
        ));
        assert!(matches!(
            &rows[ranges[3].start],
            SidebarRow::SectionHeader {
                section: "tags",
                count: 0,
                ..
            }
        ));
        assert_eq!(ranges[3].len(), 1);
    }
}
