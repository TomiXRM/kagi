//! The catppuccin latte theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_LIGHT};

pub const CATPPUCCIN_LATTE: Theme = Theme {
    slug: "catppuccin-latte",
    name: "Catppuccin Latte",
    dark: false,

    bg_base: 0xeff1f5,    // base
    bg_row_alt: 0xe6e9ef, // mantle
    surface: 0xccd0da,    // surface0
    selected: 0xbcc0cc,   // surface1
    panel: 0xe6e9ef,      // mantle
    sidebar: 0xdce0e8,    // crust
    modal: 0xccd0da,      // surface0
    modal_overlay: 0x4c4f69,

    text_main: 0x4c4f69,  // text
    text_sub: 0x6c6f85,   // subtext0
    text_muted: 0xacb0be, // surface2
    text_label: 0x9ca0b0, // overlay0

    color_head: 0xd20f39,   // red
    color_branch: 0x1e66f5, // blue
    selection_tint: 0x1e66f5,
    color_remote: 0x40a02b, // green
    color_tag: 0xfe640b,    // peach

    color_success: 0x40a02b,
    color_warning: 0xdf8e1d,
    color_blocker: 0xd20f39,
    color_blocker_muted: 0xd98a9a,

    diff_added_bg: 0xdcf0d8,
    diff_removed_bg: 0xfbdde1,
    diff_hunk: 0x1e66f5,

    change_added: 0x40a02b,
    change_modified: 0xdf8e1d,
    change_deleted: 0xd20f39,
    change_renamed: 0x1e66f5,
    change_typechange: 0xacb0be,
    change_dir: 0x9ca0b0,

    accent: 0x8839ef, // mauve

    lane_hsl: LANE_PALETTE_LIGHT,

    avatar_sat: 0.55,
    avatar_light: 0.50,

    term_bg: (0xef, 0xf1, 0xf5),
    term_fg: (0x4c, 0x4f, 0x69),
    term_cursor: (0xea, 0x76, 0xcb),
    term_black: (0xbc, 0xc0, 0xcc),
    term_red: (0xd2, 0x0f, 0x39),
    term_green: (0x40, 0xa0, 0x2b),
    term_yellow: (0xdf, 0x8e, 0x1d),
    term_blue: (0x1e, 0x66, 0xf5),
    term_magenta: (0x88, 0x39, 0xef),
    term_cyan: (0x04, 0xa5, 0xe5),
    term_white: (0x5c, 0x5f, 0x77),
    term_bright_black: (0xac, 0xb0, 0xbe),
    term_bright_red: (0xd2, 0x0f, 0x39),
    term_bright_green: (0x40, 0xa0, 0x2b),
    term_bright_yellow: (0xdf, 0x8e, 0x1d),
    term_bright_blue: (0x1e, 0x66, 0xf5),
    term_bright_magenta: (0x88, 0x39, 0xef),
    term_bright_cyan: (0x04, 0xa5, 0xe5),
    term_bright_white: (0x4c, 0x4f, 0x69),
    term_selection: (0xac, 0xb0, 0xbe, 0x99),

    // Code colours: Catppuccin Latte — same role mapping as Mocha.
    syntax: SyntaxPalette {
        keyword: 0x8839ef,     // Mauve
        string: 0x40a02b,      // Green
        comment: 0x7c7f93,     // Overlay 2
        type_name: 0xdf8e1d,   // Yellow
        function: 0x1e66f5,    // Blue
        number: 0xfe640b,      // Peach
        operator: 0x04a5e5,    // Sky
        punctuation: 0x7c7f93, // Overlay 2
        variable: 0xe64553,    // Maroon
        attribute: 0xdf8e1d,   // Yellow
    },
};
