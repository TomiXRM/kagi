//! The shared diff body owns selection keys; hunk Buttons remain separate Tab stops.
use super::super::diff_view::DiffRow;
use gpui::{
    prelude::*, App, Div, IntoElement, KeyBinding, ListState, RenderOnce, Stateful, Window,
};
use std::sync::Arc;

const CONTEXT: &str = "KagiDiffSelection";

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_diff, no_json)]
pub(crate) struct SelectRows {
    step: i8,
    extend: bool,
    boundary: bool,
    all: bool,
}

pub(crate) fn bind_keys(cx: &mut App) {
    for (key, step, extend, boundary, all) in [
        ("up", -1, false, false, false),
        ("down", 1, false, false, false),
        ("shift-up", -1, true, false, false),
        ("shift-down", 1, true, false, false),
        ("shift-secondary-up", -1, true, true, false),
        ("shift-secondary-down", 1, true, true, false),
        ("secondary-a", 1, true, true, true),
    ] {
        cx.bind_keys([KeyBinding::new(
            key,
            SelectRows {
                step,
                extend,
                boundary,
                all,
            },
            Some(CONTEXT),
        )]);
    }
}

#[derive(IntoElement)]
pub(crate) struct Pane<F: Fn(usize) -> usize + 'static> {
    pub element: Stateful<Div>,
    pub key: u64,
    pub focus_key: u64,
    pub rows: Arc<Vec<DiffRow>>,
    pub scroll: ListState,
    pub item_index: F,
}

impl<F: Fn(usize) -> usize + 'static> RenderOnce for Pane<F> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(("diff-selection-focus", self.focus_key), cx, |_, cx| {
                cx.focus_handle()
            })
            .read(cx)
            .clone()
            .tab_index(0)
            .tab_stop(true);
        let key_focus = focus.clone();
        let is_focused = focus.is_focused(window);
        #[cfg(feature = "gui-e2e")]
        super::super::e2e::diff_controls::note_body_focus(self.key, is_focused);
        self.element
            .track_focus(&focus)
            .relative()
            // An inset overlay uses the existing accent/2px ring without
            // changing the list viewport or dense hunk-row geometry.
            .when(is_focused, |pane| {
                pane.child(
                    gpui::div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(gpui::rgb(super::super::theme::theme().ui_accent())),
                )
            })
            .key_context(CONTEXT)
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
            })
            .on_action(move |action: &SelectRows, window, cx| {
                // Descendant controls, especially hunk Buttons, own their keys.
                if !key_focus.is_focused(window) {
                    cx.propagate();
                    return;
                }
                if let Some(cursor) = super::move_rows(
                    self.key,
                    &self.rows,
                    action.step,
                    action.extend,
                    action.boundary,
                    action.all,
                ) {
                    super::super::keyboard_nav::reveal_list_row(
                        &self.scroll,
                        (self.item_index)(cursor),
                    );
                    window.refresh();
                }
            })
    }
}
