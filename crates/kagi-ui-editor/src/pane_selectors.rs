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
    #[cfg(feature = "gui-e2e")]
    fn probes(&self, middle: bool) -> Vec<(gpui::Role, String, bool, bool)>;
    #[cfg(feature = "gui-e2e")]
    fn focus(&self, middle: bool, slot: usize, window: &mut Window, cx: &mut App);
    #[cfg(feature = "gui-e2e")]
    fn focused(&self, middle: bool, window: &Window) -> Option<usize>;
}
