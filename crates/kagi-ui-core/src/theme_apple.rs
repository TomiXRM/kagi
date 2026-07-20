//! Apple Light / Apple Dark themes (ADR-0125).
//!
//! Every value is taken from Apple's Human Interface Guidelines "Color" page
//! (system colors as updated June 9 2025 — the unified Liquid Glass palette —
//! extracted from the HIG data JSON's swatch alt-texts on 2026-07-19), mapped
//! onto kagi's tokens with HIG semantics: blue = accent/links, green =
//! success, red = destructive, orange = warning.
//!
//! Policy (ADR-0125):
//! * **Dark** uses the *Default (dark)* variants — they are tuned for dark
//!   backgrounds as-is.
//! * **Light** uses the *Increased contrast (light)* variants for anything
//!   read as text or thin strokes (status colours, change badges, swimlanes —
//!   default yellow `#FFCC00` on white is illegible as a 2px lane), and the
//!   vivid *Default (light)* variants for solid ref chips (white text on a
//!   filled chip stays readable).
//! * Grays are the iOS `systemGray`..`systemGray6` ramp; secondary text is
//!   the effective (alpha-composited) label colour.

use crate::theme::Theme;

/// Apple system colors — Default (light) / Default (dark) and the
/// Increased-contrast light variants used by the light theme (HIG 2025).
pub const APPLE_LIGHT: Theme = Theme {
    slug: "apple-light",
    name: "Apple Light",
    dark: false,

    // systemBackground / systemGray6 / systemGray5 ramp.
    bg_base: 0xffffff,
    bg_row_alt: 0xf4f5f5,
    surface: 0xe5e5ea,  // systemGray5
    selected: 0xd9edff, // systemBlue 15% on white
    panel: 0xf9f9f9,    // systemGray6
    sidebar: 0xf2f2f7,  // systemGray6 (increased contrast)
    modal: 0xffffff,
    modal_overlay: 0x000000,

    text_main: 0x000000,  // label
    text_sub: 0x6c6c70,   // systemGray (increased contrast)
    text_muted: 0xaeaeb2, // systemGray2
    text_label: 0x8a8a8e, // secondaryLabel composited on white

    // Ref chips render solid with white text → vivid Default (light) set.
    color_head: 0xff2d55,   // pink
    color_branch: 0x0088ff, // blue
    color_remote: 0x34c759, // green
    color_tag: 0xff8d28,    // orange

    // Status text sits on light surfaces → Increased contrast (light) set.
    color_success: 0x008932, // green
    color_warning: 0xc55300, // orange
    color_blocker: 0xe9152d, // red
    color_blocker_muted: 0xf6a1ab,

    diff_added_bg: 0xe7f8eb,   // green 12% on white
    diff_removed_bg: 0xffe7e8, // red 12% on white
    diff_hunk: 0x1e6ef4,       // blue (increased contrast)

    change_added: 0x008932,
    change_modified: 0xa16a00, // yellow (increased contrast) — amber on white
    change_deleted: 0xe9152d,
    change_renamed: 0x1e6ef4,
    change_typechange: 0x8e8e93, // systemGray
    change_dir: 0x6c6c70,

    accent: 0xcb30e0, // purple

    // Swimlanes are thin strokes on white → Increased contrast (light) set,
    // ordered so adjacent lanes stay maximally distinct (ADR-0104 philosophy).
    lane_hsl: [
        (0.604, 0.907, 0.537), // blue   #1e6ef4
        (0.813, 0.610, 0.473), // purple #b02fc2
        (0.954, 0.855, 0.488), // pink   #e7124d
        (0.990, 1.0, 0.600),   // red    #ff383c
        (0.070, 1.0, 0.386),   // orange #c55300
        (0.110, 1.0, 0.316),   // yellow #a16a00
        (0.394, 1.0, 0.269),   // green  #008932
        (0.546, 1.0, 0.341),   // cyan   #007eae

    ],

    avatar_sat: 0.55,
    avatar_light: 0.45,

    // Terminal: white background, ANSI colours from the increased-contrast
    // set (normal) and the vivid default set (bright).
    term_bg: (0xff, 0xff, 0xff),              // #ffffff
    term_fg: (0x00, 0x00, 0x00),              // #000000
    term_cursor: (0x00, 0x00, 0x00),          // #000000
    term_black: (0x1c, 0x1c, 0x1e),           // #1c1c1e
    term_red: (0xe9, 0x15, 0x2d),             // #e9152d
    term_green: (0x00, 0x89, 0x32),           // #008932
    term_yellow: (0xa1, 0x6a, 0x00),          // #a16a00
    term_blue: (0x1e, 0x6e, 0xf4),            // #1e6ef4
    term_magenta: (0xb0, 0x2f, 0xc2),         // #b02fc2
    term_cyan: (0x00, 0x7e, 0xae),            // #007eae
    term_white: (0xc7, 0xc7, 0xcc),           // #c7c7cc
    term_bright_black: (0x8e, 0x8e, 0x93),    // #8e8e93
    term_bright_red: (0xff, 0x38, 0x3c),      // #ff383c
    term_bright_green: (0x34, 0xc7, 0x59),    // #34c759
    term_bright_yellow: (0xff, 0xcc, 0x00),   // #ffcc00
    term_bright_blue: (0x00, 0x88, 0xff),     // #0088ff
    term_bright_magenta: (0xcb, 0x30, 0xe0),  // #cb30e0
    term_bright_cyan: (0x00, 0xc0, 0xe8),     // #00c0e8
    term_bright_white: (0x00, 0x00, 0x00),    // #000000 
    term_selection: (0x00, 0x88, 0xff, 0x40), // #0088ff 25% on white
};

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
};
