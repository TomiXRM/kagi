//! Graph sidebar: five independently scrolling, vertically resizable navigator panes (#864).

use std::ops::Range;

use gpui::{
    canvas, div, prelude::*, px, rgb, uniform_list, App, Bounds, Context, Pixels, SharedString,
    Window,
};
use gpui_component::input::Input;
use gpui_component::Sizable as _;

use super::sidebar::{
    build_sidebar_row, SidebarRow, SidebarState, SECTION_WORKTREES, SIDEBAR_ROW_H,
    SIDEBAR_WORKTREE_ROW_H,
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
    /// Only an explicit header click or divider drag writes the layout. The
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
        if pair_height <= 2. * SIDEBAR_ROW_H * zoom + 1. || second_top < first_bottom {
            return false;
        }
        // Each pane keeps its header plus one of its own body rows visible.
        let first_minimum = ((SIDEBAR_ROW_H + body_row_h(index)) * zoom).min(pair_height / 2.);
        let second_minimum = ((SIDEBAR_ROW_H + body_row_h(next)) * zoom).min(pair_height / 2.);
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
fn render_pane(app: &KagiApp, index: usize, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let range = &app.sidebar.pane_ranges[index];
    let Some(header) = app.sidebar.rows.get(range.start) else {
        return div().into_any_element();
    };
    let pane_id = PANE_IDS[index];
    let collapsed = app.sidebar.collapsed.contains(SECTIONS[index]);
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
    let body_count = range.end.saturating_sub(body_start);
    let scroll_handle = app.sidebar.scroll_handles[index].clone();
    let row_h = theme::scaled_px(body_row_h(index));
    let list = super::with_vertical_scrollbar(
        SCROLL_IDS[index],
        &scroll_handle,
        uniform_list(
            pane_id,
            body_count,
            cx.processor(move |this, visible: Range<usize>, _window, cx| {
                let now_secs = super::commit_list::now_unix_secs();
                this.sidebar.refresh_tree_positions();
                // What the pane's keyboard Tab stop may land on (#981).
                this.sidebar.focus.drawn[index] = visible.clone();
                let rows = this.sidebar.focus.lists[index].clone();
                visible
                    .filter_map(|position| {
                        let absolute = body_start + position;
                        let row = this.sidebar.rows.get(absolute).cloned()?;
                        let spec = super::sidebar_a11y::tree_item(&row);
                        let el = build_sidebar_row(this, &row, now_secs, cx);
                        let slot = div().id((pane_id, absolute)).w_full().h(row_h);
                        let slot = match rows.as_deref().filter(|rows| position < rows.len()) {
                            Some(rows) => {
                                super::sidebar_focus::row(this, rows, position, slot, row, el, cx)
                            }
                            None => slot.child(el),
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
        Some(rows) => rows.list(list),
        None => list,
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
            .when(!collapsed, |el| el.child(list)),
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
        .relative()
        .w_full()
        .flex()
        .flex_col()
        .min_h(theme::scaled_px(SIDEBAR_ROW_H))
        .when(collapsed, |el| {
            el.h(theme::scaled_px(SIDEBAR_ROW_H)).flex_shrink_0()
        })
        .when(!collapsed, |el| {
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
        .relative()
        .child(super::e2e::measure_inside(PANE_DIVIDER_IDS[index]))
        .w_full()
        .h(theme::scaled_px(4.))
        .flex_shrink_0()
        .bg(rgb(theme().surface))
        .when(active, |el| {
            el.cursor_row_resize()
                .hover(|s| s.bg(rgb(theme().color_branch)).cursor_row_resize())
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
    let filter_input = app.sidebar.filter.clone();
    // The five pane bodies share `sidebar.rows`, never copied on layout changes.
    // ADR-0128: the badge counts merged-class rows only (stale-only rows are
    // listed in the table but don't count as "merged").
    let cleanup_count = app
        .view()
        .cleanup_rows
        .iter()
        .filter(|r| r.status != kagi_git::ops::MergedBranchStatus::NotMerged)
        .count();

    // ── Filter input row (pinned above the virtualized list) ──────
    let filter_area: gpui::AnyElement = if let Some(input_entity) = &filter_input {
        div()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(Input::new(input_entity).xsmall().appearance(true))
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
                    .h(theme::scaled_px(22.))
                    .flex()
                    .items_center()
                    .px_2()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .bg(rgb(theme().bg_base))
                    .rounded(theme::scaled_px(4.))
                    .child(SharedString::from("filter…")),
            )
            .into_any_element()
    };

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

    let panes = (0..SECTIONS.len()).fold(
        div()
            .id("sidebar-panes")
            .relative()
            .child(super::e2e::measure_inside("sidebar-panes"))
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            // Five scaled headers may exceed a short Graph viewport; retain
            // every pane and let the stack scroll instead of painting past the
            // sidebar's bottom edge. Leaf lists still scroll independently.
            .overflow_y_scroll(),
        |container, index| {
            let container = container.child(render_pane(app, index, cx));
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
