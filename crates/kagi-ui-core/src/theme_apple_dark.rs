//! The Apple Dark theme (ADR-0125).

use crate::theme::{SyntaxPalette, Theme};

pub const APPLE_DARK: Theme = Theme {
    slug: "apple-dark",
    name: "Apple Dark",
    dark: true,

    // systemGray6-dark ramp: base #1c1c1e, chrome one step below, chips above.
    bg_base: 0x1c1c1e,
    bg_row_alt: 0x18181a,
    surface: 0x2c2c2e,  // systemGray5 (dark)
    selected: 0x3a3a3c, // systemGray4 (dark) — neutral selection, Notes-style
    panel: 0x161618,
    sidebar: 0x101012,
    modal: 0x2c2c2e,
    modal_overlay: 0x000000,

    text_main: 0xffffff,  // label (dark)
    text_sub: 0x98989f,   // secondaryLabel composited on #1c1c1e
    text_muted: 0x636366, // systemGray2 (dark)
    text_label: 0x7c7c80, // systemGray2 (dark, increased contrast)

    // Default (dark) set — tuned for dark backgrounds as-is. `color_branch`
    // doubles as kagi's UI accent (primary buttons, active tab, links);
    // Apple's dark-mode apps (Camera, Notes) accent with systemYellow, so
    // the dark theme adopts it (ADR-0126) — the light theme keeps blue.
    color_head: 0xff375f,   // pink
    color_branch: 0xffd600, // yellow (accent)
    selection_tint: 0xffd600,
    color_remote: 0x30d158, // green
    color_tag: 0xff9230,    // orange

    color_success: 0x30d158,
    color_warning: 0xff9230, // orange (HIG warning semantics; yellow = accent)
    color_blocker: 0xff4245, // red
    color_blocker_muted: 0x822d30,

    diff_added_bg: 0x1f3927,   // green 16% on #1c1c1e
    diff_removed_bg: 0x402224, // red 16% on #1c1c1e
    diff_hunk: 0x5cb8ff,       // blue (increased contrast, dark)

    change_added: 0x30d158,
    change_modified: 0xffd600,
    change_deleted: 0xff4245,
    change_renamed: 0x0091ff,
    change_typechange: 0x8e8e93, // systemGray
    change_dir: 0x98989f,

    accent: 0xdb34f2, // purple

    // Default (dark) vivids, same adjacency ordering as the light palette.
    lane_hsl: [
        (0.140, 1.0, 0.500),   // yellow #ffd600
        (0.375, 0.636, 0.504), // green  #30d158
        (0.537, 0.990, 0.616), // cyan   #3cd3fe
        (0.572, 1.0, 0.500),   // blue   #0091ff
        (0.813, 0.880, 0.576), // purple #db34f2
        (0.967, 1.0, 0.608),   // pink   #ff375f
        (0.99, 1.0, 0.62),     // red    #ff4245
        (0.079, 1.0, 0.594),   // orange #ff9230
    ],

    avatar_sat: 0.70,
    avatar_light: 0.60,

    // Terminal: #1c1c1e background, Default (dark) ANSI colours with the
    // Increased-contrast (dark) set as the bright variants.
    term_bg: (0x1c, 0x1c, 0x1e),              // #1c1c1e
    term_fg: (0xff, 0xff, 0xff),              // #ffffff
    term_cursor: (0xff, 0xd6, 0x00),          // #ffd600
    term_black: (0x2c, 0x2c, 0x2e),           // #2c2c2e
    term_red: (0xff, 0x42, 0x45),             // #ff4245
    term_green: (0x30, 0xd1, 0x58),           // #30d158
    term_yellow: (0xff, 0xd6, 0x00),          // #ffd600
    term_blue: (0x00, 0x91, 0xff),            // #0091ff
    term_magenta: (0xdb, 0x34, 0xf2),         // #db34f2
    term_cyan: (0x00, 0xd2, 0xe0),            // #00d2e0
    term_white: (0xc7, 0xc7, 0xcc),           // #c7c7cc
    term_bright_black: (0x63, 0x63, 0x66),    // #636366
    term_bright_red: (0xff, 0x61, 0x65),      // #ff6165
    term_bright_green: (0x4a, 0xd9, 0x68),    // #4ad968
    term_bright_yellow: (0xfe, 0xdf, 0x43),   // #fddf43
    term_bright_blue: (0x5c, 0xb8, 0xff),     // #5cb8ff
    term_bright_magenta: (0xea, 0x8d, 0xff),  // #ea8dff
    term_bright_cyan: (0x6d, 0xd9, 0xff),     // #6dd9ff
    term_bright_white: (0xff, 0xff, 0xff),    // #ffffff
    term_selection: (0x00, 0x91, 0xff, 0x66), // #0091ff 40% on #1c1c1e

    // Code colours: Xcode's own "Default (Dark)" theme (T-SYNTAX-001).
    // Xcode assigns operators and punctuation no colour of their own, so they
    // deliberately stay at the plain editor foreground rather than being
    // invented here. `type`/`function` use Xcode's *project* symbol colours
    // (it also has separate system-symbol colours, a distinction a TextMate
    // grammar can't make).
    syntax: SyntaxPalette {
        keyword: 0xfc5fa3,
        string: 0xfc6a5d,
        comment: 0x6c7986,
        type_name: 0x9ef1dd,
        function: 0x67b7a4,
        number: 0xd0bf69,
        operator: 0xffffff,
        punctuation: 0xffffff,
        variable: 0x67b7a4,
        attribute: 0xbf8555,
    },
};
