//! Host-owned keyboard navigation crossing the existing EditorHooks boundary.
use crate::EditorWorkspaceView;
use gpui::{AnyElement, App, Context, Window};

pub trait EditorPaneSelectors {
    fn render(
        &self,
        view: &EditorWorkspaceView,
        middle: bool,
        cx: &mut Context<EditorWorkspaceView>,
    ) -> AnyElement;
    fn release_hidden(&self, view: &EditorWorkspaceView, window: &mut Window, cx: &mut App);
    /// Host-native instrumentation without adding a layout wrapper to a pane.
    #[cfg(feature = "gui-e2e")]
    fn measure(&self, _id: &'static str) -> AnyElement {
        use gpui::{IntoElement as _, Styled as _};
        gpui::div().absolute().size_0().into_any_element()
    }
    #[cfg(feature = "gui-e2e")]
    fn measure_toggle(&self, button: AnyElement) -> AnyElement {
        button
    }
    #[cfg(feature = "gui-e2e")]
    fn probes(&self, middle: bool) -> Vec<(gpui::Role, String, bool, bool)>;
    #[cfg(feature = "gui-e2e")]
    fn focus(&self, middle: bool, slot: usize, window: &mut Window, cx: &mut App);
    #[cfg(feature = "gui-e2e")]
    fn focused(&self, middle: bool, window: &Window) -> Option<usize>;
}
