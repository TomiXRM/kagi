//! Shared button variants for Kagi's theme tokens.

use gpui::{div, prelude::*, rgb, ElementId, Hsla, Role, SharedString, Window};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::theme::theme;

/// Reveal an existing focus target once per focus entry.
/// The latch is reset on blur, so later wheel/thumb input is never pinned.
pub(super) fn reveal_on_focus<E: ParentElement>(
    element: E,
    id: ElementId,
    focus: gpui::FocusHandle,
    scroll: gpui::ScrollHandle,
    window: &mut Window,
    cx: &mut gpui::App,
) -> E {
    use gpui_component::ElementExt as _;
    use std::cell::Cell;

    let revealed = window.with_element_namespace(id, |window| {
        window.use_keyed_state("button-focus-revealed", cx, |_, _| Cell::new(false))
    });
    element.on_prepaint(move |bounds, window, cx| {
        if !focus.is_focused(window) {
            revealed.read(cx).set(false);
            return;
        }
        if revealed.read(cx).get() {
            return;
        }
        let viewport = scroll.bounds();
        let margin = gpui::px(2.);
        let delta = if bounds.size.height > viewport.size.height - margin * 2.
            || bounds.top() < viewport.top() + margin
        {
            viewport.top() + margin - bounds.top()
        } else if bounds.bottom() > viewport.bottom() - margin {
            viewport.bottom() - margin - bounds.bottom()
        } else {
            revealed.read(cx).set(true);
            return;
        };
        let old = scroll.offset();
        let mut offset = old;
        offset.y = (old.y + delta).clamp(-scroll.max_offset().y, gpui::px(0.));
        if offset != old {
            // Parent prepaint still borrows this existing scroll owner.
            window.defer(cx, move |window, _| {
                scroll.set_offset(offset);
                window.refresh();
            });
        } else {
            revealed.read(cx).set(true);
        }
    })
}

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
    Secondary,
    Destructive,
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
            Self::Primary => t.ui_accent(),
            Self::Secondary => t.text_sub,
            Self::Destructive => t.color_blocker,
            Self::SideCurrent => t.color_branch,
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
            Self::Secondary => Some(ButtonVariant::Default),
            Self::Destructive => Some(ButtonVariant::Danger),
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

/// Dense actions and status badges share size and corner tokens, not semantics.
pub(super) fn dense_control<E: Styled + ParentElement>(
    control: E,
    id: &'static str,
    cx: &gpui::App,
) -> E {
    control
        .rounded(super::e2e::commit_panel_labels::radius(
            id,
            cx.theme().radius,
        ))
        .h(super::theme::scaled_px(20.))
        .min_h(gpui::px(20.))
        .py_0()
        .text_xs()
        .line_height(super::theme::scaled_px(16.))
        .border_2()
        .border_color(gpui::transparent_black())
        .overflow_hidden()
        .ml_2()
        .flex_shrink_0()
        .relative()
        .child(super::e2e::commit_panel_labels::measure_hit_box(
            id,
            super::keyboard_nav::RING,
        ))
}

/// Opaque, named status text: no focus, hover/pressed paint or disabled-action ink.
pub(super) fn status_badge(
    id: impl Into<ElementId>,
    probe: &'static str,
    label: impl Into<SharedString>,
    fill: u32,
    stop_row_click: bool,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let label = super::e2e::commit_panel_labels::caption(probe, label);
    let ink = super::theme::on_accent_foreground(fill);
    let (fill, ink) = super::e2e::commit_panel_labels::badge_colors(probe, fill, ink);
    dense_control(
        div()
            .id(id)
            .role(super::e2e::commit_panel_labels::role(probe, Role::Label))
            .aria_label(super::e2e::commit_panel_labels::name(probe, label.clone()))
            .flex()
            .items_center()
            .justify_center()
            .px_2()
            .bg(rgb(fill))
            .text_color(rgb(ink))
            .cursor_default()
            .when(stop_row_click, |badge| {
                badge
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
            })
            .child(label),
        probe,
        cx,
    )
}

