use super::commit_panel::CommitPanelView;
use super::commit_panel_rows::*;
use super::render_helpers::*;
use super::*;

impl CommitPanelView {
    pub(super) fn render_unstaged_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let panel = &self.state;
        let tree_view = panel.tree_view;
        let unstaged_scroll_handle = self.unstaged_scroll_handle.clone();
        let unstaged_row_count = if tree_view {
            panel.unstaged_tree.len()
        } else {
            panel.unstaged_normal_files.len()
        } + panel.generated_extra_rows(false)
            + panel.agent_extra_rows(false);
        div()
            .id("cp-unstaged-scroll")
            .relative()
            .when_some(self.unstaged_navigation.rows.as_ref(), |el, rows| {
                rows.list(el)
            })
            .when(!tree_view, |el| {
                el.role(gpui::Role::ListBox)
                    .aria_label(Msg::A11yUnstagedTree.t())
            })
            .child(super::e2e::measure_inside("cp-us-viewport"))
            .flex_1()
            .min_h(theme::scaled_px(26.) + px(4.))
            .mx_1()
            .mb_px()
            .border_1()
            .border_color(rgb(theme().surface))
            .rounded_sm()
            .flex()
            .flex_col()
            .child({
                let handle = unstaged_scroll_handle.clone();
                let list = with_vertical_scrollbar(
                    "cp-unstaged-list-scroll",
                    &handle,
                    uniform_list(
                        "cp-unstaged-list",
                        unstaged_row_count,
                        cx.processor(move |view, range: std::ops::Range<usize>, _window, cx| {
                            let rows = view.unstaged_navigation.drawn(&range, _window, cx);
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.unstaged_tree_layout.as_slice());
                            range
                                .filter_map(|i| match &layout {
                                    Some(layout) => render_unstaged_tree_row(
                                        view,
                                        &rows,
                                        i,
                                        cp_tree_slot(layout, "cp-unstaged-tree", i),
                                        cx,
                                    ),
                                    None => render_unstaged_flat_row(view, &rows, i, cx),
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&unstaged_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    false,
                );
                if tree_view {
                    kagi_ui_core::tree_a11y::tree(
                        "cp-unstaged-tree",
                        list,
                        Msg::A11yUnstagedTree.t(),
                    )
                } else {
                    list
                }
            })
    }
    pub(super) fn render_staged_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let panel = &self.state;
        let tree_view = panel.tree_view;
        let staged_scroll_handle = self.staged_scroll_handle.clone();
        let staged_row_count = if tree_view {
            panel.staged_tree.len()
        } else {
            panel.staged_normal_files.len()
        } + panel.generated_extra_rows(true)
            + panel.agent_extra_rows(true);
        div()
            .id("cp-staged-scroll")
            .relative()
            .when_some(self.staged_navigation.rows.as_ref(), |el, rows| {
                rows.list(el)
            })
            .when(!tree_view, |el| {
                el.role(gpui::Role::ListBox)
                    .aria_label(Msg::A11yStagedTree.t())
            })
            .child(super::e2e::measure_inside("cp-st-viewport"))
            .flex_1()
            .min_h(theme::scaled_px(26.) + px(4.))
            .mx_1()
            .mb_px()
            .border_1()
            .border_color(rgb(theme().surface))
            .rounded_sm()
            .flex()
            .flex_col()
            .child({
                let handle = staged_scroll_handle.clone();
                let list = with_vertical_scrollbar(
                    "cp-staged-list-scroll",
                    &handle,
                    uniform_list(
                        "cp-staged-list",
                        staged_row_count,
                        cx.processor(move |view, range: std::ops::Range<usize>, _window, cx| {
                            let rows = view.staged_navigation.drawn(&range, _window, cx);
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.staged_tree_layout.as_slice());
                            range
                                .filter_map(|i| match &layout {
                                    Some(layout) => render_staged_tree_row(
                                        view,
                                        &rows,
                                        i,
                                        cp_tree_slot(layout, "cp-staged-tree", i),
                                        cx,
                                    ),
                                    None => render_staged_flat_row(view, &rows, i, cx),
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&staged_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    false,
                );
                if tree_view {
                    kagi_ui_core::tree_a11y::tree("cp-staged-tree", list, Msg::A11yStagedTree.t())
                } else {
                    list
                }
            })
    }
}
