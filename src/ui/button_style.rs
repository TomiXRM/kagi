//! Shared button variants for Kagi's theme tokens.

use gpui::{rgb, ElementId, Hsla, SharedString};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};

use super::theme::theme;

/// A translucent, theme-tinted button variant for accent action buttons.
///
/// The gpui-component filled `success`/`warning`/`danger` variants use their own
/// foreground/hover tokens. Kagi syncs only part of that palette, so labels can
/// wash out in light themes. Build accent buttons from Kagi's palette instead.
pub fn tinted_action_variant(base: u32, cx: &gpui::App) -> ButtonCustomVariant {
    let c = Hsla::from(rgb(base));
    let (rest, hover, active) = if theme().dark {
        (0.16, 0.26, 0.34)
    } else {
        (0.14, 0.22, 0.30)
    };
    ButtonCustomVariant::new(cx)
        .color(c.opacity(rest))
        .foreground(c)
        .hover(c.opacity(hover))
        .active(c.opacity(active))
}

/// Kagi-owned constructors for gpui-component buttons.
///
/// Use these for semantic action buttons instead of calling
/// `Button::success()` / `warning()` / `danger()` directly. Those filled
/// variants depend on gpui-component foreground/hover tokens that Kagi does not
/// fully own.
pub struct KagiButton;

impl KagiButton {
    pub fn accent(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        accent: u32,
        cx: &gpui::App,
    ) -> Button {
        apply_accent(Button::new(id).label(label), accent, cx)
    }

    pub fn accent_icon(
        id: impl Into<ElementId>,
        icon_path: &'static str,
        label: impl Into<SharedString>,
        accent: u32,
        cx: &gpui::App,
    ) -> Button {
        apply_accent(
            Button::new(id)
                .icon(gpui_component::Icon::empty().path(icon_path))
                .label(label),
            accent,
            cx,
        )
    }
}

pub fn apply_accent(btn: Button, accent: u32, cx: &gpui::App) -> Button {
    let t = theme();
    if accent == t.color_success || accent == t.color_warning || accent == t.color_blocker {
        btn.custom(tinted_action_variant(accent, cx))
    } else if accent == t.color_branch {
        btn.primary()
    } else if accent == t.color_remote {
        btn.info()
    } else {
        btn.ghost()
    }
}
