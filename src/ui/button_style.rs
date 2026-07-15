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
