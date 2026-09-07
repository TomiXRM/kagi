//! The ibm pc theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const IBM_PC: Theme = Theme {
    slug: "ibm-pc",
    name: "IBM PC",
    dark: true,

    bg_base: 0x000000,
    bg_row_alt: 0x0a0a0a,
    surface: 0x222222,
    selected: 0x0000aa, // the classic DOS blue highlight bar
    panel: 0x000000,
    sidebar: 0x000000,
    modal: 0x0000aa, // DOS blue dialog
    modal_overlay: 0x000000,

    text_main: 0xffffff,
    text_sub: 0xaaaaaa,
    text_muted: 0x555555,
    text_label: 0x55ffff,

    color_head: 0xff5555,
    color_branch: 0x5555ff,
    selection_tint: 0x5555ff,
    color_remote: 0x55ff55,
    color_tag: 0xffff55,

    color_success: 0x55ff55,
    color_warning: 0xffff55,
    color_blocker: 0xff5555,
    color_blocker_muted: 0xaa0000,

    diff_added_bg: 0x003300,
    diff_removed_bg: 0x330000,
    diff_hunk: 0x55ffff,

    change_added: 0x55ff55,
    change_modified: 0xffff55,
    change_deleted: 0xff5555,
    change_renamed: 0x55ffff,
    change_typechange: 0x555555,
    change_dir: 0x5555ff,

    accent: 0xff55ff, // bright magenta

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 1.0,
    avatar_light: 0.60,

    // Exact CGA 16-colour palette.
    term_bg: (0x00, 0x00, 0x00),
    term_fg: (0xaa, 0xaa, 0xaa),
    term_cursor: (0xff, 0xff, 0xff),
    term_black: (0x00, 0x00, 0x00),
    term_red: (0xaa, 0x00, 0x00),
    term_green: (0x00, 0xaa, 0x00),
    term_yellow: (0xaa, 0x55, 0x00), // brown
    term_blue: (0x00, 0x00, 0xaa),
    term_magenta: (0xaa, 0x00, 0xaa),
    term_cyan: (0x00, 0xaa, 0xaa),
    term_white: (0xaa, 0xaa, 0xaa),
    term_bright_black: (0x55, 0x55, 0x55),
    term_bright_red: (0xff, 0x55, 0x55),
    term_bright_green: (0x55, 0xff, 0x55),
    term_bright_yellow: (0xff, 0xff, 0x55),
    term_bright_blue: (0x55, 0x55, 0xff),
    term_bright_magenta: (0xff, 0x55, 0xff),
    term_bright_cyan: (0x55, 0xff, 0xff),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x00, 0x00, 0xaa, 0xb3),

    // Code colours: CONSTRUCTED, not ported — no IBM PC syntax theme exists.
    // Every value is an exact entry from the standard CGA/EGA 16-colour
    // hardware palette (Turbo-Pascal-flavoured); only the token->colour
    // assignment is ours, so it is free to be reshuffled to taste.
    syntax: SyntaxPalette {
        keyword: 0xffff55,     // bright yellow
        string: 0x55ffff,      // bright cyan
        comment: 0x555555,     // dark gray
        type_name: 0x55ff55,   // bright green
        function: 0xff55ff,    // bright magenta
        number: 0xff5555,      // bright red
        operator: 0xffffff,    // white
        punctuation: 0xaaaaaa, // light gray (foreground)
        variable: 0xaaaaaa,    // light gray
        attribute: 0xaa5500,   // brown
    },
};
