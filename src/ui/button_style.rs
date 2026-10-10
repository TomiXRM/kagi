//! Shared button variants for Kagi's theme tokens.

use gpui::{div, prelude::*, rgb, ElementId, Hsla, Role, SharedString, Window};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};

use super::theme::theme;

/// A translucent, theme-tinted button variant for accent action buttons.
///
/// The gpui-component filled `success`/`warning`/`danger` variants use their own
/// foreground/hover tokens. Kagi syncs only part of that palette, so labels can
/// wash out in light themes. Build accent buttons from Kagi's palette instead.
pub fn tinted_action_variant(base: u32, cx: &gpui::App) -> ButtonCustomVariant {
    let c = Hsla::from(rgb(base));
    // gpui-component 0.5.2 derives the tint itself from `color`: bg =
    // color@20% (mix_oklab with transparent), hover ~30%, label/border =
    // full-strength color; the `foreground`/`hover` fields are no longer
    // read. Pass the base color at FULL alpha — the 0.5.1 recipe of
    // pre-multiplying it (0.16) made the bg ~3% and the label render at
    // 0.16 alpha (washed-out stage/unstage/discard, user-reported).
    let active = if theme().dark {
        c.opacity(0.34)
    } else {
        c.opacity(0.30)
    };
    ButtonCustomVariant::new(cx)
        .color(c)
        .foreground(c) // unused by 0.5.2; kept for forward/back compat
        .hover(c) // unused by 0.5.2 (hover derives from `color`)
        .active(active)
}

/// An operation's hierarchy is independent of its palette's RGB values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    Primary,
    SideCurrent,
    SideIncoming,
    Success,
    Warning,
    Danger,
    Neutral,
    NeutralTinted,
}

impl ButtonRole {
    fn accent(self, t: &super::theme::Theme) -> u32 {
        match self {
            Self::Primary | Self::SideCurrent => t.color_branch,
            Self::SideIncoming => t.color_remote,
            Self::Success => t.color_success,
            Self::Warning => t.color_warning,
            Self::Danger => t.color_blocker,
            Self::Neutral | Self::NeutralTinted => t.text_sub,
        }
    }

    fn filled_variant(self) -> Option<ButtonVariant> {
        match self {
            Self::Primary | Self::SideCurrent => Some(ButtonVariant::Primary),
            Self::SideIncoming => Some(ButtonVariant::Info),
            Self::Neutral => Some(ButtonVariant::Ghost),
            Self::Success | Self::Warning | Self::Danger | Self::NeutralTinted => None,
        }
    }
}

#[cfg(test)]
mod role_tests {
    use super::*;

    #[test]
    fn button_roles_do_not_collide_with_palette_colors() {
        for t in super::super::theme::THEMES {
            assert_eq!(
                ButtonRole::SideCurrent.filled_variant(),
                Some(ButtonVariant::Primary),
                "{}: Keep Current must be a filled Primary",
                t.slug,
            );
            assert_eq!(
                ButtonRole::SideIncoming.filled_variant(),
                Some(ButtonVariant::Info),
                "{}: Take Incoming must be a filled Info",
                t.slug,
            );
            assert_eq!(
                ButtonRole::Primary.filled_variant(),
                Some(ButtonVariant::Primary)
            );
            assert_eq!(ButtonRole::Success.filled_variant(), None);
            assert_eq!(ButtonRole::Warning.filled_variant(), None);
            assert_eq!(ButtonRole::Danger.filled_variant(), None);
            assert_eq!(
                ButtonRole::Neutral.filled_variant(),
                Some(ButtonVariant::Ghost)
            );
            assert_eq!(ButtonRole::NeutralTinted.filled_variant(), None);
            assert_eq!(ButtonRole::SideCurrent.accent(t), t.color_branch);
            assert_eq!(ButtonRole::SideIncoming.accent(t), t.color_remote);
        }
    }
}

/// Kagi-owned constructors for gpui-component buttons.
///
/// Use these for semantic action buttons instead of calling
/// `Button::success()` / `warning()` / `danger()` directly. Those filled
/// variants depend on gpui-component foreground/hover tokens that Kagi does not
/// fully own.
pub struct KagiButton;

impl KagiButton {
    pub fn styled(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        role: ButtonRole,
        cx: &gpui::App,
    ) -> Button {
        apply_role(Button::new(id).label(label), role, cx)
    }

    pub fn icon(
        id: impl Into<ElementId>,
        icon_path: &'static str,
        label: impl Into<SharedString>,
        role: ButtonRole,
        cx: &gpui::App,
    ) -> Button {
        apply_role(
            Button::new(id)
                .icon(gpui_component::Icon::empty().path(icon_path))
                .label(label),
            role,
            cx,
        )
    }
}

