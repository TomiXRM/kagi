//! Color Vision (Blue/Orange) — a Catppuccin Mocha derivative for
//! protanopia / deuteranopia (#354 slice 4, ADR-0216).
//!
//! Every pairing that the UI uses to say "added vs removed", "ours vs
//! theirs" or "ok vs blocked" is moved off the red/green axis onto
//! blue/orange from the Okabe–Ito palette; warnings are told apart by
//! luminance (yellow). Everything else is inherited from Mocha.

use std::borrow::Cow;

use crate::{theme::Theme, theme_catppuccin_mocha::CATPPUCCIN_MOCHA};

/// Okabe–Ito sky blue: added / success / ours.
pub const CVD_BLUE: u32 = 0x56b4e9;
/// Okabe–Ito orange: removed / blocker / theirs.
pub const CVD_ORANGE: u32 = 0xe69f00;
/// Okabe–Ito yellow: warning / modified (separated by luminance).
pub const CVD_YELLOW: u32 = 0xf0e442;
/// Okabe–Ito reddish purple: renamed / hunk headers.
pub const CVD_PURPLE: u32 = 0xcc79a7;

pub const COLOR_VISION: Theme = Theme {
    slug: Cow::Borrowed("color-vision"),
    name: Cow::Borrowed("Color Vision (Blue/Orange)"),

    color_head: CVD_ORANGE,
    // Conflict editor: ours = branch, theirs = remote.
    color_branch: CVD_BLUE,
    selection_tint: CVD_BLUE,
    color_remote: CVD_ORANGE,

    color_success: CVD_BLUE,
    color_warning: CVD_YELLOW,
    color_blocker: CVD_ORANGE,
    color_blocker_muted: 0x8a6420,

    diff_added_bg: 0x123a52,
    diff_removed_bg: 0x4d3008,
    diff_hunk: CVD_PURPLE,

    change_added: CVD_BLUE,
    change_modified: CVD_YELLOW,
    change_deleted: CVD_ORANGE,
    change_renamed: CVD_PURPLE,

    // Okabe–Ito order with blue lightened for a dark background; adjacent
    // lanes always differ in hue family or luminance.
    lane_hsl: [
        (0.1152, 1.000, 0.451), // orange         #e69f00
        (0.5601, 0.770, 0.625), // sky blue       #56b4e9
        (0.1552, 0.853, 0.600), // yellow         #f0e442
        (0.9076, 0.449, 0.637), // reddish purple #cc79a7
        (0.4546, 1.000, 0.310), // bluish green   #009e73
        (0.0736, 1.000, 0.418), // vermillion     #d55e00
        (0.5773, 0.651, 0.539), // blue           #3d8fd6
        (0.0000, 0.000, 0.722), // grey           #b8b8b8
    ],

    term_red: (0xe6, 0x9f, 0x00),
    term_green: (0x56, 0xb4, 0xe9),
    term_yellow: (0xf0, 0xe4, 0x42),
    term_bright_red: (0xe6, 0x9f, 0x00),
    term_bright_green: (0x56, 0xb4, 0xe9),
    term_bright_yellow: (0xf0, 0xe4, 0x42),

    ..CATPPUCCIN_MOCHA
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_vision::{delta_e, Cvd};

    /// The pairs that carry "added vs removed": badge/text colours and the
    /// diff row backgrounds.
    fn add_remove_pairs(t: &Theme) -> [(&'static str, u32, u32); 3] {
        [
            ("change_added/deleted", t.change_added, t.change_deleted),
            ("success/blocker", t.color_success, t.color_blocker),
            ("diff bg added/removed", t.diff_added_bg, t.diff_removed_bg),
        ]
    }

    #[test]
    fn add_remove_pairs_stay_apart_for_every_dichromacy() {
        for (name, a, b) in add_remove_pairs(&COLOR_VISION) {
            let normal = delta_e(a, b, None);
            assert!(normal >= 20.0, "{name}: ΔE {normal:.1} < 20");
            for cvd in Cvd::ALL {
                let d = delta_e(a, b, Some(cvd));
                assert!(d >= 15.0, "{name} under {cvd:?}: ΔE {d:.1} < 15");
            }
        }
    }

    /// Control: the theme it derives from does not meet the bar for
    /// deuteranopia, which is why this theme exists.
    #[test]
    fn mocha_add_remove_pairs_collapse_for_deuteranopia() {
        let worst = add_remove_pairs(&CATPPUCCIN_MOCHA)
            .iter()
            .map(|(_, a, b)| delta_e(*a, *b, Some(Cvd::Deuteranopia)))
            .fold(f64::INFINITY, f64::min);
        assert!(worst < 15.0, "Mocha's worst deutan ΔE is {worst:.1}");
    }

    #[test]
    fn lane_hsl_matches_the_documented_hex() {
        let hex = [
            0xe69f00, 0x56b4e9, 0xf0e442, 0xcc79a7, 0x009e73, 0xd55e00, 0x3d8fd6, 0xb8b8b8,
        ];
        for (i, want) in hex.iter().enumerate() {
            let rgba: gpui::Rgba = COLOR_VISION.lane_color(i).into();
            let got = ((rgba.r * 255.0).round() as u32) << 16
                | ((rgba.g * 255.0).round() as u32) << 8
                | (rgba.b * 255.0).round() as u32;
            assert!(
                delta_e(got, *want, None) < 1.0,
                "lane {i}: #{got:06x} vs #{want:06x}"
            );
        }
    }
}
