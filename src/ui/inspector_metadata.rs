//! Inspector's named gpui-kit Button boundary (#1237).
//!
//! Button has a stable ID and native keyboard handling, but the pinned kit
//! exposes only InteractiveElement. Forward its stateful aria methods without
//! adding a second element/focus target or replacing its keyboard click path.
use gpui::prelude::*;
use gpui::{px, App, ElementId, IntoElement, RenderOnce, ScrollHandle, SharedString, Window};
use gpui_component::button::Button;
use gpui_component::ElementExt as _;
use std::cell::Cell;

#[derive(IntoElement)]
pub(super) struct MetadataButton(Button, bool, Option<(ElementId, ScrollHandle)>);
impl InteractiveElement for MetadataButton {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}
impl StatefulInteractiveElement for MetadataButton {}
impl MetadataButton {
    /// The kit owns focus; reveal that same control inside the message scroller.
    pub(super) fn reveal_on_focus(
        mut self,
        id: impl Into<ElementId>,
        scroll: ScrollHandle,
    ) -> Self {
        self.2 = Some((id.into(), scroll));
        self
    }
}
impl RenderOnce for MetadataButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        #[cfg(not(feature = "gui-e2e"))]
        let _ = self.1;
        #[cfg(feature = "gui-e2e")]
        if self.1 {
            let focus = window
                .use_keyed_state("inspector-hash-chip", cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            super::e2e::inspector_metadata::record_focus(focus);
        }
        let mut button = self.0;
        if let Some((id, scroll)) = self.2 {
            let focus = window
                .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone();
            let revealed = window.with_element_namespace(id, |window| {
                window.use_keyed_state("message-link-revealed", cx, |_, _| Cell::new(false))
            });
            button = button.on_prepaint(move |bounds, window, cx| {
                if !focus.is_focused(window) {
                    revealed.read(cx).set(false);
                    return;
                }
                if revealed.read(cx).get() {
                    return;
                }
                let viewport = scroll.bounds();
                // A control taller than the viewport can only align its top.
                // Do not alternate top/bottom corrections forever.
                let delta = if bounds.size.height > viewport.size.height - px(4.)
                    || bounds.top() < viewport.top() + px(2.)
                {
                    viewport.top() + px(2.) - bounds.top()
                } else if bounds.bottom() > viewport.bottom() - px(2.) {
                    viewport.bottom() - px(2.) - bounds.bottom()
                } else {
                    revealed.read(cx).set(true);
                    return;
                };
                let old = scroll.offset();
                let mut offset = old;
                offset.y = (old.y + delta).clamp(-scroll.max_offset().y, px(0.));
                if offset != old {
                    // Apply after parent prepaint releases its scroll borrow.
                    window.defer(cx, move |window, _| {
                        scroll.set_offset(offset);
                        window.refresh();
                    });
                } else {
                    // Clamping can prevent full visibility; stop retrying until
                    // focus changes, and leave subsequent wheel/thumb input alone.
                    revealed.read(cx).set(true);
                }
            });
        }
        button.render(window, cx)
    }
}
pub(super) fn named_hash_button(
    button: Button,
    name: SharedString,
    description: SharedString,
) -> MetadataButton {
    #[cfg(feature = "gui-e2e")]
    super::e2e::inspector_metadata::record_hash(name.clone());
    named_button(button, gpui::Role::Button, name, description, true)
}

pub(super) fn named_button(
    button: Button,
    role: gpui::Role,
    name: SharedString,
    description: SharedString,
    hash: bool,
) -> MetadataButton {
    MetadataButton(button, hash, None)
        .role(role)
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
