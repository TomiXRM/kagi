//! Read-only keyboard ownership for one restore card; never owns approval.
use crate::ui::{keyboard_nav::RowList, list_keyboard::ListKeyboard};
use gpui::{actions, App, FocusHandle, KeyBinding, ListState, ScrollHandle, Window};
use std::{cell::Cell, rc::Rc};

actions!(
    kagi_restore_body,
    [
        RestoreBodyPrev,
        RestoreBodyNext,
        RestoreBodyFirst,
        RestoreBodyLast,
        RestoreBodyPageUp,
        RestoreBodyPageDown
    ]
);
const BODY_CONTEXT: &str = "KagiRestoreBody";

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", RestoreBodyPrev, Some(BODY_CONTEXT)),
        KeyBinding::new("down", RestoreBodyNext, Some(BODY_CONTEXT)),
        KeyBinding::new("home", RestoreBodyFirst, Some(BODY_CONTEXT)),
        KeyBinding::new("end", RestoreBodyLast, Some(BODY_CONTEXT)),
        KeyBinding::new("pageup", RestoreBodyPageUp, Some(BODY_CONTEXT)),
        KeyBinding::new("pagedown", RestoreBodyPageDown, Some(BODY_CONTEXT)),
    ]);
}

#[derive(Clone, Copy)]
enum BodyStep {
    Line(f32),
    Page(f32),
    First,
    Last,
}

fn body_action<A: gpui::Action>(
    scroll: &ScrollHandle,
    step: BodyStep,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let scroll = scroll.clone();
    move |_, window, cx| {
        let mut offset = scroll.offset();
        match step {
            BodyStep::Line(direction) => offset.y += crate::ui::theme::scaled_px(31.) * direction,
            BodyStep::Page(direction) => offset.y += scroll.bounds().size.height * direction,
            BodyStep::First => offset.y = gpui::px(0.),
            BodyStep::Last => offset.y = -scroll.max_offset().y,
        }
        offset.y = offset.y.clamp(-scroll.max_offset().y, gpui::px(0.));
        scroll.set_offset(offset);
        window.refresh();
        cx.stop_propagation();
    }
}

pub(crate) struct InspectionList {
    keyboard: ListKeyboard,
    pub(crate) scroll: ListState,
    row_height: f32,
    revealed_entry: Cell<Option<usize>>,
}
impl InspectionList {
    pub(crate) fn new(count: usize, row_height: f32) -> Self {
        Self {
            keyboard: Default::default(),
            scroll: ListState::new(count, gpui::ListAlignment::Top, gpui::px(0.)),
            row_height,
            revealed_entry: Cell::new(None),
        }
    }
    pub(crate) fn rows<'a>(
        &self,
        keys: impl ExactSizeIterator<Item = (&'a str, usize)> + Clone,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> Rc<RowList> {
        self.scroll
            .clone()
            .with_uniform_item_height(crate::ui::theme::scaled_px(self.row_height));
        self.keyboard.rows(keys, &self.scroll, fallback, window, cx)
    }
    pub(crate) fn wheel_handler(
        &self,
    ) -> impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static {
        let scroll = self.scroll.clone();
        let previous = Cell::new(scroll.scroll_px_offset_for_scrollbar().y);
        move |event, _, cx| {
            let max = scroll.max_offset_for_scrollbar().y;
            let offset = scroll
                .scroll_px_offset_for_scrollbar()
                .y
                .clamp(-max, gpui::px(0.));
            // The inner List already handled Bubble. Use its prior offset so
            // the event reaching an edge does not also move the parent.
            // Pixels uses total ordering (-0 < 0); numerically, both mean top.
            let before = previous.replace(offset).clamp(-max, gpui::px(0.)).as_f32();
            let delta = event.delta.pixel_delta(gpui::px(20.)).y.as_f32();
            let max = max.as_f32();
            if max > 0. && ((delta < 0. && before > -max) || (delta > 0. && before < 0.)) {
                cx.stop_propagation();
            }
        }
    }
    pub(crate) fn reveal_parent(&self, parent: &ScrollHandle, window: &Window) {
        let Some(index) = self.keyboard.focused_entry(window) else {
            self.revealed_entry.set(None);
            return;
        };
        if self.revealed_entry.get() == Some(index) {
            return;
        }
        let Some(row) = self.scroll.bounds_for_item(index) else {
            return;
        };
        // RowList updates its scroll before the following layout. Keep this
        // reveal pending until that layout places the row in its inner pane.
        let inner = self.scroll.viewport_bounds();
        if row.top() < inner.top() - gpui::px(1.) || row.bottom() > inner.bottom() + gpui::px(1.) {
            return;
        }
        let viewport = parent.bounds();
        let delta = if row.top() < viewport.top() {
            viewport.top() - row.top()
        } else if row.bottom() > viewport.bottom() {
            viewport.bottom() - row.bottom()
        } else {
            self.revealed_entry.set(Some(index));
            return;
        };
        let mut offset = parent.offset();
        offset.y += delta;
        parent.set_offset(offset);
    }
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<String> {
        self.keyboard.focused(window)
    }
}

