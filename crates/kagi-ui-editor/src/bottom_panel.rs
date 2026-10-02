//! Host bottom-panel seam for the Editor workspace (PR #919 review).
//!
//! The panel belongs under the center code viewer only, but it is built by
//! `KagiApp` (its listeners need `Context<KagiApp>`) while the tree, center,
//! and hunks panes all render inside this one entity. [`EditorWorkspaceElement`]
//! carries the host's element into the entity's own render without re-entering
//! `KagiApp`: during layout it hands the panel to the entity and immediately
//! lays out the ordinary (uncached) entity view, whose `render` takes it in the
//! same call. Every frame redraws from the root, so the host supplies a fresh
//! panel each time; nothing is retained across frames.

use gpui::{
    AnyElement, App, Bounds, ElementId, Entity, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

use crate::EditorWorkspaceView;

/// Embeds an [`EditorWorkspaceView`] with the host's bottom panel laid out
/// beneath its center pane. Use in place of embedding the entity directly.
pub struct EditorWorkspaceElement {
    view: Entity<EditorWorkspaceView>,
    bottom_panel: Option<AnyElement>,
}

impl EditorWorkspaceElement {
    pub fn new(view: Entity<EditorWorkspaceView>, bottom_panel: Option<AnyElement>) -> Self {
        Self { view, bottom_panel }
    }
}

impl IntoElement for EditorWorkspaceElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for EditorWorkspaceElement {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let bottom_panel = self.bottom_panel.take();
        self.view
            .update(cx, |view, _| view.bottom_panel = bottom_panel);
        let mut element = self.view.clone().into_any_element();
        let layout_id = element.request_layout(window, cx);
        // `render` normally took it; never let a panel outlive this layout.
        self.view.update(cx, |view, _| view.bottom_panel = None);
        (layout_id, element)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        element: &mut AnyElement,
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}