pub fn apply_role(btn: Button, role: ButtonRole, cx: &gpui::App) -> Button {
    match role.filled_variant() {
        Some(variant) => btn.with_variant(variant),
        None => btn.custom(tinted_action_variant(role.accent(&theme()), cx)),
    }
}

// Tier A observes the AX attributes set on both active and inert modal controls.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalButtonA11y {
    pub role: Role,
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static MODAL_BUTTONS: std::cell::RefCell<std::collections::HashMap<&'static str, ModalButtonA11y>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
pub fn clear_recorded_modal_buttons() {
    MODAL_BUTTONS.with(|buttons| buttons.borrow_mut().clear());
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_modal_button(id: &str) -> Option<ModalButtonA11y> {
    MODAL_BUTTONS.with(|buttons| buttons.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
fn record_modal_button(id: &'static str, label: &SharedString, reason: Option<&SharedString>) {
    MODAL_BUTTONS.with(|buttons| {
        buttons.borrow_mut().insert(
            id,
            ModalButtonA11y {
                role: Role::Button,
                label: label.to_string(),
                description: reason.map(ToString::to_string),
                disabled: reason.is_some(),
            },
        );
    });
}

#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_modal_button(_id: &'static str, _label: &SharedString, _reason: Option<&SharedString>) {}

/// Dialog actions share the small (24px) control, regardless of operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModalButtonKind {
    Cancel,
    Primary,
    Destructive,
    Secondary,
}

/// An unavailable action stays visible with its reason in the accessibility
/// tree. gpui-component's disabled button still reports an enabled AX node,
/// so this inert control is deliberately not a `Button`.
pub(crate) fn modal_button(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::AnyElement {
    modal_button_with_tab_stop(id, label, kind, reason, on_click, true, cx)
}

/// AppNotice keeps window/root focus for its Enter/Escape confirmation
/// sequence; unlike other dialog actions its button is not a Tab stop.
pub(crate) fn modal_button_without_tab_stop(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::AnyElement {
    modal_button_with_tab_stop(id, label, kind, reason, on_click, false, cx)
}

fn modal_button_with_tab_stop(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    tab_stop: bool,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let label = label.into();
    record_modal_button(id, &label, reason.as_ref());
    if let Some(reason) = reason {
        let accent = match kind {
            ModalButtonKind::Destructive => theme().color_blocker,
            ModalButtonKind::Primary => theme().color_branch,
            ModalButtonKind::Cancel | ModalButtonKind::Secondary => theme().text_sub,
        };
        return div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_description(reason.clone())
            .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                builder.parent_node().set_disabled();
            })
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .h_6()
            .px_3()
            .text_xs()
            .rounded(cx.theme().radius)
            .bg(Hsla::from(rgb(accent)).opacity(0.15))
            .text_color(cx.theme().muted_foreground.opacity(0.5))
            .child(label)
            .into_any_element();
    }
    let button = match kind {
        ModalButtonKind::Cancel => Button::new(id).label(label).ghost(),
        ModalButtonKind::Primary => KagiButton::styled(id, label, ButtonRole::Primary, cx),
        ModalButtonKind::Destructive => KagiButton::styled(id, label, ButtonRole::Danger, cx),
        ModalButtonKind::Secondary => Button::new(id).label(label),
    };
    button
        .small()
        .tab_stop(tab_stop)
        .on_click(on_click)
        .into_any_element()
}

/// An icon-only dialog action (#1043): the same small (24px) square as an
/// icon `Button`, named by `label` in its tooltip and for assistive
/// technology. gpui-component's `Button` names its AX node only from a
/// visible label, so the glyph-only control is drawn here — still a Tab stop
/// that Enter / Space press like a click.
pub(crate) fn modal_icon_button(
    id: &'static str,
    icon: gpui_component::IconName,
    label: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let label = label.into();
    record_modal_button(id, &label, None);
    ModalIconButton {
        id,
        icon,
        label,
        on_click: Box::new(on_click),
    }
    .into_any_element()
}

type ClickHandler = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App)>;

#[derive(IntoElement)]
struct ModalIconButton {
    id: &'static str,
    icon: gpui_component::IconName,
    label: SharedString,
    on_click: ClickHandler,
}

impl RenderOnce for ModalIconButton {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focused = focus.is_focused(window);
        let label = self.label;
        let on_click = self.on_click;
        let size = super::theme::scaled_px(24.);
        div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(label.clone())
            .track_focus(&focus)
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(size)
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(if focused {
                cx.theme().ring
            } else {
                gpui::transparent_black()
            })
            .text_color(cx.theme().muted_foreground)
            .hover(|style| style.bg(cx.theme().secondary_hover))
            .on_click(move |event, window, cx| on_click(event, window, cx))
            .child(gpui_component::Icon::new(self.icon).small())
    }
}
