//! Read-only keyboard ownership for one restore card; never owns approval.
use crate::ui::{keyboard_nav::RowList, list_keyboard::ListKeyboard};
use gpui::{actions, App, FocusHandle, KeyBinding, ScrollHandle, UniformListScrollHandle, Window};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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
    pub(crate) scroll: UniformListScrollHandle,
    pub(crate) drawn: Rc<RefCell<std::ops::Range<usize>>>,
    count: Cell<usize>,
    wheel_start: Rc<Cell<gpui::Pixels>>,
    revealed_entry: Cell<Option<usize>>,
}
impl InspectionList {
    fn new() -> Self {
        Self {
            keyboard: Default::default(),
            scroll: UniformListScrollHandle::new(),
            drawn: Rc::new(RefCell::new(0..0)),
            count: Cell::new(0),
            wheel_start: Rc::new(Cell::new(gpui::px(0.))),
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
        self.count.set(keys.len());
        self.keyboard.rows(
            keys,
            &crate::ui::keyboard_nav::RowScroll::Uniform(
                self.scroll.clone(),
                self.drawn.borrow().clone(),
            ),
            fallback,
            window,
            cx,
        )
    }
    pub(crate) fn wheel_capture(&self) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        let scroll = self.scroll.0.borrow().base_handle.clone();
        let start = self.wheel_start.clone();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |_: &gpui::ScrollWheelEvent, phase, _, _| {
                    if phase.capture() {
                        start.set(scroll.offset().y);
                    }
                });
            },
        )
        .absolute()
        .size(gpui::px(0.))
    }
    pub(crate) fn wheel_handler(
        &self,
    ) -> impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static {
        let scroll = self.scroll.0.borrow().base_handle.clone();
        let previous = self.wheel_start.clone();
        move |event, _, cx| {
            let max = scroll.max_offset().y;
            // Capture reads the actual event-start offset, after deferred
            // keyboard reveal/layout; Bubble runs after the uniform list.
            let before = previous.get().clamp(-max, gpui::px(0.)).as_f32();
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
        if !self.drawn.borrow().contains(&index) {
            return;
        }
        let state = self.scroll.0.borrow();
        let Some(size) = state.last_item_size else {
            return;
        };
        let count = self.count.get();
        if count == 0 {
            return;
        }
        // UniformList uses its pixel-rounded measured row, not the unrounded
        // zoom token. Dividing its live content extent retains that geometry.
        let height = size.contents.height / count as f32;
        let scroll = &state.base_handle;
        let inner = scroll.bounds();
        let top = inner.top() + scroll.offset().y + height * index as f32;
        let row = gpui::Bounds::new(
            gpui::point(inner.left(), top),
            gpui::size(inner.size.width, height),
        );
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
    pub(crate) fn new(cx: &App) -> Self {
        Self {
            refs: InspectionList::new(),
            preview: InspectionList::new(),
            body_scroll: Default::default(),
            body_focus: cx.focus_handle(),
        }
    }
    pub(crate) fn owns_row_focus(&self, window: &Window) -> bool {
        self.refs.keyboard.focused_entry(window).is_some()
            || self.preview.keyboard.focused_entry(window).is_some()
    }
}

impl RestoreInspection {
    pub(crate) fn body(&self, label: &'static str) -> gpui::Stateful<gpui::Div> {
        read_only_scroll_body(&self.body_scroll, &self.body_focus, label)
    }
}

/// Shared read-only card navigation; only the focused body consumes these keys.
pub(crate) fn read_only_scroll_body(
    scroll: &ScrollHandle,
    focus: &FocusHandle,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    use gpui::prelude::*;
    use BodyStep::{First, Last, Line, Page};
    crate::ui::keyboard_nav::with_ring(
        crate::ui::modal_shell::modal_scroll_body()
            .relative()
            .role(gpui::Role::Group)
            .aria_label(label)
            .key_context(BODY_CONTEXT)
            .track_focus(&focus.clone().tab_index(0).tab_stop(true))
            .track_scroll(scroll)
            .map(|mut body| {
                body.style().restrict_scroll_to_axis = Some(true);
                body
            })
            .on_action(body_action::<RestoreBodyPrev>(scroll, Line(1.)))
            .on_action(body_action::<RestoreBodyNext>(scroll, Line(-1.)))
            .on_action(body_action::<RestoreBodyFirst>(scroll, First))
            .on_action(body_action::<RestoreBodyLast>(scroll, Last))
            .on_action(body_action::<RestoreBodyPageUp>(scroll, Page(1.)))
            .on_action(body_action::<RestoreBodyPageDown>(scroll, Page(-1.))),
    )
}

#[cfg(feature = "gui-e2e")]
impl crate::ui::KagiApp {
    pub fn oplog_restore_scroll_for_e2e(&self, list: &str) -> Option<UniformListScrollHandle> {
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
