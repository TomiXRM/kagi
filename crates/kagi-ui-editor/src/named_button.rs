//! Stateful AX naming forwarded to the pinned Button interaction owner.
use gpui_component::button::Button;

#[derive(gpui::IntoElement)]
pub(crate) struct NamedButton(pub(crate) Button);
impl gpui::InteractiveElement for NamedButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}
impl gpui::StatefulInteractiveElement for NamedButton {}
impl gpui::RenderOnce for NamedButton {
    fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl gpui::IntoElement {
        self.0.render(window, cx)
    }
}
