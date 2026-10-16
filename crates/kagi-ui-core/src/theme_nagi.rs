//! Nagi — restrained Apple-derived palettes, not a color-vision certification.
//!
//! Four chromatic families only: blue (interaction), green (success), ochre
//! (warning), and brick (danger). Syntax, refs, graph lanes and ANSI reuse them;
//! magenta/cyan ANSI slots intentionally become brick/green. Generated avatars
//! are neutral; fetched images and application-supplied terminal RGB are untouched.
//! Labels, glyphs and diff +/- markers remain necessary: subdued green/brick and
//! repeated graph hues cannot uniquely encode state for every color vision.

use std::borrow::Cow;

use crate::{
    theme::{SyntaxPalette, Theme},
    theme_apple_dark::APPLE_DARK,
    theme_apple_light::APPLE_LIGHT,
};

const LIGHT_BLUE: u32 = 0x354d5f;
const LIGHT_GREEN: u32 = 0x375345;
const LIGHT_OCHRE: u32 = 0x5a4a30;
const LIGHT_BRICK: u32 = 0x653f39;
const DARK_BLUE: u32 = 0xb2c4d2;
const DARK_GREEN: u32 = 0xb3ccbf;
const DARK_OCHRE: u32 = 0xd6c9b3;
const DARK_BRICK: u32 = 0xd7bbb7;

