//! The catppuccin mocha theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const CATPPUCCIN_MOCHA: Theme = Theme {
    slug: "catppuccin",
    name: "Catppuccin Mocha",
    dark: true,

    bg_base: 0x1e1e2e,
    bg_row_alt: 0x1a1a2a,
    surface: 0x313244,
    selected: 0x45475a,
    panel: 0x181825,
    sidebar: 0x11111b,
    modal: 0x313244,
    modal_overlay: 0x000000,

    text_main: 0xcdd6f4,
    text_sub: 0xa6adc8,
    text_muted: 0x585b70,
    text_label: 0x6c7086,

    color_head: 0xf38ba8,
    color_branch: 0x89b4fa,
    selection_tint: 0x89b4fa,
    color_remote: 0xa6e3a1,
    color_tag: 0xfab387,

    color_success: 0xa6e3a1,
    color_warning: 0xf9e2af,
    color_blocker: 0xf38ba8,
    color_blocker_muted: 0x8f5360,

    diff_added_bg: 0x1c3a2a,
    diff_removed_bg: 0x3a1c1c,
    diff_hunk: 0x89b4fa,

    change_added: 0xa6e3a1,
    change_modified: 0xf9e2af,
    change_deleted: 0xf38ba8,
    change_renamed: 0x89b4fa,
    change_typechange: 0x585b70,
    change_dir: 0x6c7086,

    accent: 0xcba6f7, // mauve

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.70,
    avatar_light: 0.60,

    term_bg: (0x1e, 0x1e, 0x2e),
    term_fg: (0xcd, 0xd6, 0xf4),
    term_cursor: (0xf5, 0xc2, 0xe7),
    term_black: (0x45, 0x47, 0x5a),
    term_red: (0xf3, 0x8b, 0xa8),
    term_green: (0xa6, 0xe3, 0xa1),
    term_yellow: (0xf9, 0xe2, 0xaf),
    term_blue: (0x89, 0xb4, 0xfa),
    term_magenta: (0xcb, 0xa6, 0xf7),
    term_cyan: (0x89, 0xdc, 0xeb),
    term_white: (0xba, 0xc2, 0xde),
    term_bright_black: (0x58, 0x5b, 0x70),
    term_bright_red: (0xf3, 0x8b, 0xa8),
    term_bright_green: (0xa6, 0xe3, 0xa1),
    term_bright_yellow: (0xf9, 0xe2, 0xaf),
    term_bright_blue: (0x89, 0xb4, 0xfa),
    term_bright_magenta: (0xcb, 0xa6, 0xf7),
    term_bright_cyan: (0x89, 0xdc, 0xeb),
    term_bright_white: (0xcd, 0xd6, 0xf4),
    term_selection: (0x58, 0x5b, 0x70, 0x99),

    // Code colours: Catppuccin Mocha, roles per the project's own style guide
    // ("Language Defaults"). Named palette entries in comments so the mapping
    // stays auditable against upstream.
    syntax: SyntaxPalette {
        keyword: 0xcba6f7,     // Mauve
        string: 0xa6e3a1,      // Green
        comment: 0x9399b2,     // Overlay 2
        type_name: 0xf9e2af,   // Yellow
        function: 0x89b4fa,    // Blue
        number: 0xfab387,      // Peach
        operator: 0x89dceb,    // Sky
        punctuation: 0x9399b2, // Overlay 2
        variable: 0xeba0ac,    // Maroon
        attribute: 0xf9e2af,   // Yellow
    },
};
