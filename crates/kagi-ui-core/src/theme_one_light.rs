//! The one light theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_LIGHT};

pub const ONE_LIGHT: Theme = Theme {
    slug: "one-light",
    name: "One Light",
    dark: false,

    bg_base: 0xfafafa,
    bg_row_alt: 0xf0f0f1,
    surface: 0xeaeaeb,
    selected: 0xd4e2fb,
    panel: 0xf0f0f0,
    sidebar: 0xeaeaeb,
    modal: 0xffffff,
    modal_overlay: 0x383a42,

    text_main: 0x383a42,
    text_sub: 0x4f525e,
    text_muted: 0x9d9d9f,
    text_label: 0x7a7c85,

    color_head: 0xe45649,   // red
    color_branch: 0x4078f2, // blue
    selection_tint: 0x4078f2,
    color_remote: 0x50a14f, // green
    color_tag: 0xc18401,    // amber

    color_success: 0x50a14f,
    color_warning: 0xb07a00,
    color_blocker: 0xe45649,
    color_blocker_muted: 0xc88a83,

    diff_added_bg: 0xddf3df,
    diff_removed_bg: 0xfbdedb,
    diff_hunk: 0x4078f2,

    change_added: 0x50a14f,
    change_modified: 0xb07a00,
    change_deleted: 0xe45649,
    change_renamed: 0x4078f2,
    change_typechange: 0x9d9d9f,
    change_dir: 0x7a7c85,

    accent: 0xa626a4, // purple

    lane_hsl: LANE_PALETTE_LIGHT,

    avatar_sat: 0.50,
    avatar_light: 0.48,

    term_bg: (0xfa, 0xfa, 0xfa),
    term_fg: (0x38, 0x3a, 0x42),
    term_cursor: (0x52, 0x6f, 0xff),
    term_black: (0x38, 0x3a, 0x42),
    term_red: (0xe4, 0x56, 0x49),
    term_green: (0x50, 0xa1, 0x4f),
    term_yellow: (0xc1, 0x84, 0x01),
    term_blue: (0x40, 0x78, 0xf2),
    term_magenta: (0xa6, 0x26, 0xa4),
    term_cyan: (0x01, 0x84, 0xbc),
    term_white: (0xa0, 0xa1, 0xa7),
    term_bright_black: (0x69, 0x6c, 0x77),
    term_bright_red: (0xe4, 0x56, 0x49),
    term_bright_green: (0x50, 0xa1, 0x4f),
    term_bright_yellow: (0xc1, 0x84, 0x01),
    term_bright_blue: (0x40, 0x78, 0xf2),
    term_bright_magenta: (0xa6, 0x26, 0xa4),
    term_bright_cyan: (0x01, 0x84, 0xbc),
    term_bright_white: (0x38, 0x3a, 0x42),
    term_selection: (0xc6, 0xd8, 0xf7, 0xcc),

    // Code colours: Atom One Light.
    syntax: SyntaxPalette {
        keyword: 0xa626a4,
        string: 0x50a14f,
        comment: 0xa0a1a7,
        type_name: 0xc18401,
        function: 0x4078f2,
        number: 0x986801,
        operator: 0x0184bc,
        punctuation: 0x383a42, // Foreground — flat by design
        variable: 0xe45649,
        attribute: 0x986801,
    },
};