pub const NAGI_LIGHT: Theme = Theme {
    slug: Cow::Borrowed("nagi-light"),
    name: Cow::Borrowed("Nagi Light"),

    // Neutral row selection retains normal/semantic ink, unlike Apple's
    // opaque accent selection. Input/diff selection has a separate blue source.
    selected: 0xdedede,
    inactive_selected: 0xd3d3d3,
    text_sub: 0x505050,
    text_main: 0x202020,
    text_muted: 0x505050,
    text_label: 0x505050,

    color_head: LIGHT_BRICK,
    color_branch: LIGHT_BLUE,
    color_remote: LIGHT_GREEN,
    color_tag: LIGHT_OCHRE,
    link: LIGHT_BLUE,
    // Composited at 30%, not the foreground blue: keeps syntax readable on
    // added/removed lines as well as ordinary Input/TextView backgrounds.
    selection_tint: 0x8fa7b7,

    color_success: LIGHT_GREEN,
    color_warning: LIGHT_OCHRE,
    color_blocker: LIGHT_BRICK,
    color_blocker_muted: 0x856b67,

    diff_added_bg: 0xedf2f0,
    diff_removed_bg: 0xf2eeed,
    diff_hunk: LIGHT_BLUE,

    change_added: LIGHT_GREEN,
    change_modified: LIGHT_OCHRE,
    change_deleted: LIGHT_BRICK,
    change_renamed: LIGHT_BLUE,
    change_typechange: 0x505050,
    change_dir: 0x505050,
    accent: LIGHT_BLUE,

    // Four families plus neutral lanes. Lane geometry/labels, not eight unique
    // hues, identify branches; lighter repetitions remain >= 3:1 on selection.
    lane_hsl: [
        (205.0 / 360.0, 0.28, 0.29),
        (38.0 / 360.0, 0.30, 0.27),
        (150.0 / 360.0, 0.20, 0.27),
        (8.0 / 360.0, 0.28, 0.31),
        (0.0, 0.0, 0.36),
        (205.0 / 360.0, 0.18, 0.40),
        (38.0 / 360.0, 0.20, 0.40),
        (0.0, 0.0, 0.45),
    ],
    avatar_sat: 0.0,
    avatar_light: 0.40,

    // Both normal/bright slots are readable on the terminal background. ANSI
    // black/white are legible grays, not literal endpoints that disappear here.
    term_bg: (0xff, 0xff, 0xff),
    term_fg: (0x20, 0x20, 0x20),
    term_cursor: (0x35, 0x4d, 0x5f),
    term_black: (0x24, 0x24, 0x24),
    term_red: (0x65, 0x3f, 0x39),
    term_green: (0x37, 0x53, 0x45),
    term_yellow: (0x5a, 0x4a, 0x30),
    term_blue: (0x35, 0x4d, 0x5f),
    term_magenta: (0x65, 0x3f, 0x39),
    term_cyan: (0x37, 0x53, 0x45),
    term_white: (0x60, 0x60, 0x60),
    term_bright_black: (0x66, 0x66, 0x66),
    term_bright_red: (0x72, 0x4c, 0x46),
    term_bright_green: (0x43, 0x60, 0x52),
    term_bright_yellow: (0x67, 0x57, 0x3c),
    term_bright_blue: (0x42, 0x5a, 0x6c),
    term_bright_magenta: (0x72, 0x4c, 0x46),
    term_bright_cyan: (0x43, 0x60, 0x52),
    term_bright_white: (0x20, 0x20, 0x20),
    term_selection: (0x8f, 0xa7, 0xb7, 0x40),

    // Color structure-bearing tokens, not every identifier. Comments remain
    // readable neutral ink; operators, punctuation and variables stay plain.
    syntax: SyntaxPalette {
        keyword: LIGHT_BLUE,
        string: LIGHT_GREEN,
        comment: 0x505050,
        type_name: LIGHT_BLUE,
        function: LIGHT_BLUE,
        number: LIGHT_OCHRE,
        operator: 0x202020,
        punctuation: 0x202020,
        variable: 0x202020,
        attribute: LIGHT_OCHRE,
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
    selected: 0x353535,
    inactive_selected: 0x373737,
    panel: 0x161616,
    sidebar: 0x101010,
    modal: 0x2a2a2a,
    text_main: 0xf2f2f2,
    text_sub: 0xb4b4b4,
    text_muted: 0xb4b4b4,
    text_label: 0xb4b4b4,

    color_head: DARK_BRICK,
    color_branch: DARK_BLUE,
    color_remote: DARK_GREEN,
    color_tag: DARK_OCHRE,
    link: DARK_BLUE,
    selection_tint: 0x4e6a7e,

    color_success: DARK_GREEN,
    color_warning: DARK_OCHRE,
    color_blocker: DARK_BRICK,
    color_blocker_muted: 0x8a6f6b,

    diff_added_bg: 0x19261f,
    diff_removed_bg: 0x2a1f1d,
    diff_hunk: DARK_BLUE,

    change_added: DARK_GREEN,
    change_modified: DARK_OCHRE,
    change_deleted: DARK_BRICK,
    change_renamed: DARK_BLUE,
    change_typechange: 0xb4b4b4,
    change_dir: 0xb4b4b4,
    accent: DARK_BLUE,

    lane_hsl: [
        (205.0 / 360.0, 0.26, 0.76),
        (38.0 / 360.0, 0.30, 0.77),
        (150.0 / 360.0, 0.20, 0.75),
        (8.0 / 360.0, 0.28, 0.78),
        (0.0, 0.0, 0.64),
        (205.0 / 360.0, 0.18, 0.62),
        (38.0 / 360.0, 0.20, 0.64),
        (0.0, 0.0, 0.76),
    ],
    avatar_sat: 0.0,
    avatar_light: 0.67,

    term_bg: (0x1c, 0x1c, 0x1c),
    term_fg: (0xf2, 0xf2, 0xf2),
    term_cursor: (0xb2, 0xc4, 0xd2),
    term_black: (0x98, 0x98, 0x98),
    term_red: (0xd7, 0xbb, 0xb7),
    term_green: (0xb3, 0xcc, 0xbf),
    term_yellow: (0xd6, 0xc9, 0xb3),
    term_blue: (0xb2, 0xc4, 0xd2),
    term_magenta: (0xd7, 0xbb, 0xb7),
    term_cyan: (0xb3, 0xcc, 0xbf),
    term_white: (0xd4, 0xd4, 0xd4),
    term_bright_black: (0xab, 0xab, 0xab),
    term_bright_red: (0xe4, 0xd5, 0xd3),
    term_bright_green: (0xcc, 0xdb, 0xd4),
    term_bright_yellow: (0xe3, 0xdb, 0xcf),
    term_bright_blue: (0xcc, 0xd8, 0xe0),
    term_bright_magenta: (0xe4, 0xd5, 0xd3),
    term_bright_cyan: (0xcc, 0xdb, 0xd4),
    term_bright_white: (0xf2, 0xf2, 0xf2),
    term_selection: (0x4e, 0x6a, 0x7e, 0x40),

    syntax: SyntaxPalette {
        keyword: DARK_BLUE,
        string: DARK_GREEN,
        comment: 0xb4b4b4,
        type_name: DARK_BLUE,
        function: DARK_BLUE,
        number: DARK_OCHRE,
        operator: 0xf2f2f2,
        punctuation: 0xf2f2f2,
        variable: 0xf2f2f2,
        attribute: DARK_OCHRE,
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
    c.sidebar_accent_foreground = to_hsla(k.text_main);
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
