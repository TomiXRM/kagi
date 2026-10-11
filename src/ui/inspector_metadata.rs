//! Inspector's named gpui-kit Button boundary (#1237).
//!
//! Button has a stable ID and native keyboard handling, but the pinned kit
//! exposes only InteractiveElement. Forward its stateful aria methods without
//! adding a second element/focus target or replacing its keyboard click path.
use gpui::prelude::*;
use gpui::{App, IntoElement, RenderOnce, SharedString, Window};
use gpui_component::button::Button;

#[derive(IntoElement)]
pub(super) struct MetadataButton(Button);
impl InteractiveElement for MetadataButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}
impl StatefulInteractiveElement for MetadataButton {}
impl RenderOnce for MetadataButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(feature = "gui-e2e")]
        {
            let focus = window
                .use_keyed_state("inspector-hash-chip", cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            super::e2e::inspector_metadata::record_focus(focus);
        }
        self.0.render(window, cx)
    }
}
pub(super) fn named_hash_button(
    button: Button,
    name: SharedString,
    description: SharedString,
) -> MetadataButton {
    #[cfg(feature = "gui-e2e")]
    super::e2e::inspector_metadata::record_hash(name.clone());
    MetadataButton(button)
        .role(gpui::Role::Button)
        .aria_label(name)
        .aria_description(description)
}

pub(super) fn identity(name: &str, email: &str) -> SharedString {
    match (name.is_empty(), email.is_empty()) {
        (false, false) => format!("{name} <{email}>").into(),
        (true, _) => email.to_owned().into(),
        (_, true) => name.to_owned().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::identity;

    #[test]
    fn incomplete_trailer_identities_do_not_add_empty_or_duplicate_email() {
        let trailers = kagi_git::parse_coauthors(
            "Co-authored-by: Alice\nCo-authored-by: <a@b>\nCo-authored-by: Bob <b@c>",
        );
        let identities: Vec<_> = trailers
            .iter()
            .map(|author| identity(&author.name, &author.email))
            .collect();
        assert_eq!(identities, vec!["Alice", "a@b", "Bob <b@c>"]);
        assert_eq!(identity("", ""), "");
    }
}
