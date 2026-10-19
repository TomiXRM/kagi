//! Keyboard ownership for the existing PDF viewport, not its toolbar or file tree.
use crate::EditorWorkspaceView;
use gpui::{actions, prelude::*, App, Div, FocusHandle, KeyBinding, ScrollHandle, Stateful, Window};
use kagi_ui_core::{i18n::Msg, keyboard_nav::with_ring, theme::scaled_px};

const CONTEXT: &str = "KagiPdfBody";
actions!(
    kagi_pdf,
    [PdfUp, PdfDown, PdfLeft, PdfRight, PdfPageUp, PdfPageDown, PdfTop, PdfBottom]
);

/// Register after the host's fallback file/graph navigation bindings.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", PdfUp, Some(CONTEXT)),
        KeyBinding::new("down", PdfDown, Some(CONTEXT)),
        KeyBinding::new("left", PdfLeft, Some(CONTEXT)),
        KeyBinding::new("right", PdfRight, Some(CONTEXT)),
        KeyBinding::new("pageup", PdfPageUp, Some(CONTEXT)),
        KeyBinding::new("pagedown", PdfPageDown, Some(CONTEXT)),
        KeyBinding::new("space", PdfPageDown, Some(CONTEXT)),
        KeyBinding::new("shift-space", PdfPageUp, Some(CONTEXT)),
        KeyBinding::new("home", PdfTop, Some(CONTEXT)),
        KeyBinding::new("end", PdfBottom, Some(CONTEXT)),
        KeyBinding::new("cmd-up", PdfTop, Some(CONTEXT)),
        KeyBinding::new("cmd-down", PdfBottom, Some(CONTEXT)),
    ]);
}

impl EditorWorkspaceView {
    fn pdf_body_visible(&self) -> bool {
        self.pdf_surface_visible() && self.open_path.is_some() && !self.content_missing
    }

    /// Retire only body-owned focus, including when the host hides this Editor.
    /// A modal, toolbar control or another pane keeps its independently owned focus.
    pub fn release_hidden_pdf_focus(&self, visible: bool, window: &mut Window, cx: &mut App) {
        if (!visible || !self.pdf_body_visible())
            && self.pdf_body_focus.as_ref().is_some_and(|focus| focus.is_focused(window))
        {
            self.root_focus.focus(window, cx);
        }
    }

    pub(super) fn prepare_pdf_focus(&mut self, window: &mut Window, cx: &mut App) {
        self.release_hidden_pdf_focus(true, window, cx);
        if self.pdf_body_visible() {
            self.pdf_body_focus.get_or_insert_with(|| cx.focus_handle());
        } else {
            self.pdf_body_focus = None;
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    Vertical(f32),
    Horizontal(f32),
    Page(f32),
    Top,
    Bottom,
}

fn scroll_action<A: gpui::Action>(
    scroll: &ScrollHandle,
    step: Step,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let scroll = scroll.clone();
    move |_, window, cx| {
        let old = scroll.offset();
        let mut offset = old;
        // Match the existing read-only inspection body's line increment. A
        // viewport step retains one line of overlap for reading continuity.
        let line = scaled_px(31.);
        let page = (scroll.bounds().size.height - line).max(line);
        let max = scroll.max_offset();
        match step {
            Step::Vertical(direction) => offset.y += line * direction,
            Step::Horizontal(direction) => offset.x += line * direction,
            Step::Page(direction) => offset.y += page * direction,
            Step::Top => offset.y = gpui::px(0.),
            Step::Bottom => offset.y = -max.y,
        }
        offset.x = offset.x.clamp(-max.x, gpui::px(0.));
        offset.y = offset.y.clamp(-max.y, gpui::px(0.));
        if offset != old {
            scroll.set_offset(offset);
            window.refresh();
        }
        // Boundaries are still PDF navigation, never a fallback file/page move.
        cx.stop_propagation();
    }
}

pub(super) fn body(
    body: Stateful<Div>,
    scroll: &ScrollHandle,
    focus: &FocusHandle,
) -> Stateful<Div> {
    let pointer_focus = focus.clone();
    with_ring(
        body.key_context(CONTEXT)
            .role(gpui::Role::Group)
            .aria_label(Msg::EditorPdfReadOnly.t())
            .track_focus(&focus.clone().tab_index(0).tab_stop(true))
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                pointer_focus.focus(window, cx);
            })
            .on_action(scroll_action::<PdfUp>(scroll, Step::Vertical(1.)))
            .on_action(scroll_action::<PdfDown>(scroll, Step::Vertical(-1.)))
            .on_action(scroll_action::<PdfLeft>(scroll, Step::Horizontal(1.)))
            .on_action(scroll_action::<PdfRight>(scroll, Step::Horizontal(-1.)))
            .on_action(scroll_action::<PdfPageUp>(scroll, Step::Page(1.)))
            .on_action(scroll_action::<PdfPageDown>(scroll, Step::Page(-1.)))
            .on_action(scroll_action::<PdfTop>(scroll, Step::Top))
            .on_action(scroll_action::<PdfBottom>(scroll, Step::Bottom)),
    )
}
