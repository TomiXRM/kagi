//! The Flower Road theme.
//!
//! Built from a supplied palette: backgrounds #ffffff / #f8faff (the commit
//! list's zebra pair), label colours #d699ba / #95c0aa / #d1d48c / #000, text
//! #172540.
//!
//! The three pastel labels measure 1.1-1.6:1 against the periwinkle they came
//! with, so they cannot carry text. They stay at full strength where they are a
//! *fill* (graph lanes, diff washes) and are darkened along their own hue for
//! every text role. That split is the whole design: the palette's character
//! lives in the lane colours, its legibility in the derived text ramp.
//!
//! Selection is the #95c0aa green. It sits deeper than a straight hue-swap of
//! the periwinkle it replaced would: at equal lightness it landed 21 (channel
//! sum) from `diff_added_bg`, and with both now green there is no hue left to
//! tell "selected row" from "added line" in a diff. At L=0.82 the gap is 61 and
//! text still reads at 10.6:1 — the same as the old navy.
//!
//! Split out of `theme.rs` for the LOC ratchet, following `theme_apple`.

use crate::theme::{SyntaxPalette, Theme};

pub const FLOWER_ROAD: Theme = Theme {
    slug: "flower-road",
    name: "Flower Road",
    dark: false,

    // A warm ivory rather than white — the palette this theme is built from is
    // warm, and a blue-white base fought it.
    bg_base: 0xfff9f5, // BG1
    // BG2: the same step down from BG1, with roughly two thirds of the warmth
    // taken out. At full saturation the stripe read as an orange band rather
    // than an alternating row; the point of a zebra is to be noticed only when
    // you are following a row across.
    bg_row_alt: 0xf9f6f4,
    // `surface` is what 28 of the app's borders are drawn in as well as the
    // hover wash, so the supplied border colour goes here rather than into a
    // token of its own.
    surface: 0xeaddd8,
    selected: 0xc5ddd1, // #95c0aa at L=0.82
    // Chrome is BG1 exactly: the sidebar and tab strip are meant to read as
    // one sheet with the commit list, not as darker panels. The edges that
    // need to show are drawn as `selected`-coloured borders.
    panel: 0xfff9f5,
    sidebar: 0xfff9f5,
    modal: 0xfffcfa,
    modal_overlay: 0x172540,

    text_main: 0x172540, // supplied font colour
    text_sub: 0x3b4767,
    text_muted: 0x6a7592,
    text_label: 0x566180,

    // The accent runs pink throughout: the mode switcher, branch names, the
    // hunk rule and the text-selection tint all take `color_branch`, and it is
    // the supplied #d698ba verbatim rather than a darkened derivative. That is
    // 2.1:1 on white — chosen for the colour, not for the contrast. `color_head`
    // stays deep so HEAD still separates from the branches around it.
    color_head: 0x6f204b,   // deepest — HEAD is the anchor
    color_branch: 0xd698ba, // the supplied pink, verbatim
    // Not the pink accent: at 30% over a context line that lands 12 (channel
    // sum) from `diff_removed_bg`, so selecting a context line in a diff looked
    // exactly like a removed line. The palette's olive is the one label colour
    // that collides with neither diff wash — and reads as a highlighter.
    selection_tint: 0xb8bc4e,
    color_remote: 0x318259, // green label, darkened
    color_tag: 0x70741b,    // olive label, darkened

    color_success: 0x318259,
    color_warning: 0x8a5a10,
    // This palette has no red: the "red" family is the #d699ba pink, darkened
    // to 4.5:1 so a refusal still reads as one. It shares a hue with
    // `color_head` by necessity — the two are separated by lightness, not hue.
    color_blocker: 0xc73d88, // brightest of the three — an alarm should shout
    color_blocker_muted: 0xdfa6c6,

    diff_added_bg: 0xdfece5,   // #95c0aa at L=0.90
    diff_removed_bg: 0xf0dbe6, // #d699ba at L=0.90
    diff_hunk: 0xd698ba,

    change_added: 0x318259,
    change_modified: 0x8a5a10,
    change_deleted: 0xc73d88,
    change_renamed: 0x2e549e,
    change_typechange: 0x87419f,
    change_dir: 0x566180,

    accent: 0xd698ba, // matches color_branch

    // The palette's own hues at a lightness that reads on the base (every lane
    // >= 4.4:1), ordered so adjacent indices stay distinct. Lane 7 is the
    // supplied #000 — the one label colour that needed no adjustment.
    // Flower Road's swimlanes. Ordered so neighbouring lane indices are as far
    // apart as the set allows — worst adjacent pair 0.39 on a hue+chroma
    // distance, where the naive order left Silver next to Lavender at 0.02.
    // The supplied palette had no blue at all (hues 0.28-0.73 were empty),
    // which is why both added flowers are blue, and why they differ in
    // lightness so they separate from each other too.
    //
    // Used verbatim: they run 1.4-2.3:1 on the ivory, against the 3.5-6.5 the
    // other light themes' lanes sit at. That is a deliberate trade of
    // legibility for the palette, not an oversight.
    lane_hsl: [
        (0.733, 0.490, 0.800), // Lavender     #c7b3e5  1.83:1
        (0.106, 0.768, 0.594), // Marigold     #e7ad48  1.92:1
        (0.610, 0.483, 0.704), // Tsuyukusa    #8fa8d8  2.30:1
        (0.269, 0.153, 0.667), // Sage Green   #a7b79d  2.03:1
        (0.963, 0.496, 0.751), // Rose         #dfa0ae  2.06:1
        (0.750, 0.056, 0.788), // Silver       #c9c6cc  1.62:1
        (0.590, 0.513, 0.775), // Wasurenagusa #a8c3e3  1.74:1
        (0.147, 0.723, 0.661), // Nanohana     #e7d86a  1.39:1
    ],

    avatar_sat: 0.42,
    avatar_light: 0.46,

    term_bg: (0xff, 0xf9, 0xf5),
    term_fg: (0x17, 0x25, 0x40),
    term_cursor: (0xa5, 0x27, 0x6b),
    term_black: (0x17, 0x25, 0x40),
    term_red: (0xc7, 0x3d, 0x88),
    term_green: (0x31, 0x82, 0x59),
    term_yellow: (0x70, 0x74, 0x1b),
    term_blue: (0x2e, 0x54, 0x9e),
    term_magenta: (0xa5, 0x27, 0x6b),
    term_cyan: (0x17, 0x82, 0x82),
    term_white: (0x56, 0x61, 0x80),
    term_bright_black: (0x6a, 0x75, 0x92),
    term_bright_red: (0xd6, 0x99, 0xba),
    term_bright_green: (0x95, 0xc0, 0xaa),
    term_bright_yellow: (0x94, 0x99, 0x2c),
    term_bright_blue: (0x44, 0x6c, 0xbd),
    term_bright_magenta: (0xc6, 0x3f, 0x89),
    term_bright_cyan: (0x24, 0xa0, 0xa0),
    term_bright_white: (0x17, 0x25, 0x40),
    term_selection: (0x2e, 0x54, 0x9e, 0x40),

    // Each token takes one of the palette's hues, darkened well clear of the
    // 3.0:1 floor (measured: lowest is `variable` at 4.4:1). Operators and
    // punctuation are the plain foreground — flat by choice, matching how the
    // supplied palette has no colour to spare for them.
    syntax: SyntaxPalette {
        keyword: 0xa5276b,     // pink
        string: 0x318259,      // green
        comment: 0x6a7592,     // muted; meant to recede
        type_name: 0x70741b,   // olive
        function: 0x2e549e,    // blue
        number: 0x9c4e1c,      // orange
        operator: 0x172540,    // foreground
        punctuation: 0x172540, // foreground
        variable: 0x178282,    // teal
        attribute: 0x87419f,   // purple
    },
};
