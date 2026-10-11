use super::commit_panel::CommitPanelView;
use super::commit_panel_render::*;
use super::render_helpers::*;
use super::*;

impl CommitPanelView {
    pub(super) fn render_files_toggle(
        &self,
        staged: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let tree_view = self.state.tree_view;
        let list_click = cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                view.state.tree_view = false;
                cx.notify();
            },
        );
        let tree_click = cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                view.state.tree_view = true;
                cx.notify();
            },
        );
        div()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .rounded_sm()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(theme().surface))
            .child(
                self.render_files_segment(
                    if staged {
                        "cp-staged-view-list"
                    } else {
                        "cp-view-list"
                    },
                    if compact {
                        Msg::CompactList.t()
                    } else {
                        "List"
                    },
                    !tree_view,
                )
                .when(compact, |el| {
                    super::keyboard_nav::focusable(el)
                        .role(gpui::Role::Button)
                        .aria_label(Msg::CompactList.t())
                })
                .on_click(list_click),
            )
            .child(
                self.render_files_segment(
                    if staged {
                        "cp-staged-view-tree"
                    } else {
                        "cp-view-tree"
                    },
                    if compact {
                        Msg::CompactTree.t()
                    } else {
                        "Tree"
                    },
                    tree_view,
                )
                .when(compact, |el| {
                    super::keyboard_nav::focusable(el)
                        .role(gpui::Role::Button)
                        .aria_label(Msg::CompactTree.t())
                })
                .on_click(tree_click),
            )
    }

    pub(super) fn render_files_segment(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .relative()
            .child(super::e2e::measure_inside(id))
            .px_1p5()
            .py_px()
            .text_xs()
            .bg(if active {
                theme().selected_for_window(self.window_active)
            } else {
                rgb(theme().surface).into()
            })
            .text_color(rgb(if active {
                theme().text_main
            } else {
                theme().text_muted
            }))
            .hover(|st| st.text_color(rgb(theme().text_main)).cursor_pointer())
            .child(SharedString::from(label))
    }
    pub(super) fn render_unstaged_header(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let unstaged_count = self.state.unstaged.len();
        let toggle_btn = self.render_files_toggle(false, compact, cx);
        div()
            .flex()
            .flex_row()
            .when(compact, |el| el.flex_wrap().gap_1())
            .items_center()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .when(compact, |el| el.w_full().flex_none())
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(format!(
                        "{} ({})",
                        if compact {
                            Msg::CompactUnstaged.t()
                        } else {
                            "Unstaged"
                        },
                        unstaged_count
                    ))),
            )
            // #476: live on a worktree panel too — it stages into the worktree.
            .when(compact || unstaged_count > 0, |el| {
                el.child(self.render_bulk_action(false, compact, cx))
            })
            // W17-DISCARD: "Discard all" — disabled (muted, no handler) at 0 targets.
            .when(unstaged_count > 0, |el| {
                el.child(self.render_discard_all(compact, cx))
            })
            .child(toggle_btn)
    }
    pub(super) fn render_staged_header(&self, compact: bool, cx: &mut Context<Self>) -> gpui::Div {
        let staged_count = self.state.staged.len();
        div()
            .flex()
            .flex_row()
            .when(compact, |el| el.flex_wrap().gap_1())
            .items_center()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .when(compact, |el| el.w_full().flex_none())
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(format!(
                        "{} ({})",
                        if compact {
                            Msg::CompactStaged.t()
                        } else {
                            "Staged"
                        },
                        staged_count
                    ))),
            )
            // #476: live on a worktree panel too — see "Stage all" above.
            .when(compact || staged_count > 0, |el| {
                el.child(self.render_bulk_action(true, compact, cx))
            })
            .when(compact, |el| {
                el.child(self.render_files_toggle(true, compact, cx))
            })
    }

    fn render_bulk_action(
        &self,
        staged: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let enabled = if staged {
            !self.state.staged.is_empty()
        } else {
            !self.state.unstaged.is_empty()
        };
        let id = if staged {
            "cp-unstage-all"
        } else {
            "cp-stage-all"
        };
        let label = match (staged, compact) {
            (true, true) => Msg::CompactUnstageAll.t(),
            (false, true) => Msg::CompactStageAll.t(),
            (true, false) => "Unstage all",
            (false, false) => "Stage all",
        };
        let click = cx.listener(
            move |view: &mut CommitPanelView, _: &gpui::ClickEvent, window, cx| {
                if staged {
                    view.defer_unstage_all(window, cx);
                } else {
                    view.defer_stage_all(window, cx);
                }
            },
        );
        div()
            .id(id)
            .when(!staged, |el| el.mr_2())
            .px_1p5()
            .py_px()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .text_xs()
            .text_color(rgb(if !enabled {
                theme().text_muted
            } else if staged {
                theme().color_warning
            } else {
                theme().color_success
            }))
            .when(enabled, |el| {
                el.hover(|style| style.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(click)
            })
            .when(compact, |el| el.role(gpui::Role::Button).aria_label(label))
            .when(compact && enabled, super::keyboard_nav::focusable)
            .when(!enabled, |el| {
                el.a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                    builder.parent_node().set_disabled();
                })
            })
            .relative()
            .child(super::e2e::measure_inside(id))
            .child(SharedString::from(label))
    }

    pub(super) fn render_discard_all(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let discard_eligible_count = self
            .state
            .unstaged
            .iter()
            .filter(|f| !self.state.is_conflicted(&f.path))
            .count();
        let discard_all_click = cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                view.defer_open_discard_all(window, cx);
            },
        );
        let enabled = discard_eligible_count > 0;
        let mut btn = div()
            .id("cp-discard-all")
            .mr_2()
            .px_1p5()
            .py_px()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .text_xs()
            .child(SharedString::from(if compact {
                Msg::CompactDiscardAll.t()
            } else {
                "Discard all"
            }));
        if enabled {
            btn = btn
                .text_color(rgb(theme().color_blocker))
                .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                .on_click(discard_all_click);
        } else {
            btn = btn.text_color(rgb(theme().text_muted));
        }
        #[cfg(feature = "gui-e2e")]
        let btn = btn
            .relative()
            .child(super::e2e::measure_inside("cp-discard-all"));
        btn
    }
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
            .flex_1()
            .min_h(px(0.))
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
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.unstaged_tree_layout.as_slice());
                            range
                                .filter_map(|i| match &layout {
                                    Some(layout) => render_unstaged_tree_row(
                                        view,
                                        i,
                                        cp_tree_slot(layout, "cp-unstaged-tree", i),
                                        cx,
                                    ),
                                    None => render_unstaged_flat_row(view, i, cx),
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
            .flex_1()
            .min_h(px(0.))
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
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.staged_tree_layout.as_slice());
                            range
                                .filter_map(|i| match &layout {
                                    Some(layout) => render_staged_tree_row(
                                        view,
                                        i,
                                        cp_tree_slot(layout, "cp-staged-tree", i),
                                        cx,
                                    ),
                                    None => render_staged_flat_row(view, i, cx),
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
