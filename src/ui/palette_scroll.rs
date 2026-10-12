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
        }
        .into_any_element()
    }

    pub(super) fn wrap(&self, list: AnyElement, count: usize) -> AnyElement {
        let mut state = self.state.borrow_mut();
        state.destination = None;
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
        if state.requested.take().is_none() {
            return;
        }
        let Some(layout) = state.destination.take() else {
            return;
        };
        drop(state);
        let row = window.layout_bounds(layout);
        let mut offset = self.offset();
        if row.size.height > viewport.size.height || row.top() + offset.y < viewport.top() {
            offset.y = viewport.top() - row.top();
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
            state.destination = Some(layout);
        }
        #[cfg(feature = "gui-e2e")]
        {
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
