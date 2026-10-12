//! Variable-height palette rows remain scroll targets beneath semantic groups.
use std::{cell::RefCell, rc::Rc};

use gpui::{
    prelude::*, AnyElement, App, Bounds, ElementId, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, Point, ScrollHandle, Window,
};

#[derive(Clone, Default)]
pub struct PaletteScroll {
    handle: ScrollHandle,
    state: Rc<RefCell<RowScrollState>>,
}

#[derive(Default)]
struct RowScrollState {
    requested: Option<usize>,
    destination: Option<LayoutId>,
    section_top: Option<LayoutId>,
    #[cfg(feature = "gui-e2e")]
    layouts: Vec<Option<LayoutId>>,
    #[cfg(feature = "gui-e2e")]
    rows: Vec<Bounds<Pixels>>,
}

impl PaletteScroll {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn offset(&self) -> Point<Pixels> {
        self.handle.offset()
    }
    pub fn set_offset(&self, offset: Point<Pixels>) {
        self.handle.set_offset(offset);
    }
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.handle.bounds()
    }
    #[cfg(feature = "gui-e2e")]
    pub fn bounds_for_item(&self, index: usize) -> Option<Bounds<Pixels>> {
        self.state.borrow().rows.get(index).copied()
    }
    #[cfg(feature = "gui-e2e")]
    pub fn children_count(&self) -> usize {
        self.state.borrow().rows.len()
    }
    pub fn scroll_to_item(&self, index: usize) {
        self.state.borrow_mut().requested = Some(index);
    }
    pub(super) fn handle(&self) -> &ScrollHandle {
        &self.handle
    }

    pub(super) fn measure_row(&self, index: usize, row: AnyElement) -> AnyElement {
        if !cfg!(feature = "gui-e2e") && self.state.borrow().requested != Some(index) {
            return row;
        }
        PaletteRowLayout {
            row,
            index,
            scroll: self.clone(),
            group: false,
        }
        .into_any_element()
    }

    pub(super) fn measure_group(&self, first: usize, group: AnyElement) -> AnyElement {
        if self.state.borrow().requested != Some(first) {
            return group;
        }
        PaletteRowLayout {
            row: group,
            index: first,
            scroll: self.clone(),
            group: true,
        }
        .into_any_element()
    }

    pub(super) fn wrap(&self, list: AnyElement, count: usize) -> AnyElement {
        let mut state = self.state.borrow_mut();
        state.destination = None;
        state.section_top = None;
        #[cfg(feature = "gui-e2e")]
        {
            state.layouts.resize(count, None);
            state.rows.resize(count, Bounds::default());
        }
        if count == 0 {
            state.requested.take();
        }
        drop(state);
        PaletteViewport {
            list,
            scroll: self.clone(),
        }
        .into_any_element()
    }

    fn reveal(&self, viewport: Bounds<Pixels>, window: &mut Window) {
        let mut state = self.state.borrow_mut();
        #[cfg(feature = "gui-e2e")]
        for index in 0..state.layouts.len() {
            if let Some(layout) = state.layouts[index] {
                state.rows[index] = window.layout_bounds(layout);
            }
        }
        let Some(index) = state.requested.take() else {
            return;
        };
        let Some(layout) = state.destination.take() else {
            return;
        };
        let section_top = state.section_top.take();
        drop(state);
        let row = window.layout_bounds(layout);
        let mut offset = self.offset();
        let top = section_top
            .map(|layout| window.layout_bounds(layout).top())
            .unwrap_or(row.top());
        if index == 0 {
            offset.y = gpui::px(0.);
        } else if row.size.height > viewport.size.height || top + offset.y < viewport.top() {
            offset.y = viewport.top() - top;
        } else if row.bottom() + offset.y > viewport.bottom() {
            offset.y = viewport.bottom() - row.bottom();
        }
        self.set_offset(offset);
    }
}

/// Capture the actual leaf layout ID, without adding layout or AX containers.
struct PaletteRowLayout {
    row: AnyElement,
    index: usize,
    scroll: PaletteScroll,
    group: bool,
}

impl IntoElement for PaletteRowLayout {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PaletteRowLayout {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout = self.row.request_layout(window, cx);
        let mut state = self.scroll.state.borrow_mut();
        if state.requested == Some(self.index) {
            if self.group {
                state.section_top = Some(layout);
            } else {
                state.destination = Some(layout);
            }
        }
        #[cfg(feature = "gui-e2e")]
        if !self.group {
            state.layouts[self.index] = Some(layout);
        }
        (layout, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.row.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.row.paint(window, cx);
    }
}

/// Reveal the measured leaf before the scroll container applies its offset.
struct PaletteViewport {
    list: AnyElement,
    scroll: PaletteScroll,
}

impl IntoElement for PaletteViewport {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PaletteViewport {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.list.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.scroll.reveal(bounds, window);
        self.list.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.list.paint(window, cx);
    }
}
