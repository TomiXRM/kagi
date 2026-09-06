//! The tokyo night theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_DARK};

pub const TOKYO_NIGHT: Theme = Theme {
    slug: "tokyo-night",
    name: "Tokyo Night",
    dark: true,

    bg_base: 0x1a1b26,
    bg_row_alt: 0x16161e,
    surface: 0x292e42,
    selected: 0x283457,
    panel: 0x16161e,
    sidebar: 0x13131a,
    modal: 0x24283b,
    modal_overlay: 0x000000,

    text_main: 0xc0caf5,
    text_sub: 0xa9b1d6,
    text_muted: 0x565f89,
    text_label: 0x9aa5ce,

    color_head: 0xf7768e,
    color_branch: 0x7aa2f7,
    selection_tint: 0x7aa2f7,
    color_remote: 0x9ece6a,
    color_tag: 0xe0af68,

    color_success: 0x9ece6a,
    color_warning: 0xe0af68,
    color_blocker: 0xf7768e,
    color_blocker_muted: 0x7a4250,

    diff_added_bg: 0x1f3328,
    diff_removed_bg: 0x3a1f28,
    diff_hunk: 0x7aa2f7,

    change_added: 0x9ece6a,
    change_modified: 0xe0af68,
    change_deleted: 0xf7768e,
    change_renamed: 0x7aa2f7,
    change_typechange: 0x565f89,
    change_dir: 0x7dcfff,

    accent: 0xbb9af7, // magenta

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.65,
    avatar_light: 0.65,

    term_bg: (0x1a, 0x1b, 0x26),
    term_fg: (0xc0, 0xca, 0xf5),
    term_cursor: (0xc0, 0xca, 0xf5),
    term_black: (0x15, 0x16, 0x1e),
    term_red: (0xf7, 0x76, 0x8e),
    term_green: (0x9e, 0xce, 0x6a),
    term_yellow: (0xe0, 0xaf, 0x68),
    term_blue: (0x7a, 0xa2, 0xf7),
    term_magenta: (0xbb, 0x9a, 0xf7),
    term_cyan: (0x7d, 0xcf, 0xff),
    term_white: (0xa9, 0xb1, 0xd6),
    term_bright_black: (0x41, 0x48, 0x68),
    term_bright_red: (0xf7, 0x76, 0x8e),
    term_bright_green: (0x9e, 0xce, 0x6a),
    term_bright_yellow: (0xe0, 0xaf, 0x68),
    term_bright_blue: (0x7a, 0xa2, 0xf7),
    term_bright_magenta: (0xbb, 0x9a, 0xf7),
    term_bright_cyan: (0x7d, 0xcf, 0xff),
    term_bright_white: (0xc0, 0xca, 0xf5),
    term_selection: (0x28, 0x34, 0x57, 0xb3),

    // Code colours: Tokyo Night (main dark variant). Upstream deliberately
    // shares one rule for operators and punctuation.
    syntax: SyntaxPalette {
        keyword: 0xbb9af7, // purple
        string: 0x9ece6a,  // green
        comment: 0x51597d,
        type_name: 0x0db9d7, // cyan
        function: 0x7aa2f7,  // blue
        number: 0xff9e64,    // orange
        operator: 0x89ddff,
        punctuation: 0x89ddff, // shares the operator rule upstream
        variable: 0xc0caf5,
        attribute: 0x7aa2f7,
    },
};
