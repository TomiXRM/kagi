//! The pinky boo theme.

use crate::theme::{SyntaxPalette, Theme, LANE_PALETTE_LIGHT};

pub const PINKY_BOO: Theme = Theme {
    slug: "pinky-boo",
    name: "Pinky Boo",
    dark: false,

    bg_base: 0xfbfbfb,
    bg_row_alt: 0xf3f3f3,
    surface: 0xf6eef5,
    selected: 0xffafeb,
    panel: 0xf3f3f3,
    sidebar: 0xefefef,
    modal: 0xffffff,
    modal_overlay: 0x5a5a5a,

    text_main: 0x5a5a5a,
    text_sub: 0x6e6e6e,
    text_muted: 0xa0a0a0,
    text_label: 0x909090,

    color_head: 0xff398d,   // hot pink
    color_branch: 0x47b0e6, // blue
    selection_tint: 0x47b0e6,
    color_remote: 0x587c0c, // olive green
    color_tag: 0xd56700,    // orange

    color_success: 0x587c0c,
    color_warning: 0x895503,
    color_blocker: 0xad0707,
    color_blocker_muted: 0xd99a9a,

    diff_added_bg: 0xe8f0d0,
    diff_removed_bg: 0xfde0e0,
    diff_hunk: 0x47b0e6,

    change_added: 0x587c0c,
    change_modified: 0x895503,
    change_deleted: 0xad0707,
    change_renamed: 0x47b0e6,
    change_typechange: 0xa0a0a0,
    change_dir: 0x909090,

    accent: 0xff398d, // hot pink

    lane_hsl: LANE_PALETTE_LIGHT,

    avatar_sat: 0.50,
    avatar_light: 0.50,

    term_bg: (0xfb, 0xfb, 0xfb),
    term_fg: (0x7d, 0x7d, 0x7d),
    term_cursor: (0xf8, 0xae, 0xf0),
    term_black: (0x74, 0x72, 0x73),
    term_red: (0xcd, 0x31, 0x31),
    term_green: (0x00, 0xbc, 0x00),
    term_yellow: (0xf0, 0xe4, 0x3b),
    term_blue: (0x7f, 0xb8, 0xf5),
    term_magenta: (0xff, 0x13, 0xb9),
    term_cyan: (0x05, 0x98, 0xbc),
    term_white: (0xff, 0xaf, 0xeb),
    term_bright_black: (0xb3, 0xb3, 0xb3),
    term_bright_red: (0xe2, 0x55, 0x55),
    term_bright_green: (0x14, 0xce, 0x14),
    term_bright_yellow: (0xeb, 0xc1, 0x3f),
    term_bright_blue: (0x7f, 0xb3, 0xec),
    term_bright_magenta: (0xff, 0x71, 0xe2),
    term_bright_cyan: (0x05, 0x98, 0xbc),
    term_bright_white: (0xa5, 0xa5, 0xa5),
    term_selection: (0xc9, 0xc9, 0xc9, 0x40),

    // Code colours: Pinky Boo (kissa1001/pinky-boo-vscode-theme, a light
    // One-Dark-Pro derivative). Its `keyword.operator` catch-all is the plain
    // foreground; per-language sub-scopes vary, which we don't model.
    //
    // Darkened from upstream, hue and saturation preserved. Pinky Boo inherits
    // several token colours unchanged from its DARK ancestor (One Dark's
    // string `#98c379`, function `#47b0e6`), which on this theme's near-white
    // `#fbfbfb` background wash out to the point of illegibility — string
    // measured 1.9:1 contrast, i.e. barely visible (user report: "text goes
    // white"). Each value here is the upstream hue taken down to >= 4.0:1.
    syntax: SyntaxPalette {
        keyword: 0x138a82,     // upstream 0x1bc5b9 (2.1:1)
        string: 0x5c873d,      // upstream 0x98c379 (1.9:1)
        comment: 0x7f848e,     // left as-is: comments are meant to recede
        type_name: 0xc45f00,   // upstream 0xd56700 (3.5:1)
        function: 0x1983b9,    // upstream 0x47b0e6 (2.4:1)
        number: 0xac6e34,      // upstream 0xd19a66 (2.4:1)
        operator: 0x5a5a5a,    // Foreground — flat by design
        punctuation: 0x5a5a5a, // Foreground
        variable: 0xf30067,    // upstream 0xff398d (3.3:1)
        attribute: 0xac6e34,   // upstream 0xd19a66 (2.4:1)
    },
};