pub fn apply_role(btn: Button, role: ButtonRole, cx: &gpui::App) -> Button {
    match role.filled_variant() {
        Some(variant) => btn.with_variant(variant),
        None => {
            let palette = theme();
            if matches!(&*palette.slug, "nagi-light" | "nagi-dark") {
                // Custom's fixed 40% pressed tint washes out subdued semantic ink.
                // Keep native neutral fills and labels, including disabled ink.
                btn.with_variant(ButtonVariant::Default)
            } else {
                btn.custom(tinted_action_variant(role.accent(&palette), cx))
            }
        }
    }
}

/// Name glyph-only kit buttons without adding a second accessibility node.
/// The pinned Button exposes Interactivity, but not its stateful AX builders.
pub(crate) fn named_button(
    button: Button,
    label: impl Into<SharedString>,
    toggled: Option<bool>,
) -> Button {
    button_accessibility(button, label.into(), toggled, None, false)
}

/// The pinned kit disables interaction/paint but does not publish AX disabled.
pub(crate) fn named_disabled_button(
    button: Button,
    label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    button_accessibility(button, label.into(), None, None, disabled)
}

pub(crate) fn named_button_with_description(
    id: impl Into<ElementId>,
    button: Button,
    label: SharedString,
    toggled: Option<bool>,
    description: Option<SharedString>,
    disabled: bool,
) -> gpui::AnyElement {
    let button = button_accessibility(button, label, toggled, description.clone(), disabled);
    if let Some(description) = description {
        button
            .hover_tooltip_builder(id, move |window, cx| {
                super::render_helpers::subject_tooltip(description.clone(), window, cx)
            })
            .into_any_element()
    } else {
        button.into_any_element()
    }
}

fn button_accessibility(
    button: Button,
    label: SharedString,
    toggled: Option<bool>,
    description: Option<SharedString>,
    disabled: bool,
) -> Button {
    struct AccessibleButton(Button);
    impl gpui::InteractiveElement for AccessibleButton {
        fn interactivity(&mut self) -> &mut gpui::Interactivity {
            self.0.interactivity()
        }
    }
    impl gpui::StatefulInteractiveElement for AccessibleButton {}
    let mut button = AccessibleButton(button).aria_label(label);
    if disabled {
        button = button.a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
            builder.parent_node().set_disabled();
        });
    }
    if let Some(description) = description {
        button = button.aria_description(description);
    }
    if let Some(toggled) = toggled {
        button = button.aria_toggled(if toggled {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        });
    }
    #[cfg(feature = "gui-e2e")]
    if let Some(id) = button.interactivity().element_id.clone() {
        button.0 = button.0.on_key_down(move |_, _, _| {
            super::e2e::pr_detail::record_key_focus(id.clone());
        });
    }
    button.0
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
    static MODAL_FOCUS: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
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
pub fn modal_focus_probe() -> Option<&'static str> {
    MODAL_FOCUS.with(|focus| focus.take())
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

/// Pinned, right-aligned dialog footer. Callers append Cancel first and the
/// default action last. When space runs out, Cancel wraps below the action;
/// construction order and button semantics stay unchanged.
pub(crate) fn modal_button_row() -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap_reverse()
        .flex_shrink_0()
        .gap_2()
        .justify_end()
}

/// Dialog actions share the small (24px) minimum; full captions may wrap taller.
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

struct ModalCaption {
    label: SharedString,
    detail: Option<SharedString>,
}

/// The Rebase/Replay footer keeps identity in AX/tooltip, not intrinsic width.
pub(crate) fn bounded_modal_button(
    id: &'static str,
    caption: (SharedString, SharedString),
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &gpui::App,
) -> gpui::AnyElement {
    render_modal_button(
        id,
        ModalCaption {
            label: caption.0,
            detail: Some(caption.1),
        },
        kind,
        reason,
        on_click,
        true,
        cx,
    )
}
/// AppNotice opts into Tab activation only for its armed danger action.
pub(crate) fn modal_button_with_tab_stop(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    tab_stop: bool,
    cx: &gpui::App,
) -> gpui::AnyElement {
    render_modal_button(
        id,
        ModalCaption {
            label: label.into(),
            detail: None,
        },
        kind,
        reason,
        on_click,
        tab_stop,
        cx,
    )
}