pub(crate) struct RestoreInspection {
    pub(crate) refs: InspectionList,
    pub(crate) preview: InspectionList,
    pub(crate) body_scroll: gpui::ScrollHandle,
    pub(crate) body_focus: FocusHandle,
}
impl RestoreInspection {
    pub(crate) fn new(refs: usize, preview: usize, cx: &App) -> Self {
        Self {
            refs: InspectionList::new(refs, 31.),
            preview: InspectionList::new(preview, crate::ui::graph_view::ROW_H),
            body_scroll: Default::default(),
            body_focus: cx.focus_handle(),
        }
    }
}

impl RestoreInspection {
    pub(crate) fn body(&self, label: &'static str) -> gpui::Stateful<gpui::Div> {
        use gpui::prelude::*;
        use BodyStep::{First, Last, Line, Page};
        crate::ui::keyboard_nav::with_ring(
            crate::ui::modal_shell::modal_scroll_body()
                .relative()
                .role(gpui::Role::Group)
                .aria_label(label)
                .key_context(BODY_CONTEXT)
                .track_focus(&self.body_focus.clone().tab_index(0).tab_stop(true))
                .track_scroll(&self.body_scroll)
                .on_action(body_action::<RestoreBodyPrev>(&self.body_scroll, Line(1.)))
                .on_action(body_action::<RestoreBodyNext>(&self.body_scroll, Line(-1.)))
                .on_action(body_action::<RestoreBodyFirst>(&self.body_scroll, First))
                .on_action(body_action::<RestoreBodyLast>(&self.body_scroll, Last))
                .on_action(body_action::<RestoreBodyPageUp>(
                    &self.body_scroll,
                    Page(1.),
                ))
                .on_action(body_action::<RestoreBodyPageDown>(
                    &self.body_scroll,
                    Page(-1.),
                )),
        )
    }
}

#[cfg(feature = "gui-e2e")]
impl crate::ui::KagiApp {
    pub fn oplog_restore_scroll_for_e2e(&self, list: &str) -> Option<ListState> {
        let modal = self.oplog_restore_modal()?;
        match list {
            "restore-refs" => Some(modal.inspection.refs.scroll.clone()),
            "restore-preview-rows" => Some(modal.inspection.preview.scroll.clone()),
            _ => None,
        }
    }
    pub fn oplog_restore_body_scroll_for_e2e(&self) -> Option<gpui::ScrollHandle> {
        self.oplog_restore_modal()
            .map(|modal| modal.inspection.body_scroll.clone())
    }
    pub fn oplog_restore_body_focused_for_e2e(&self, window: &Window) -> bool {
        self.oplog_restore_modal()
            .is_some_and(|modal| modal.inspection.body_focus.is_focused(window))
    }
}
