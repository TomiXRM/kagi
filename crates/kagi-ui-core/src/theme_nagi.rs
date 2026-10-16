//! Nagi — restrained Apple-derived palettes, not a color-vision certification.
//!
//! Four chromatic families only: blue (interaction/structure), green (success),
//! amber (warning), and red (danger). Graph lanes cycle blue/green/amber;
//! remote/tag metadata and ordinary syntax stay neutral. ANSI semantic slots
//! reuse the four families; generated avatars are neutral. Fetched images and
//! application-supplied terminal RGB are untouched. Labels, glyphs and diff +/-
//! markers remain necessary; hue alone cannot encode every state for everyone.

use std::borrow::Cow;

use crate::{
    theme::{SyntaxPalette, Theme},
    theme_apple_dark::APPLE_DARK,
    theme_apple_light::APPLE_LIGHT,
};

const LIGHT_BLUE: u32 = 0x1558a0;
const LIGHT_GREEN: u32 = 0x18643e;
const LIGHT_OCHRE: u32 = 0x7b5000;
const LIGHT_BRICK: u32 = 0x9e3326;
const DARK_BLUE: u32 = 0x70b3ff;
const DARK_GREEN: u32 = 0x73c895;
const DARK_OCHRE: u32 = 0xe8b45f;
const DARK_BRICK: u32 = 0xff9990;

pub const NAGI_LIGHT: Theme = Theme {
    slug: Cow::Borrowed("nagi-light"),
    name: Cow::Borrowed("Nagi Light"),

    // Blue identifies the active selection without replacing normal/semantic
    // ink; inactive selection and the surrounding chrome remain neutral.
    selected: 0xc9e1fb,
    inactive_selected: 0xd3d3d3,
    text_sub: 0x505050,
    text_main: 0x202020,
    text_muted: 0x505050,
    text_label: 0x505050,

    color_head: LIGHT_BLUE,
    color_branch: LIGHT_BLUE,
    color_remote: 0x505050,
    color_tag: 0x505050,
    link: LIGHT_BLUE,
    // The existing 30% text-selection overlay stays distinctly blue even over
    // green/red diff washes, while its lightness preserves semantic ink.
    selection_tint: 0x9fd3ff,

    color_success: LIGHT_GREEN,
    color_warning: LIGHT_OCHRE,
    color_blocker: LIGHT_BRICK,
    color_blocker_muted: 0x856b67,

    diff_added_bg: 0xd8ede2,
    diff_removed_bg: 0xf5dcd7,
    diff_hunk: LIGHT_BLUE,

    change_added: LIGHT_GREEN,
    change_modified: LIGHT_OCHRE,
    change_deleted: LIGHT_BRICK,
    change_renamed: LIGHT_BLUE,
    change_typechange: 0x505050,
    change_dir: 0x505050,
    accent: LIGHT_BLUE,

    // Three chromatic lanes reuse the global families without adding hues.
    lane_hsl: [
        (0.58633095, 0.7679558, 0.35490197),
        (0.41666666, 0.61290324, 0.24313726),
        (0.10569106, 1.0, 0.24117647),
        (0.58633095, 0.7679558, 0.35490197),
        (0.41666666, 0.61290324, 0.24313726),
        (0.10569106, 1.0, 0.24117647),
        (0.58633095, 0.7679558, 0.35490197),
        (0.41666666, 0.61290324, 0.24313726),
    ],
    avatar_sat: 0.0,
    avatar_light: 0.40,

    // Both normal/bright slots are readable on the terminal background. ANSI
    // black/white are legible grays, not literal endpoints that disappear here.
    term_bg: (0xff, 0xff, 0xff),
    term_fg: (0x20, 0x20, 0x20),
    term_cursor: (0x15, 0x58, 0xa0),
    term_black: (0x24, 0x24, 0x24),
    term_red: (0x9e, 0x33, 0x26),
    term_green: (0x18, 0x64, 0x3e),
    term_yellow: (0x7b, 0x50, 0x00),
    term_blue: (0x15, 0x58, 0xa0),
    term_magenta: (0x9e, 0x33, 0x26),
    term_cyan: (0x18, 0x64, 0x3e),
    term_white: (0x60, 0x60, 0x60),
    term_bright_black: (0x66, 0x66, 0x66),
    term_bright_red: (0xa8, 0x4c, 0x41),
    term_bright_green: (0x2f, 0x74, 0x51),
    term_bright_yellow: (0x88, 0x62, 0x18),
    term_bright_blue: (0x2c, 0x69, 0xaa),
    term_bright_magenta: (0xa8, 0x4c, 0x41),
    term_bright_cyan: (0x2f, 0x74, 0x51),
    term_bright_white: (0x20, 0x20, 0x20),
    term_selection: (0x9f, 0xd3, 0xff, 0x40),

    // Only keywords/types carry structure blue. Ordinary code and comments
    // stay neutral so syntax does not compete with added/deleted row cues.
    syntax: SyntaxPalette {
        keyword: LIGHT_BLUE,
        string: 0x202020,
        comment: 0x505050,
        type_name: LIGHT_BLUE,
        function: 0x202020,
        number: 0x202020,
        operator: 0x202020,
        punctuation: 0x202020,
        variable: 0x202020,
        attribute: 0x202020,
    },
    ..APPLE_LIGHT
};