fn render_modal_button(
    id: &'static str,
    caption: ModalCaption,
    kind: ModalButtonKind,
    reason: Option<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    tab_stop: bool,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let ModalCaption { label, detail } = caption;
    record_modal_button(id, &label, reason.as_ref());
    let description = detail.map(|detail| match &reason {
        Some(reason) => SharedString::from(format!("{detail}\n{reason}")),
        None => detail,
    });
    #[cfg(feature = "gui-e2e")]
    if let Some(description) = &description {
        MODAL_BUTTONS.with(|buttons| {
            if let Some(button) = buttons.borrow_mut().get_mut(id) {
                button.description = Some(description.to_string());
            }
        });
    }
    if let Some(reason) = reason {
        return unavailable_modal_button(id, label, kind, reason, description, cx);
    }
    let role = match kind {
        ModalButtonKind::Cancel | ModalButtonKind::Secondary => ButtonRole::Secondary,
        ModalButtonKind::Primary => ButtonRole::Primary,
        ModalButtonKind::Destructive => ButtonRole::Destructive,
    };
    let bounded = description.is_some();
    let button = button_accessibility(
        apply_role(
            Button::new(id).child(modal_caption(label.clone(), bounded)),
            role,
            cx,
        ),
        label,
        None,
        description.clone(),
        false,
    );
    let button = button
        .small()
        .min_w_0()
        .max_w_full()
        .when(!bounded, |button| button.h_auto().min_h_6().py_1())
        .tab_stop(tab_stop)
        .on_key_down(move |event, window, cx| {
            #[cfg(feature = "gui-e2e")]
            MODAL_FOCUS.with(|focus| focus.set(Some(id)));
            super::keyboard_nav::stop_activation_keys(event, window, cx);
        })
        .on_click(on_click);
    if let Some(description) = description {
        button
            .hover_tooltip_builder((id, 0usize), move |window, cx| {
                super::render_helpers::subject_tooltip(description.clone(), window, cx)
            })
            .into_any_element()
    } else {
        button.into_any_element()
    }
}

fn unavailable_modal_button(
    id: &'static str,
    label: SharedString,
    kind: ModalButtonKind,
    reason: SharedString,
    description: Option<SharedString>,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let bounded = description.is_some();
    let description = description.unwrap_or(reason);
    let accent = match kind {
        ModalButtonKind::Destructive => theme().color_blocker,
        ModalButtonKind::Primary => theme().ui_accent(),
        ModalButtonKind::Cancel | ModalButtonKind::Secondary => theme().text_sub,
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_description(description.clone())
        .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
            builder.parent_node().set_disabled();
        })
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .h_6()
        .min_w_0()
        .max_w_full()
        .when(!bounded, |button| button.h_auto().min_h_6().py_1())
        .px_3()
        .text_xs()
        .rounded(cx.theme().radius)
        .bg(Hsla::from(rgb(accent)).opacity(0.15))
        .text_color(cx.theme().muted_foreground.opacity(0.5))
        .child(modal_caption(label, bounded))
        .hover_tooltip_builder((id, 0usize), move |window, cx| {
            if bounded {
                super::render_helpers::subject_tooltip(description.clone(), window, cx)
            } else {
                Tooltip::new(description.clone()).build(window, cx)
            }
        })
        .into_any_element()
}

fn modal_caption(label: SharedString, bounded: bool) -> gpui::AnyElement {
    if !bounded {
        return div()
            .min_w_0()
            .max_w_full()
            .whitespace_normal()
            .line_height(gpui::relative(1.))
            .text_center()
            .child(label)
            .into_any_element();
    }
    div()
        .max_w(super::theme::scaled_px(160.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .relative()
        .child(super::e2e::measure_inside("plan-confirm-caption"))
        .child(label)
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
            .hover_tooltip_builder((self.id, 0usize), move |window, cx| {
                Tooltip::new(label.clone()).build(window, cx)
            })
    }
}
