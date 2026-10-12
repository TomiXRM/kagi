//! The shared diff body owns selection keys; hunk Buttons remain separate Tab stops.
use super::super::diff_view::DiffRow;
use gpui::{
    prelude::*, App, Div, IntoElement, KeyBinding, ListState, RenderOnce, Stateful, Window,
};
use std::sync::Arc;

const CONTEXT: &str = "KagiDiffSelection";
const SPLIT_CONTEXT: &str = "KagiSplitDiffSelection";

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = kagi_diff, no_json)]
pub(crate) struct SelectRows {
    pub(super) step: i8,
    pub(super) extend: bool,
    pub(super) boundary: bool,
    pub(super) all: bool,
    page: bool,
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
                page: false,
            },
            Some(CONTEXT),
        )]);
    }
    for (key, step, extend, boundary, page) in [
        ("home", -1, false, true, false),
        ("end", 1, false, true, false),
        ("shift-home", -1, true, true, false),
        ("shift-end", 1, true, true, false),
        ("pageup", -1, false, false, true),
        ("pagedown", 1, false, false, true),
        ("shift-pageup", -1, true, false, true),
        ("shift-pagedown", 1, true, false, true),
    ] {
        cx.bind_keys([KeyBinding::new(
            key,
            SelectRows {
                step,
                extend,
                boundary,
                all: false,
                page,
            },
            Some(SPLIT_CONTEXT),
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
    pub projection: Option<Arc<super::super::diff_split::cache::SplitProjection>>,
    pub items: Option<std::rc::Rc<Vec<super::super::render_helpers::row_overlay::Item>>>,
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
        let ring_visible = is_focused && window.last_input_was_keyboard();
        #[cfg(feature = "gui-e2e")]
        super::super::e2e::diff_controls::note_body_focus(self.key, is_focused, ring_visible);
        self.element
            .track_focus(&focus)
            .relative()
            // An inset overlay uses the existing accent/2px ring without
            // changing the list viewport or dense hunk-row geometry.
            .when(ring_visible, |pane| {
                pane.child(
                    gpui::div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(gpui::rgb(super::super::theme::theme().ui_accent())),
                )
            })
            .key_context(if self.projection.is_some() {
                "KagiDiffSelection KagiSplitDiffSelection"
            } else {
                CONTEXT
            })
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                window.focus(&focus, cx);
            })
            .on_action(move |action: &SelectRows, window, cx| {
                // Descendant controls, especially hunk Buttons, own their keys.
                if !key_focus.is_focused(window) {
                    cx.propagate();
                    return;
                }
                let cursor = if let Some(projection) = &self.projection {
                    let distance = if action.page {
                        page_distance(&self.scroll, self.items.as_deref().map(Vec::as_slice))
                    } else {
                        1
                    };
                    super::move_split_rows(self.key, &self.rows, &projection.rows, action, distance)
                } else {
                    super::move_rows(
                        self.key,
                        &self.rows,
                        action.step,
                        action.extend,
                        action.boundary,
                        action.all,
                    )
                };
                if let Some(cursor) = cursor {
                    super::super::keyboard_nav::reveal_list_row(
                        &self.scroll,
                        (self.item_index)(cursor),
                    );
                    window.refresh();
                }
            })
    }
}

/// Count actual fully visible aligned rows; expansion items consume height,
/// but must never become keyboard caret stops.
fn page_distance(
    scroll: &ListState,
    items: Option<&[super::super::render_helpers::row_overlay::Item]>,
) -> usize {
    use super::super::render_helpers::row_overlay::Item;
    let top = scroll.logical_scroll_top().item_ix;
    let bottom = scroll.viewport_bounds().bottom();
    let mut visible = 0usize;
    for item in top..scroll.item_count() {
        let Some(bounds) = scroll.bounds_for_item(item) else {
            break;
        };
        if bounds.bottom() > bottom {
            break;
        }
        if items.is_none_or(|items| matches!(items.get(item), Some(Item::Row(_)))) {
            visible += 1;
        }
    }
    visible.saturating_sub(1).max(1)
}