pub const NAGI_DARK: Theme = Theme {
    slug: Cow::Borrowed("nagi-dark"),
    name: Cow::Borrowed("Nagi Dark"),

    // Remove the inherited blue cast, preserving Apple's elevation order.
    bg_base: 0x101010,
    bg_row_alt: 0x181818,
    surface: 0x2c2c2c,
    selected: 0x1768b5,
    inactive_selected: 0x373737,
    panel: 0x161616,
    sidebar: 0x101010,
    modal: 0x2a2a2a,
    text_main: 0xf2f2f2,
    text_sub: 0xb4b4b4,
    text_muted: 0xb4b4b4,
    text_label: 0xb4b4b4,

    color_head: DARK_BLUE,
    color_branch: 0x99c9ff,
    color_remote: 0xb4b4b4,
    color_tag: 0xb4b4b4,
    link: DARK_BLUE,
    selection_tint: 0x294e73,

    color_success: DARK_GREEN,
    color_warning: DARK_OCHRE,
    color_blocker: DARK_BRICK,
    color_blocker_muted: 0x8a6f6b,

    diff_added_bg: 0x163725,
    diff_removed_bg: 0x422725,
    diff_hunk: DARK_BLUE,

    change_added: DARK_GREEN,
    change_modified: DARK_OCHRE,
    change_deleted: DARK_BRICK,
    change_renamed: DARK_BLUE,
    change_typechange: 0xb4b4b4,
    change_dir: 0xb4b4b4,
    accent: DARK_BLUE,

    lane_hsl: [
        (0.5882353, 1.0, 0.8),
        (0.41558442, 0.503268, 0.7),
        (0.105919, 0.6993464, 0.7),
        (0.5882353, 1.0, 0.8),
        (0.41558442, 0.503268, 0.7),
        (0.105919, 0.6993464, 0.7),
        (0.5882353, 1.0, 0.8),
        (0.41558442, 0.503268, 0.7),
    ],
    avatar_sat: 0.0,
    avatar_light: 0.67,

    term_bg: (0x1c, 0x1c, 0x1c),
    term_fg: (0xf2, 0xf2, 0xf2),
    term_cursor: (0x70, 0xb3, 0xff),
    term_black: (0x98, 0x98, 0x98),
    term_red: (0xff, 0x99, 0x90),
    term_green: (0x73, 0xc8, 0x95),
    term_yellow: (0xe8, 0xb4, 0x5f),
    term_blue: (0x70, 0xb3, 0xff),
    term_magenta: (0xff, 0x99, 0x90),
    term_cyan: (0x73, 0xc8, 0x95),
    term_white: (0xd4, 0xd4, 0xd4),
    term_bright_black: (0xab, 0xab, 0xab),
    term_bright_red: (0xff, 0xad, 0xa6),
    term_bright_green: (0x8f, 0xd3, 0xaa),
    term_bright_yellow: (0xed, 0xc3, 0x81),
    term_bright_blue: (0x8d, 0xc2, 0xff),
    term_bright_magenta: (0xff, 0xad, 0xa6),
    term_bright_cyan: (0x8f, 0xd3, 0xaa),
    term_bright_white: (0xf2, 0xf2, 0xf2),
    term_selection: (0x29, 0x4e, 0x73, 0x40),

    syntax: SyntaxPalette {
        keyword: DARK_BLUE,
        string: 0xf2f2f2,
        comment: 0xb4b4b4,
        type_name: DARK_BLUE,
        function: 0xf2f2f2,
        number: 0xf2f2f2,
        operator: 0xf2f2f2,
        punctuation: 0xf2f2f2,
        variable: 0xf2f2f2,
        attribute: 0xf2f2f2,
    },
    ..APPLE_DARK
};

