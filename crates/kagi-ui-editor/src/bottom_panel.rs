//! Host content slots for the Editor workspace.
//!
//! KagiApp builds the bottom panel and notifications with its own listeners.
//! This element passes them into the ordinary uncached entity render during
//! layout, without re-entering the host. The panel sits below the center;
//! notifications sit inside its content, below the header/tabs/banner and
//! between the file tree and hunks. Neither slot is retained across frames.

use gpui::{
    AnyElement, App, Bounds, ElementId, Entity, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window,
};

use crate::EditorWorkspaceView;

/// Embeds the workspace with host-owned center content slots.
pub struct EditorWorkspaceElement {
    view: Entity<EditorWorkspaceView>,
    bottom_panel: Option<AnyElement>,
    notifications: Option<AnyElement>,
}

impl EditorWorkspaceElement {
    pub fn new(
        view: Entity<EditorWorkspaceView>,
        bottom_panel: Option<AnyElement>,
        notifications: Option<AnyElement>,
    ) -> Self {
        Self {
            view,
            bottom_panel,
            notifications,
        }
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
        let notifications = self.notifications.take();
        self.view.update(cx, |view, _| {
            view.bottom_panel = bottom_panel;
            view.notifications = notifications;
        });
        let mut element = self.view.clone().into_any_element();
        let layout_id = element.request_layout(window, cx);
        // `render` normally took it; never let a panel outlive this layout.
        self.view.update(cx, |view, _| {
            view.bottom_panel = None;
            view.notifications = None;
        });
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
