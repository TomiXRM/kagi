//! The one dark theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const ONE_DARK: Theme = Theme {
    slug: "one-dark",
    name: "One Dark",
    dark: true,

    bg_base: 0x282c34,
    bg_row_alt: 0x24272e,
    surface: 0x3a3f4b,
    selected: 0x4b5263,
    panel: 0x21252b,
    sidebar: 0x1c1f24,
    modal: 0x3a3f4b,
    modal_overlay: 0x000000,

    text_main: 0xabb2bf,
    text_sub: 0x9099a8,
    text_muted: 0x5c6370,
    text_label: 0x6b7280,

    color_head: 0xe06c75,   // red
    color_branch: 0x61afef, // blue
    selection_tint: 0x61afef,
    color_remote: 0x98c379, // green
    color_tag: 0xe5c07b,    // yellow

    color_success: 0x98c379,
    color_warning: 0xe5c07b,
    color_blocker: 0xe06c75,
    color_blocker_muted: 0x8a4f55,

    diff_added_bg: 0x26392b,
    diff_removed_bg: 0x3a2526,
    diff_hunk: 0x61afef,

    change_added: 0x98c379,
    change_modified: 0xe5c07b,
    change_deleted: 0xe06c75,
    change_renamed: 0x61afef,
    change_typechange: 0x5c6370,
    change_dir: 0x6b7280,

    accent: 0xc678dd, // purple

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.55,
    avatar_light: 0.62,

    term_bg: (0x28, 0x2c, 0x34),
    term_fg: (0xab, 0xb2, 0xbf),
    term_cursor: (0x52, 0x8b, 0xff),
    term_black: (0x3f, 0x44, 0x51),
    term_red: (0xe0, 0x6c, 0x75),
    term_green: (0x98, 0xc3, 0x79),
    term_yellow: (0xe5, 0xc0, 0x7b),
    term_blue: (0x61, 0xaf, 0xef),
    term_magenta: (0xc6, 0x78, 0xdd),
    term_cyan: (0x56, 0xb6, 0xc2),
    term_white: (0xab, 0xb2, 0xbf),
    term_bright_black: (0x5c, 0x63, 0x70),
    term_bright_red: (0xe0, 0x6c, 0x75),
    term_bright_green: (0x98, 0xc3, 0x79),
    term_bright_yellow: (0xe5, 0xc0, 0x7b),
    term_bright_blue: (0x61, 0xaf, 0xef),
    term_bright_magenta: (0xc6, 0x78, 0xdd),
    term_bright_cyan: (0x56, 0xb6, 0xc2),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x3e, 0x44, 0x51, 0xcc),

    // Code colours: Atom One Dark (via One Dark Pro). Punctuation is plain.
    syntax: SyntaxPalette {
        keyword: 0xc678dd, // purple
        string: 0x98c379,  // green
        comment: 0x7f848e,
        type_name: 0xe5c07b,   // yellow
        function: 0x61afef,    // blue
        number: 0xd19a66,      // orange
        operator: 0x56b6c2,    // cyan
        punctuation: 0xabb2bf, // Foreground — flat by design
        variable: 0xe06c75,    // red
        attribute: 0x61afef,
    },
};
