//! Renderer-fed Editor identity/geometry evidence; absent from normal builds.
use gpui::{canvas, prelude::*, Bounds, Pixels, Role};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug)]
pub struct EditorTabProbe {
    pub path: String,
    pub label: String,
    pub full_path: String,
    pub tooltip: String,
    pub bounds: Bounds<Pixels>,
    pub role: Role,
    pub close_bounds: Bounds<Pixels>,
}

#[derive(Default)]
pub(crate) struct TabProbes(pub Rc<RefCell<Vec<EditorTabProbe>>>);

pub(crate) fn measure(probes: &TabProbes, index: usize) -> impl IntoElement {
    let probes = probes.0.clone();
    canvas(
        move |bounds, _, _| {
            if let Some(probe) = probes.borrow_mut().get_mut(index) {
                probe.bounds = bounds;
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub(crate) fn measure_close(probes: &TabProbes, index: usize) -> impl IntoElement {
    let probes = probes.0.clone();
    canvas(
        move |bounds, _, _| {
            if let Some(probe) = probes.borrow_mut().get_mut(index) {
                probe.close_bounds = bounds;
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl crate::EditorWorkspaceView {
    pub fn tab_probes_for_e2e(&self) -> Vec<EditorTabProbe> {
        self.tab_probes.0.borrow().clone()
    }
    pub fn tab_strip_bounds_for_e2e(&self) -> Option<Bounds<Pixels>> {
        Some(self.tab_scroll.bounds())
    }
    pub fn tab_scroll_offset_for_e2e(&self) -> gpui::Point<Pixels> {
        self.tab_scroll.offset()
    }
    pub fn selector_probes_for_e2e(&self, middle: bool) -> Vec<(Role, String, bool, bool)> {
        self.hooks.pane_selectors.probes(middle)
    }
    pub fn focus_selector_for_e2e(
        &self,
        middle: bool,
        slot: usize,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.hooks.pane_selectors.focus(middle, slot, window, cx);
    }
    pub fn selector_focused_for_e2e(&self, middle: bool, window: &gpui::Window) -> Option<usize> {
        self.hooks.pane_selectors.focused(middle, window)
    }
}
