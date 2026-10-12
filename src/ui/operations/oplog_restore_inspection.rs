//! Read-only keyboard ownership for one restore card; never owns approval.
use crate::ui::{keyboard_nav::RowList, list_keyboard::ListKeyboard};
use gpui::{App, FocusHandle, ListState, Window};
use std::rc::Rc;

pub(crate) struct InspectionList {
    keyboard: ListKeyboard,
    pub(crate) scroll: ListState,
    row_height: f32,
}
impl InspectionList {
    pub(crate) fn new(count: usize, row_height: f32) -> Self {
        Self {
            keyboard: Default::default(),
            scroll: ListState::new(count, gpui::ListAlignment::Top, gpui::px(0.)),
            row_height,
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
    pub(crate) fn reveal_parent(&self, parent: &gpui::ScrollHandle, window: &Window) {
        let Some(index) = self.keyboard.focused_entry(window) else {
            return;
        };
        let Some(row) = self.scroll.bounds_for_item(index) else {
            return;
        };
        let viewport = parent.bounds();
        let delta = if row.top() < viewport.top() {
            viewport.top() - row.top()
        } else if row.bottom() > viewport.bottom() {
            viewport.bottom() - row.bottom()
        } else {
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
        let scroll = self.body_scroll.clone();
        let focus = self.body_focus.clone();
        crate::ui::keyboard_nav::with_ring(
            crate::ui::modal_shell::modal_scroll_body()
                .relative()
                .role(gpui::Role::Group)
                .aria_label(label)
                .track_focus(&self.body_focus.clone().tab_index(0).tab_stop(true))
                .track_scroll(&self.body_scroll)
                .on_key_down(move |event, window, cx| {
                    if !focus.is_focused(window) || event.keystroke.modifiers.modified() {
                        return;
                    }
                    let mut offset = scroll.offset();
                    let page = scroll.bounds().size.height;
                    match event.keystroke.key.as_str() {
                        "up" => offset.y += crate::ui::theme::scaled_px(31.),
                        "down" => offset.y -= crate::ui::theme::scaled_px(31.),
                        "pageup" => offset.y += page,
                        "pagedown" => offset.y -= page,
                        "home" => offset.y = gpui::px(0.),
                        "end" => offset.y = -scroll.max_offset().y,
                        _ => return,
                    }
                    offset.y = offset.y.clamp(-scroll.max_offset().y, gpui::px(0.));
                    scroll.set_offset(offset);
                    cx.stop_propagation();
                }),
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
    pub fn oplog_restore_body_focused_for_e2e(&self, window: &Window) -> bool {
        self.oplog_restore_modal()
            .is_some_and(|modal| modal.inspection.body_focus.is_focused(window))
    }
}
