//! The monokai theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const MONOKAI: Theme = Theme {
    slug: "monokai",
    name: "Monokai (Warm Hybrid)",
    dark: true,

    bg_base: 0x28242a,
    bg_row_alt: 0x221e24,
    surface: 0x3a3540,
    selected: 0x4a4454,
    panel: 0x1f1b21,
    sidebar: 0x1a161c,
    modal: 0x3a3540,
    modal_overlay: 0x000000,

    text_main: 0xf0ece8,
    text_sub: 0xb8b4bc,
    text_muted: 0x918d94,
    text_label: 0xa09ca3,

    color_head: 0xff3d6f,   // vivid pink (ref keyword #ff668c, boosted)
    color_branch: 0x5a9fff, // vivid blue
    selection_tint: 0x5a9fff,
    color_remote: 0xa8e05a, // vivid green (ref function #a4d671)
    color_tag: 0xff8c1a,    // punchy warm orange

    color_success: 0xa8e05a,
    color_warning: 0xf4cd62, // matches ref string yellow #f4cd62
    color_blocker: 0xff3d6f,
    color_blocker_muted: 0x8f4a5e,

    diff_added_bg: 0x253520,
    diff_removed_bg: 0x35202c,
    diff_hunk: 0x5a9fff,

    change_added: 0xa8e05a,
    change_modified: 0xf4cd62,
    change_deleted: 0xff3d6f,
    change_renamed: 0x5a9fff,
    change_typechange: 0x918d94,
    change_dir: 0xa09ca3,

    accent: 0xb08fff, // vivid purple (ref #af9cf4, boosted)

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.68,
    avatar_light: 0.62,

    term_bg: (0x28, 0x24, 0x2a),
    term_fg: (0xf0, 0xec, 0xe8),
    term_cursor: (0xff, 0x8c, 0x1a),
    term_black: (0x3a, 0x35, 0x40),
    term_red: (0xff, 0x3d, 0x6f),
    term_green: (0xa8, 0xe0, 0x5a),
    term_yellow: (0xf4, 0xcd, 0x62),
    term_blue: (0x5a, 0x9f, 0xff),
    term_magenta: (0xb0, 0x8f, 0xff),
    term_cyan: (0x7b, 0xe8, 0xf5),
    term_white: (0xff, 0xfd, 0xf8),
    term_bright_black: (0x91, 0x8d, 0x94),
    term_bright_red: (0xff, 0x70, 0x96),
    term_bright_green: (0xbf, 0xed, 0x78),
    term_bright_yellow: (0xf8, 0xdf, 0x80),
    term_bright_blue: (0x80, 0xb8, 0xff),
    term_bright_magenta: (0xcc, 0xb4, 0xff),
    term_bright_cyan: (0xa0, 0xf0, 0xff),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x5a, 0x53, 0x62, 0xb3),

    // Code colours: classic Monokai as shipped in VS Code's built-in
    // theme-monokai. Operators share the keyword rule; punctuation is plain.
    syntax: SyntaxPalette {
        keyword: 0xf92672,
        string: 0xe6db74,
        comment: 0x88846f, // VS Code port (the original .tmTheme is #75715e)
        type_name: 0x66d9ef,
        function: 0xa6e22e,
        number: 0xae81ff,
        operator: 0xf92672,    // same rule as keyword upstream
        punctuation: 0xf8f8f2, // Foreground — flat by design
        variable: 0xf8f8f2,
        attribute: 0xa6e22e,
    },
};