/// Complete the existing bridge's inherited kit roles without adding new hues.
pub(crate) fn complete_component_palette(k: &Theme, c: &mut gpui_component::theme::ThemeColor) {
    let to_hsla = |value| gpui::Hsla::from(gpui::rgb(value));

    let away = to_hsla(if k.dark { 0xffffff } else { 0x000000 });
    c.link_hover = c.link;
    c.link_active = c.link;
    c.success_foreground = to_hsla(k.bg_base);
    c.success_hover = c.success.blend(away.alpha(0.12));
    c.success_active = c.success.blend(away.alpha(0.24));
    c.warning_foreground = to_hsla(k.bg_base);
    c.warning_hover = c.warning.blend(away.alpha(0.12));
    c.warning_active = c.warning.blend(away.alpha(0.24));
    c.danger_foreground = c.button_danger_foreground;
    c.info_foreground = c.primary_foreground;
    c.info_hover = c.primary_hover;
    c.info_active = c.primary_active;
    c.button_success = c.success;
    c.button_success_foreground = c.success_foreground;
    c.button_success_hover = c.success_hover;
    c.button_success_active = c.success_active;
    c.button_warning_hover = c.warning_hover;
    c.button_warning_active = c.warning_active;
    c.list_active_border = c.primary;
    c.list_even = to_hsla(k.bg_row_alt);
    c.list_head = to_hsla(k.panel);
    c.table = to_hsla(k.bg_base);
    c.table_active = to_hsla(k.selected);
    c.table_active_border = c.primary;
    c.table_even = to_hsla(k.bg_row_alt);
    c.table_head = to_hsla(k.panel);
    c.table_head_foreground = to_hsla(k.text_main);
    c.table_foot = to_hsla(k.panel);
    c.table_foot_foreground = to_hsla(k.text_sub);
    c.table_hover = to_hsla(k.surface);
    c.table_row_border = to_hsla(k.surface);
    c.sidebar_primary = c.primary;
    c.sidebar_primary_foreground = c.primary_foreground;
    c.sidebar_accent = to_hsla(k.selected);
    c.sidebar_accent_foreground = k.selection_foreground(true);
    c.tab_active = k.segment_selected_bg();
    c.tab_active_foreground = k.selection_foreground(true);
    c.sidebar_border = to_hsla(k.surface);
    c.progress_bar = c.primary;
    c.slider_bar = c.primary;
    c.slider_thumb = c.primary_foreground;
    c.switch = c.input;
    c.switch_thumb = c.primary_foreground;
    c.drop_target = c.primary.alpha(0.12);
    c.chart_1 = k.lane_color(0);
    c.chart_2 = k.lane_color(1);
    c.chart_3 = k.lane_color(2);
    c.chart_4 = k.lane_color(3);
    c.chart_5 = k.lane_color(4);
    c.chart_bullish = c.success;
    c.chart_bearish = c.danger;
    c.red = c.danger;
    c.red_light = c.red.alpha(0.12);
    c.green = c.success;
    c.green_light = c.green.alpha(0.12);
    c.blue = c.primary;
    c.blue_light = c.blue.alpha(0.12);
    c.yellow = c.warning;
    c.yellow_light = c.yellow.alpha(0.12);
    c.magenta = c.red;
    c.magenta_light = c.red_light;
    c.cyan = c.green;
    c.cyan_light = c.green_light;
}
