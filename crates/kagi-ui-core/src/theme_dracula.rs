//! The dracula theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const DRACULA: Theme = Theme {
    slug: "dracula",
    name: "Dracula",
    dark: true,

    bg_base: 0x282a36,
    bg_row_alt: 0x21222c,
    surface: 0x343746,
    selected: 0x44475a,
    panel: 0x21222c,
    sidebar: 0x191a21,
    modal: 0x343746,
    modal_overlay: 0x000000,

    text_main: 0xf8f8f2,
    text_sub: 0xc8c8d4,
    text_muted: 0x6272a4,
    text_label: 0x7e84ad,

    color_head: 0xff79c6,   // pink
    color_branch: 0xbd93f9, // purple
    selection_tint: 0xbd93f9,
    color_remote: 0x50fa7b, // green
    color_tag: 0xffb86c,    // orange

    color_success: 0x50fa7b,
    color_warning: 0xf1fa8c,
    color_blocker: 0xff5555,
    color_blocker_muted: 0x9a4d4d,

    diff_added_bg: 0x1d3b2b,
    diff_removed_bg: 0x3a1e22,
    diff_hunk: 0x8be9fd,

    change_added: 0x50fa7b,
    change_modified: 0xf1fa8c,
    change_deleted: 0xff5555,
    change_renamed: 0x8be9fd,
    change_typechange: 0x6272a4,
    change_dir: 0x7e84ad,

    accent: 0xbd93f9, // purple

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.70,
    avatar_light: 0.65,

    term_bg: (0x28, 0x2a, 0x36),
    term_fg: (0xf8, 0xf8, 0xf2),
    term_cursor: (0xf8, 0xf8, 0xf2),
    term_black: (0x21, 0x22, 0x2c),
    term_red: (0xff, 0x55, 0x55),
    term_green: (0x50, 0xfa, 0x7b),
    term_yellow: (0xf1, 0xfa, 0x8c),
    term_blue: (0xbd, 0x93, 0xf9),
    term_magenta: (0xff, 0x79, 0xc6),
    term_cyan: (0x8b, 0xe9, 0xfd),
    term_white: (0xf8, 0xf8, 0xf2),
    term_bright_black: (0x62, 0x72, 0xa4),
    term_bright_red: (0xff, 0x6e, 0x6e),
    term_bright_green: (0x69, 0xff, 0x94),
    term_bright_yellow: (0xff, 0xff, 0xa5),
    term_bright_blue: (0xd6, 0xac, 0xff),
    term_bright_magenta: (0xff, 0x92, 0xdf),
    term_bright_cyan: (0xa4, 0xff, 0xff),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x44, 0x47, 0x5a, 0x99),

    // Code colours: Dracula (official spec / VS Code theme). Punctuation and
    // plain variables are the foreground by design; operators reuse Pink.
    syntax: SyntaxPalette {
        keyword: 0xff79c6,     // Pink
        string: 0xf1fa8c,      // Yellow
        comment: 0x6272a4,     // Comment blue
        type_name: 0x8be9fd,   // Cyan
        function: 0x50fa7b,    // Green
        number: 0xbd93f9,      // Purple
        operator: 0xff79c6,    // Pink (no separate operator rule)
        punctuation: 0xf8f8f2, // Foreground — flat by design
        variable: 0xf8f8f2,    // Foreground
        attribute: 0x50fa7b,   // Green
    },
};
