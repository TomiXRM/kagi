//! Applicable visual roles from the final PR #1814 settings kit.
//! Pages compose these roles; interaction and native effects remain separate.
use gpui::{rgb, Hsla, Pixels};
use kagi_ui_core::theme::{self, theme};

pub(crate) fn pt(value: f32) -> Pixels {
    theme::scaled_px(value * 16. / theme::BASE_REM_PX)
}

pub(crate) mod metrics {
    pub const SIDEBAR_WIDTH: f32 = 212.;
    pub const SIDEBAR_ROW: f32 = 32.;
    pub const TILE: f32 = 20.;
    pub const ROW: f32 = 40.;
    pub const ROW_WITH_SUBTITLE: f32 = 52.;
    pub const GROUP_RADIUS: f32 = 10.;
    pub const GROUP_PADDING_X: f32 = 12.;
    pub const CONTROL_HEIGHT: f32 = 24.;
    pub const CONTROL_RADIUS: f32 = 7.;
    pub const CONTROL_HIT_HEIGHT: f32 = 28.;
    pub const STEPPER_GAP: f32 = 8.;
    pub const STEPPER_VALUE_WIDTH: f32 = 40.;
    pub const THEME_CARD_WIDTH: f32 = 144.;
    pub const DETAIL_PADDING_LEFT: f32 = 6.;
    pub const DETAIL_PADDING_RIGHT: f32 = 18.;
    /// Physical pixels: the two-pixel ring, three-pixel gap and breathing room.
    pub const FOCUS_REVEAL_MARGIN: f32 = 7.;
    pub const BODY: f32 = 13.;
    pub const SECONDARY: f32 = 11.5;
    pub const TITLE: f32 = 15.;
}

pub(crate) fn detail_min_width() -> Pixels {
    let stepper = pt(metrics::CONTROL_HIT_HEIGHT * 2.
        + metrics::STEPPER_GAP * 2.
        + metrics::STEPPER_VALUE_WIDTH
        + metrics::GROUP_PADDING_X * 2.)
        + gpui::px(2.);
    let theme = pt(metrics::THEME_CARD_WIDTH) + gpui::px(metrics::FOCUS_REVEAL_MARGIN * 2.);
    stepper.max(theme) + pt(metrics::DETAIL_PADDING_LEFT + metrics::DETAIL_PADDING_RIGHT)
}

pub(crate) fn mix(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((base >> shift) & 0xff) as f32;
        let b = ((over >> shift) & 0xff) as f32;
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

pub(crate) mod ink {
    use super::{mix, rgb, theme, Hsla};

    pub fn window() -> u32 {
        let t = theme();
        if t.dark {
            t.bg_base
        } else {
            t.panel
        }
    }
    pub fn sidebar() -> u32 {
        let t = theme();
        mix(window(), t.surface, if t.dark { 0.22 } else { 0.35 })
    }
    pub fn card() -> u32 {
        let t = theme();
        if t.dark {
            mix(t.bg_base, t.surface, 0.42)
        } else {
            t.bg_base
        }
    }
    pub fn hairline() -> u32 {
        let t = theme();
        mix(card(), t.text_sub, if t.dark { 0.18 } else { 0.22 })
    }
    pub fn control() -> u32 {
        mix(card(), theme().text_sub, 0.16)
    }
    pub fn control_hover() -> u32 {
        mix(card(), theme().text_sub, 0.26)
    }
    pub fn control_pressed() -> u32 {
        mix(card(), theme().text_sub, 0.36)
    }
    pub fn selected() -> Hsla {
        // Retain the reviewed light/dark parity correction, not the experiment's bug.
        let t = theme();
        rgb(match t.slug.as_ref() {
            "apple-light" => 0xffffff,
            "apple-dark" => 0x66666c,
            _ => return t.segment_selected_bg(),
        })
        .into()
    }
    pub fn text() -> Hsla {
        rgb(theme().text_main).into()
    }
    pub fn secondary() -> Hsla {
        rgb(theme().text_sub).into()
    }
    pub fn disabled_opacity() -> f32 {
        0.45
    }
}
