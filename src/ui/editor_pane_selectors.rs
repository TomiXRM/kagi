//! Editor pane selectors reuse the bin's existing roving TabList owner.
use super::{
    i18n::Msg,
    keyboard_nav::{self, Activation, TabFocus, TabList},
    theme,
};
#[cfg(feature = "gui-e2e")]
use gpui::Role;
use gpui::{div, prelude::*, rgb, AnyElement, App, Context, Window};
use kagi_ui_editor::{EditorPaneSelectors, EditorWorkspaceView, MiddlePaneTab, RightPaneTab};

#[derive(Default)]
pub(super) struct PaneSelectors {
    right: TabFocus,
    middle: TabFocus,
}

impl PaneSelectors {
    fn owner(&self, middle: bool) -> &TabFocus {
        if middle {
            &self.middle
        } else {
            &self.right
        }
    }

    fn list(
        &self,
        view: &EditorWorkspaceView,
        middle: bool,
        selected: usize,
        cx: &mut Context<EditorWorkspaceView>,
    ) -> TabList {
        let entity = cx.entity().downgrade();
        TabList::new(
            self.owner(middle),
            keyboard_nav::slot_keys(2),
            vec![0, 1],
            Some(selected),
            Activation::Automatic,
            Some(view.root_focus.clone()),
            move |slot, keyboard, window, cx| {
                let focused = window.focused(cx);
                let _ = entity.update(cx, |view, cx| {
                    if !middle && !view.details_visible() {
                        return;
                    }
                    if middle {
                        view.set_middle_tab(
                            if slot == 0 {
                                MiddlePaneTab::Diff
                            } else {
                                MiddlePaneTab::Snapshot
                            },
                            cx,
                        );
                    } else {
                        view.set_right_tab(
                            if slot == 0 {
                                RightPaneTab::Diff
                            } else {
                                RightPaneTab::History
                            },
                            window,
                            cx,
                        );
                    }
                });
                if keyboard {
                    if let Some(focused) = focused {
                        focused.focus(window, cx);
                    }
                }
            },
            cx,
        )
    }
}

impl EditorPaneSelectors for PaneSelectors {
    fn render(
        &self,
        view: &EditorWorkspaceView,
        middle: bool,
        cx: &mut Context<EditorWorkspaceView>,
    ) -> AnyElement {
        let selected = if middle {
            usize::from(view.middle_tab == MiddlePaneTab::Snapshot)
        } else {
            usize::from(view.right_tab == RightPaneTab::History)
        };
        // Keep closing labels at their real geometry without rebuilding a roving owner.
        let list =
            (middle || view.details_visible()).then(|| self.list(view, middle, selected, cx));
        let prefix = if middle {
            "ews-middle-tab"
        } else {
            "ews-right-tab"
        };
        let labels = if middle {
            [
                Msg::EditorMiddleTabDiff.t(),
                Msg::EditorMiddleTabSnapshot.t(),
            ]
        } else {
            [Msg::EditorRightTabDiff.t(), Msg::EditorRightTabHistory.t()]
        };
        let strip = div().id(if middle {
            "ews-middle-tabs"
        } else {
            "ews-right-tabs"
        });
        let mut strip = match &list {
            Some(list) => list.list(strip),
            None => strip,
        }
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .gap_1()
        .px_2()
        .py_1()
        .bg(rgb(theme::theme().panel));
        for (slot, label) in labels.into_iter().enumerate() {
            let active = slot == selected;
            let chip = div()
                .id((prefix, slot))
                .relative()
                .w_full()
                .flex()
                .items_center()
                .justify_center()
                .min_h(theme::scaled_px(24.))
                .px(keyboard_nav::inset(8.))
                .rounded_sm()
                .cursor_default()
                .text_xs()
                .text_color(rgb(if active {
                    theme::theme().text_main
                } else {
                    theme::theme().text_muted
                }))
                .bg(rgb(if active {
                    theme::theme().selected
                } else {
                    theme::theme().panel
                }))
                .when(!active, |el| {
                    el.hover(|s| s.bg(rgb(theme::theme().surface)))
                })
                .child(label);
            let cell = match &list {
                Some(list) => list.cell(slot, label, chip),
                None => chip.border_2().border_color(gpui::transparent_black()),
            };
            strip = strip.child(segment_target(middle, slot, cell));
        }
        strip.into_any_element()
    }

    fn release_hidden(&self, view: &EditorWorkspaceView, window: &mut Window, cx: &mut App) {
        if view.open_path.is_none() || !view.details_visible() {
            self.right.sync(&[], cx);
        }
        if view.right_tab != RightPaneTab::History || view.selected_history_commit.is_none() {
            self.middle.sync(&[], cx);
        }
        self.right
            .release_closed(Some(&view.root_focus), window, cx);
        self.middle
            .release_closed(Some(&view.root_focus), window, cx);
    }

    #[cfg(feature = "gui-e2e")]
    fn measure(&self, id: &'static str) -> AnyElement {
        super::e2e::measure_inside(id)
    }

    #[cfg(feature = "gui-e2e")]
    fn measure_toggle(&self, button: AnyElement) -> AnyElement {
        super::e2e::measure_leaf("editor-details-toggle", button)
    }

    #[cfg(feature = "gui-e2e")]
    fn probes(&self, middle: bool) -> Vec<(Role, String, bool, bool)> {
        self.owner(middle).recorded_tabs()
    }
    #[cfg(feature = "gui-e2e")]
    fn focus(&self, middle: bool, slot: usize, window: &mut Window, cx: &mut App) {
        self.owner(middle).focus(slot, window, cx);
    }
    #[cfg(feature = "gui-e2e")]
    fn focused(&self, middle: bool, window: &Window) -> Option<usize> {
        self.owner(middle).focused(window)
    }
}

/// Include the fixed ring border in native hit-target geometry, rather than
/// measuring only the bordered cell's content box.
fn segment_target(middle: bool, slot: usize, cell: gpui::Stateful<gpui::Div>) -> AnyElement {
    div()
        .flex_1()
        .relative()
        .child(super::e2e::measure_inside(match (middle, slot) {
            (false, 0) => "editor-right-selector-0",
            (false, _) => "editor-right-selector-1",
            (true, 0) => "editor-middle-selector-0",
            (true, _) => "editor-middle-selector-1",
        }))
        .child(cell)
        .into_any_element()
}
