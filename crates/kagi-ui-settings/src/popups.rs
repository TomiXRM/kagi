//! Final-kit popup value presentation backed by the existing GPUI menu.
use super::kit_style::{ink, metrics, pt};
use super::*;
use gpui_component::{
    menu::{PopupMenu, PopupMenuItem},
    ElementExt as _, Icon, IconName, Sizable, Size,
};
use kagi_ui_core::tooltip::HoverTooltip as _;
use std::cell::Cell;

type Entry = (String, SharedString, bool, bool, SettingsIntent);
pub(super) struct SettingsPopup {
    id: &'static str,
    menu: Entity<PopupMenu>,
    entries: Rc<Vec<Entry>>,
    anchor: Rc<Cell<Bounds<Pixels>>>,
    _subscription: Subscription,
}

impl SettingsView {
    pub(super) fn close_obsolete_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let obsolete = self.popup.as_ref().is_some_and(|popup| {
            self.pane != Pane::SmartCommit
                || (popup.id == "sc-model-select"
                    && self.snapshot.smart_provider != SmartProvider::Ollama)
        });
        if obsolete {
            self.close_popup(window, cx);
        }
    }

    fn close_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(popup) = self.popup.take() {
            if popup.menu.focus_handle(cx).contains_focused(window, cx) {
                let id = if self.pane != Pane::SmartCommit {
                    format!("settings-pane-{}", self.pane.id())
                } else if popup.id == "sc-model-select"
                    && (self.snapshot.models.is_empty()
                        || !self.snapshot.smart_ollama_available
                        || self.snapshot.smart_detecting
                        || self.snapshot.smart_provider != SmartProvider::Ollama
                        || self
                            .snapshot
                            .smart_model
                            .as_ref()
                            .is_some_and(|model| !self.snapshot.models.contains(model)))
                {
                    "sc-provider-select".into()
                } else {
                    popup.id.into()
                };
                self.handle(&id, cx).focus(window, cx);
            }
            cx.notify();
        }
    }

    fn popup_intent_available(&self, intent: &SettingsIntent) -> bool {
        match intent {
            SettingsIntent::SmartProvider(provider) => self
                .snapshot
                .providers
                .iter()
                .any(|entry| entry.provider == *provider && entry.available),
            SettingsIntent::SmartModel(model) => {
                self.snapshot.smart_provider == SmartProvider::Ollama
                    && self.snapshot.smart_ollama_available
                    && !self.snapshot.smart_detecting
                    && self.snapshot.models.contains(model)
            }
            _ => false,
        }
    }

    fn open_popup(
        &mut self,
        id: &'static str,
        entries: Rc<Vec<Entry>>,
        anchor: Rc<Cell<Bounds<Pixels>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.popup.as_ref().is_some_and(|popup| popup.id == id) {
            self.close_popup(window, cx);
            return;
        }
        self.close_popup(window, cx);
        let owner = cx.entity().downgrade();
        let menu = PopupMenu::build(window, cx, |mut menu, _, _| {
            for (item_id, label, selected, enabled, intent) in entries.iter() {
                let text_owner = owner.clone();
                let item_id = item_id.clone();
                let label = label.clone();
                let accessible_label = label.clone();
                let selected = *selected;
                let enabled = *enabled;
                let callback_owner = owner.clone();
                let intent = intent.clone();
                menu = menu.item(
                    PopupMenuItem::element(move |_, cx| {
                        div()
                            .id(SharedString::from(item_id.clone()))
                            .relative()
                            .min_w_0()
                            .w_full()
                            .role(Role::Label)
                            .aria_label(label.clone())
                            .text_size(pt(metrics::BODY))
                            .line_height(pt(20.))
                            .truncate()
                            .children(
                                cfg!(feature = "gui-e2e")
                                    .then(|| {
                                        text_owner.upgrade().map(|owner| {
                                            owner.read(cx).measured(
                                                item_id.clone(),
                                                Role::MenuItemRadio,
                                                label.to_string(),
                                                label.to_string(),
                                                false,
                                                selected,
                                                !enabled,
                                            )
                                        })
                                    })
                                    .flatten(),
                            )
                            .child(label.clone())
                            .hover_tooltip(
                                SharedString::from(format!("{item_id}-name")),
                                label.clone(),
                            )
                    })
                    .aria_label(accessible_label)
                    .radio_checked(selected)
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        if enabled {
                            let _ = callback_owner.update(cx, |this, cx| {
                                if this.popup_intent_available(&intent) {
                                    this.emit(intent.clone(), window, cx);
                                }
                            });
                        }
                    }),
                );
            }
            menu.scrollable(true).min_w(pt(120.)).max_w(pt(280.))
        });
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                this.close_popup(window, cx);
            });
        menu.focus_handle(cx).focus(window, cx);
        self.popup = Some(SettingsPopup {
            id,
            menu,
            entries,
            anchor,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn popup_control(
        &mut self,
        id: &'static str,
        title: Msg,
        entries: Vec<Entry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let stale = self.popup.as_ref().is_some_and(|popup| {
            popup.id == id
                && (popup.entries.len() != entries.len()
                    || popup.entries.iter().zip(&entries).any(|(old, new)| {
                        old.0 != new.0 || old.1 != new.1 || old.2 != new.2 || old.3 != new.3
                    }))
        });
        if stale {
            self.close_popup(window, cx);
        }
        let label = entries
            .iter()
            .find(|entry| entry.2)
            .or_else(|| entries.first())
            .map(|entry| entry.1.clone())
            .unwrap_or_else(|| Msg::SettingsSmartNoModels.t().into());
        let enabled = entries.iter().any(|entry| entry.3);
        let open = self.popup.as_ref().is_some_and(|popup| popup.id == id);
        let anchor = window
            .use_keyed_state(SharedString::from(format!("{id}-anchor")), cx, |_, _| {
                Rc::new(Cell::new(Bounds::default()))
            })
            .read(cx)
            .clone();
        let capture = anchor.clone();
        let click_anchor = anchor.clone();
        let entries = Rc::new(entries);
        let click_entries = entries.clone();
        let key_entries = entries;
        let width = (label
            .chars()
            .map(|ch| if ch.is_ascii() { 1. } else { 1.8 })
            .sum::<f32>()
            * 6.6
            + 38.)
            .clamp(100., 240.);
        let visual = div()
            .id(SharedString::from(format!("{id}-visual")))
            .relative()
            .h(pt(metrics::CONTROL_HEIGHT))
            .w_full()
            .min_w_0()
            .rounded(pt(metrics::CONTROL_RADIUS))
            .flex()
            .items_center()
            .bg(rgb(if open {
                ink::control_pressed()
            } else {
                ink::control()
            }))
            .opacity(if enabled { 1. } else { ink::disabled_opacity() })
            .when(enabled && !open, |el| {
                el.group_hover(id, |s| s.bg(rgb(ink::control_hover())))
                    .group_active(id, |s| s.bg(rgb(ink::control_pressed())))
            })
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.child(self.measured(
                    format!("{id}-visual"),
                    Role::Image,
                    label.to_string(),
                    String::new(),
                    false,
                    false,
                    !enabled,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(pt(8.))
                    .truncate()
                    .child(label.clone()),
            )
            .child(
                div()
                    .w(pt(18.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::ChevronsUpDown).with_size(Size::Size(pt(12.)))),
            );
        self.control(
            id.into(),
            format!("{}: {}", title.t(), label).into(),
            false,
            enabled,
            Role::ComboBox,
            ink::card(),
            px(0.),
            10.,
            None,
            None,
            window,
            cx,
        )
        .aria_label(title.t())
        .aria_value(label.clone())
        .aria_expanded(open)
        .w(pt(width))
        .max_w_full()
        .h(pt(metrics::CONTROL_HIT_HEIGHT))
        .flex()
        .items_center()
        .child(visual)
        .on_prepaint(move |bounds, _, _| capture.set(bounds))
        .on_click(cx.listener(move |this, _, window, cx| {
            if enabled {
                this.open_popup(id, click_entries.clone(), click_anchor.clone(), window, cx);
            }
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if enabled && event.keystroke.key == "down" && this.handle(id, cx).is_focused(window) {
                this.open_popup(id, key_entries.clone(), anchor.clone(), window, cx);
                cx.stop_propagation();
            }
        }))
        .hover_tooltip(SharedString::from(format!("{id}-value")), label)
        .into_any_element()
    }

    pub(super) fn popup_element(&self) -> Option<AnyElement> {
        let popup = self.popup.as_ref()?;
        let content =
            div()
                .relative()
                .child(popup.menu.clone())
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(self.measured(
                        "settings-popup-menu".into(),
                        Role::Menu,
                        String::new(),
                        String::new(),
                        false,
                        false,
                        false,
                    ))
                });
        Some(
            deferred(
                anchored()
                    .position(popup.anchor.get().bottom_right())
                    .anchor(Anchor::TopRight)
                    .snap_to_window_with_margin(px(8.))
                    .child(content),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
