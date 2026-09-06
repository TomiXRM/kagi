//! The Apple Light theme (ADR-0125).

use crate::theme::{SyntaxPalette, Theme};

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
    selection_tint: 0x0088ff,
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

    // Code colours: Xcode's own "Default (Light)" theme (T-SYNTAX-001).
    // Same note as Apple Dark: operators/punctuation are plain by design.
    syntax: SyntaxPalette {
        keyword: 0x9b2393,
        string: 0xc41a16,
        comment: 0x5d6c79,
        type_name: 0x1c464a,
        function: 0x326d74,
        number: 0x1c00cf,
        operator: 0x000000,
        punctuation: 0x000000,
        variable: 0x326d74,
        attribute: 0x815f03,
    },
};
