//! W9-THEME / ADR-0036: single-source colour theme registry.
//!
//! All UI colour comes from the active [`Theme`].  Modules call [`theme()`]
//! (a `&'static Theme`) every render frame, so switching a theme is just an
//! atomic index update + `cx.notify()` — no signature churn anywhere.
//!
//! # Design
//!
//! * [`Theme`] holds **semantic** `u32` RGB fields (e.g. `bg_base`, `text_main`,
//!   `color_branch`) plus a few non-RGB values (lane HSLA palette, avatar
//!   saturation/lightness, terminal selection alpha) and a `dark: bool` flag.
//! * [`THEMES`] lists the built-in themes; index 0 (Catppuccin Mocha) is the
//!   default and a byte-exact port of the previously hard-coded constants, so
//!   the default look has zero regression.
//! * [`ACTIVE`] is an `AtomicUsize` index into [`THEMES`].  [`set_active`]
//!   updates it (and persists to `settings.json`); [`theme()`] reads it.
//!
//! # Persistence
//!
//! The active theme slug is stored in `~/.kagi/settings.json` (hand-written
//! JSON, no serde — same approach as `oplog.rs`), honouring `KAGI_LOG_DIR`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use gpui::{hsla, rgb, App, Hsla};

use crate::settings::{read_setting, write_setting, Settings};

// Compatibility re-export: font constants historically lived in `theme`.
pub use crate::fonts::{ui_font, CJK_FONT, MONO_FONT, UI_FONT};

// ──────────────────────────────────────────────────────────────────────────
// Theme struct
// ──────────────────────────────────────────────────────────────────────────

/// A complete colour theme.  All colour fields are `0xRRGGBB` `u32` (consumed
/// by `gpui::rgb`) except the lane palette (HSLA), the avatar saturation /
/// lightness scalars, and the terminal selection alpha.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Stable lowercase slug used for menus, settings, and `KAGI_THEME`.
    pub slug: &'static str,
    /// Human-readable name shown in the View → Theme menu.
    pub name: &'static str,
    /// Whether this is a dark theme (drives diff highlight + alpha choices).
    pub dark: bool,

    // ── Backgrounds ──────────────────────────────────────────────
    /// Window / commit-list base background.
    pub bg_base: u32,
    /// Alternate (zebra) commit-row background.
    pub bg_row_alt: u32,
    /// Surface (chips, hover, modal body).
    pub surface: u32,
    /// Selected-row highlight.
    pub selected: u32,
    /// Detail panel / tab strip background (mantle).
    pub panel: u32,
    /// Sidebar background (crust).
    pub sidebar: u32,
    /// Modal background.
    pub modal: u32,
    /// Full-screen modal scrim (alpha applied at the call site).
    pub modal_overlay: u32,

    // ── Text ─────────────────────────────────────────────────────
    pub text_main: u32,
    pub text_sub: u32,
    pub text_muted: u32,
    /// Field labels in the detail panel.
    pub text_label: u32,

    // ── Ref / decoration colours ─────────────────────────────────
    pub color_head: u32,
    pub color_branch: u32,
    pub color_remote: u32,
    pub color_tag: u32,

    // ── Status text ──────────────────────────────────────────────
    pub color_success: u32,
    pub color_warning: u32,
    pub color_blocker: u32,
    /// Muted/dimmed blocker colour for disabled-but-dangerous menu items.
    pub color_blocker_muted: u32,

    // ── Diff display ─────────────────────────────────────────────
    pub diff_added_bg: u32,
    pub diff_removed_bg: u32,
    pub diff_hunk: u32,

    // ── File change-kind badges ──────────────────────────────────
    pub change_added: u32,
    pub change_modified: u32,
    pub change_deleted: u32,
    pub change_renamed: u32,
    pub change_typechange: u32,
    pub change_dir: u32,

    // ── Accent buttons ───────────────────────────────────────────
    /// Cherry-pick action button (Catppuccin mauve).
    pub accent: u32,

    // ── Graph lane palette (8 cycling colours, HSLA components) ───
    /// `(hue, saturation, lightness)` for each lane; alpha is always 1.0.
    pub lane_hsl: [(f32, f32, f32); 8],

    // ── Avatar fixed saturation / lightness ──────────────────────
    pub avatar_sat: f32,
    pub avatar_light: f32,

    // ── Terminal palette (RGB triples + selection RGBA) ──────────
    pub term_bg: (u8, u8, u8),
    pub term_fg: (u8, u8, u8),
    pub term_cursor: (u8, u8, u8),
    pub term_black: (u8, u8, u8),
    pub term_red: (u8, u8, u8),
    pub term_green: (u8, u8, u8),
    pub term_yellow: (u8, u8, u8),
    pub term_blue: (u8, u8, u8),
    pub term_magenta: (u8, u8, u8),
    pub term_cyan: (u8, u8, u8),
    pub term_white: (u8, u8, u8),
    pub term_bright_black: (u8, u8, u8),
    pub term_bright_red: (u8, u8, u8),
    pub term_bright_green: (u8, u8, u8),
    pub term_bright_yellow: (u8, u8, u8),
    pub term_bright_blue: (u8, u8, u8),
    pub term_bright_magenta: (u8, u8, u8),
    pub term_bright_cyan: (u8, u8, u8),
    pub term_bright_white: (u8, u8, u8),
    /// Terminal selection highlight RGBA.
    pub term_selection: (u8, u8, u8, u8),

    /// Per-theme code colours (T-SYNTAX-001). Before this existed every theme
    /// shared gpui-component's bundled palette, picked by `dark` alone — so
    /// Apple Dark and Catppuccin Mocha highlighted identically (user report).
    pub syntax: SyntaxPalette,
}

/// The ten code-token colours a theme defines, expanded to gpui-component's
/// full ~42-entry `SyntaxColors` by [`syntax_theme_json`].
///
/// Ten rather than forty-two because that is the honest granularity: the
/// upstream palettes these are ported from distinguish roughly this many
/// roles, and the rest are aliases of them (`boolean` is a `number`, `enum` is
/// a `type`, …). Deriving keeps the theme table maintainable and stops the table
/// filling with repeats.
///
/// Where an upstream theme deliberately does NOT colour a role — Xcode gives
/// operators and punctuation no colour at all, and several dark themes leave
/// punctuation plain — set it to the theme's own `text_main`. Flat is the
/// design there; inventing a colour would misrepresent the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxPalette {
    /// `fn`, `let`, `if`, `pub`, `impl`, `return`…
    pub keyword: u32,
    /// String literals (and, derived, escapes / regex).
    pub string: u32,
    /// Line and block comments; rendered italic.
    pub comment: u32,
    /// Type names, structs, enums, primitives.
    pub type_name: u32,
    /// Function and method names.
    pub function: u32,
    /// Numeric and boolean literals.
    pub number: u32,
    /// `+ - = == => -> &` …
    pub operator: u32,
    /// Braces, brackets, commas, semicolons.
    pub punctuation: u32,
    /// Plain identifiers, locals, parameters.
    pub variable: u32,
    /// Rust `#[derive(…)]`, decorators, annotations.
    pub attribute: u32,
}

impl Theme {
    /// HSLA colour for graph lane `i` (cycles through the 8-colour palette).
    pub fn lane_color(&self, i: usize) -> Hsla {
        let (h, s, l) = self.lane_hsl[i % self.lane_hsl.len()];
        hsla(h, s, l, 1.0)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Active-theme atomic + accessors
// ──────────────────────────────────────────────────────────────────────────

/// Index into [`THEMES`] of the currently-active theme.  Defaults to 0
/// (Catppuccin Mocha).
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The currently-active theme.  Called from every render path.
#[inline]
pub fn theme() -> &'static Theme {
    let i = ACTIVE.load(Ordering::Relaxed);
    &THEMES[i.min(THEMES.len() - 1)]
}

/// GitKraken-style ref-badge styling (user request).
///
/// Both light and dark themes use the tinted chip — the ref colour at low
/// alpha for the fill, a stronger alpha for the border. Dark themes put white
/// text on the tint; light themes put the theme's main (near-black) text on
/// it, mirroring GitKraken's light theme (ADR-0126 — the old opaque light
/// chips read as high-contrast blocks against light surfaces).
///
/// Returns `(bg_rgba, border_rgba, text_rgb)` for use with
/// `gpui::rgba` / `gpui::rgb`.
#[inline]
pub fn badge_style(color: u32) -> (u32, u32, u32) {
    let t = theme();
    // 0x33 ≈ 20% fill, 0x66 ≈ 40% border (rgitui grammar).
    let text = if t.dark { 0xffffff } else { t.text_main };
    ((color << 8) | 0x33, (color << 8) | 0x66, text)
}

/// The active theme's lane colour `i` packed as a `0xRRGGBB` u32, for feeding
/// the lane hue into [`badge_style`] (lane-driven ref pills in swimlane mode).
#[inline]
pub fn lane_color_u32(i: usize) -> u32 {
    let c: gpui::Rgba = theme().lane_color(i).into();
    let q = |x: f32| -> u32 { (x.clamp(0.0, 1.0) * 255.0).round() as u32 };
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// Index of the active theme (for the menu "✓" marker).
#[inline]
pub fn active_index() -> usize {
    ACTIVE.load(Ordering::Relaxed).min(THEMES.len() - 1)
}

// ──────────────────────────────────────────────────────────────────────────
// W27-UIPOLISH: global UI zoom (rem-size scaling).
// ──────────────────────────────────────────────────────────────────────────
//
// gpui's `text_*` helpers (text_sm/xs/lg/…) and rem-based lengths resolve
// through `Window::rem_size()` (default 16px). Scaling rem_size therefore
// scales virtually all of kagi's text — kagi uses `text_sm`/`text_xs` 260+
// times and explicit `.text_size(px(..))` only twice. We store the zoom as a
// global permille (×1000) integer in an `AtomicUsize` (mirroring `ACTIVE`),
// persist it to `settings.json` under `"ui_zoom"`, and apply it every frame
// via `window.set_rem_size(px(BASE_REM_PX * zoom()))` at the top of render.

/// Base (1.0×) rem size in pixels — gpui's own default.
pub const BASE_REM_PX: f32 = 16.0;

/// Zoom clamp bounds (inclusive), as documented in the ticket.
pub const ZOOM_MIN: f32 = 0.7;
pub const ZOOM_MAX: f32 = 1.5;

/// One zoom step (cmd-+ / cmd--).
pub const ZOOM_STEP: f32 = 0.1;

/// Active UI zoom factor stored as permille (×1000) so it fits an atomic int.
/// Defaults to 1000 = 1.0× (no zoom).
static UI_ZOOM_PERMILLE: AtomicUsize = AtomicUsize::new(1000);

/// Clamp a raw zoom factor into `[ZOOM_MIN, ZOOM_MAX]`.
#[inline]
pub fn clamp_zoom(z: f32) -> f32 {
    z.clamp(ZOOM_MIN, ZOOM_MAX)
}

/// The currently-active UI zoom factor (e.g. `1.0`, `1.2`). Read every frame.
#[inline]
pub fn zoom() -> f32 {
    UI_ZOOM_PERMILLE.load(Ordering::Relaxed) as f32 / 1000.0
}

/// The rem size in pixels for the current zoom (`BASE_REM_PX * zoom()`), passed
/// to `window.set_rem_size(..)` so all rem-based text/layout scales.
#[inline]
pub fn rem_size_px() -> f32 {
    BASE_REM_PX * zoom()
}

/// W27/W28: scale a fixed layout dimension by the active UI zoom.
///
/// gpui 0.2.2 has no global element-scale transform, and `rem_size` scaling
/// only affects rem-based **text**.  Literal `px(..)` layout dimensions (row
/// heights, panel widths, paddings, graph node/lane geometry) stay fixed unless
/// routed through here, which causes text↔layout drift on zoom — most visibly
/// the commit graph misaligning with its (rem-scaled) text rows.  Wrapping a
/// layout constant as `scaled_px(N)` makes it track the same `zoom()` factor as
/// the text, so the whole UI scales uniformly.
///
/// Use for **layout** dimensions, not for text sizes (text already scales via
/// rem).  `scaled_px(0.0)` and hairline `1.0` borders are returned unscaled-ish
/// by nature of multiplication; callers that want crisp 1px borders may keep a
/// literal `px(1.)`.
#[inline]
pub fn scaled_px(n: f32) -> gpui::Pixels {
    gpui::px(n * zoom())
}

/// W28: bare-`f32` sibling of [`scaled_px`] for coordinate math.
///
/// The commit-graph path-builder computes lane x-centres, node radii, corner
/// radii and edge widths as plain `f32` before wrapping the final point in
/// `px(..)`.  Routing those intermediate values through `scaled(..)` makes the
/// graph geometry track the same `zoom()` factor as the (rem-scaled) row text,
/// so the whole graph scales uniformly and stays aligned.  Identical to
/// `scaled_px` except it returns the bare `f32` instead of `Pixels`.
#[inline]
pub fn scaled(n: f32) -> f32 {
    n * zoom()
}

/// Set the active zoom factor (clamped) and persist it to `settings.json`.
/// Returns the clamped value that is now active.
pub fn set_zoom(z: f32) -> f32 {
    let clamped = clamp_zoom(z);
    let permille = (clamped * 1000.0).round() as usize;
    UI_ZOOM_PERMILLE.store(permille, Ordering::Relaxed);
    write_setting("ui_zoom", Some(&format!("{}", permille)));
    clamped
}

/// Initialise the active zoom at startup from `settings.json` (`"ui_zoom"`,
/// stored as a permille integer). Missing / unparsable / out-of-range values
/// fall back to 1.0×.
pub fn init_zoom() {
    if let Some(permille) = Settings::load().ui_zoom_permille() {
        let z = clamp_zoom(permille as f32 / 1000.0);
        UI_ZOOM_PERMILLE.store((z * 1000.0).round() as usize, Ordering::Relaxed);
    }
    klog!("zoom: {:.2}x", zoom());
}

// ──────────────────────────────────────────────────────────────────────────
// Commit-list column widths (BRANCH/TAG + GRAPH) — persisted across restarts.
// ──────────────────────────────────────────────────────────────────────────

/// Persist one commit-list column width (logical px, rounded) to `settings.json`.
/// `key` is `"badge_col_w"` or `"graph_col_w"`. Called from the resize-drag
/// handler; the final drag move writes the final value (settings.json is tiny).
pub fn set_col_width(key: &str, w: f32) {
    write_setting(key, Some(&format!("{}", w.round() as i64)));
}

/// Read a persisted column width (logical px) from `settings.json`, if present.
pub fn read_col_width(key: &str) -> Option<f32> {
    read_setting(key).and_then(|s| s.trim().parse::<f32>().ok())
}

// ──────────────────────────────────────────────────────────────────────────
// T-SETTINGS-001: compact-graph toggle (persisted, global — mirrors zoom).
// ──────────────────────────────────────────────────────────────────────────
//
// `graph_compact` lives on `KagiApp` (read every render frame), but the
// Settings window persists/restores it through `settings.json` like every other
// preference.  We keep a process-global atomic so startup can seed the initial
// value (read once when a `KagiApp` is constructed) without a serde layer.

/// Active compact-graph flag (`false` = normal row height). Defaults to off.
static GRAPH_COMPACT: AtomicBool = AtomicBool::new(false);

/// The currently-active compact-graph flag (seeds new `KagiApp`s at startup).
#[inline]
pub fn compact_graph() -> bool {
    GRAPH_COMPACT.load(Ordering::Relaxed)
}

/// Set + persist the compact-graph flag to `settings.json` (key `graph_compact`).
pub fn set_compact_graph(on: bool) {
    GRAPH_COMPACT.store(on, Ordering::Relaxed);
    write_setting("graph_compact", Some(if on { "true" } else { "false" }));
}

/// Initialise the compact-graph flag at startup from `settings.json`
/// (`"graph_compact"`, `"true"`/`"false"`). Missing/invalid → off.
pub fn init_compact_graph() {
    if let Some(on) = Settings::load().graph_compact() {
        GRAPH_COMPACT.store(on, Ordering::Relaxed);
    }
    klog!("graph_compact: {}", compact_graph());
}

// ──────────────────────────────────────────────────────────────────────────
// ADR-0124: diff display mode (unified / side-by-side), persisted + global.
// ──────────────────────────────────────────────────────────────────────────

/// Active split-diff flag (`false` = unified single-column). Defaults to off.
static DIFF_SPLIT: AtomicBool = AtomicBool::new(false);

/// The currently-active split-diff flag (read at render time).
#[inline]
pub fn diff_split() -> bool {
    DIFF_SPLIT.load(Ordering::Relaxed)
}

/// Set + persist the split-diff flag to `settings.json` (key `diff_split`).
pub fn set_diff_split(on: bool) {
    DIFF_SPLIT.store(on, Ordering::Relaxed);
    write_setting("diff_split", Some(if on { "true" } else { "false" }));
}

/// Initialise the split-diff flag at startup from `settings.json`
/// (`"diff_split"`, `"true"`/`"false"`). Missing/invalid → off (unified).
pub fn init_diff_split() {
    if let Some(on) = Settings::load().diff_split() {
        DIFF_SPLIT.store(on, Ordering::Relaxed);
    }
    klog!("diff_split: {}", diff_split());
}

/// Log the persisted **swimlane-visuals** flag at startup (settings.json
/// `"graph_lane_compact"`, `"true"`/`"false"`; missing → off).
///
/// This flag drives the swimlane *visuals* only — avatar commit nodes, the
/// lane tint band, and the graph lane padding (bin crate `render_helpers.rs`).
/// The lane *layout* is always the gitk-style `graph::layout` (ADR-0122) and does
/// not read this flag. Backed by an atomic (like `graph_compact`) so the
/// Settings-screen toggle can flip it live; the loaded value is logged here so
/// the startup state is debuggable alongside the other settings lines.
static GRAPH_LANE_COMPACT: AtomicBool = AtomicBool::new(false);

/// The currently-active swimlane-visuals flag (read at render time).
#[inline]
pub fn graph_lane_compact() -> bool {
    GRAPH_LANE_COMPACT.load(Ordering::Relaxed)
}

/// Set + persist the swimlane-visuals flag to `settings.json`
/// (key `graph_lane_compact`).
pub fn set_graph_lane_compact(on: bool) {
    GRAPH_LANE_COMPACT.store(on, Ordering::Relaxed);
    write_setting(
        "graph_lane_compact",
        Some(if on { "true" } else { "false" }),
    );
}

/// Initialise the swimlane-visuals flag at startup from `settings.json`.
pub fn init_graph_lane_compact() {
    if let Some(on) = Settings::load().graph_lane_compact() {
        GRAPH_LANE_COMPACT.store(on, Ordering::Relaxed);
    }
    klog!("graph_lane_compact: {}", graph_lane_compact());
}

/// Background auto-fetch flag. Defaults to **on** (periodic + on-focus fetch so
/// the commit graph and ahead/behind counts stay fresh without manual fetches).
static AUTO_FETCH: AtomicBool = AtomicBool::new(true);

/// The currently-active auto-fetch flag (read by the auto-fetch ticker).
#[inline]
pub fn auto_fetch() -> bool {
    AUTO_FETCH.load(Ordering::Relaxed)
}

/// Set + persist the auto-fetch flag to `settings.json` (key `auto_fetch`).
pub fn set_auto_fetch(on: bool) {
    AUTO_FETCH.store(on, Ordering::Relaxed);
    write_setting("auto_fetch", Some(if on { "true" } else { "false" }));
}

/// Initialise the auto-fetch flag at startup from `settings.json`
/// (`"auto_fetch"`). Missing → on; only an explicit `"false"` disables it.
pub fn init_auto_fetch() {
    if let Some(on) = Settings::load().auto_fetch() {
        AUTO_FETCH.store(on, Ordering::Relaxed);
    }
    klog!("auto_fetch: {}", auto_fetch());
}

/// Look up a theme index by slug.
pub fn index_of(slug: &str) -> Option<usize> {
    THEMES
        .iter()
        .position(|t| t.slug == legacy_slug_alias(slug))
}

/// Map retired theme slugs onto their successors so an existing
/// `settings.json` (or `KAGI_THEME`) doesn't silently fall back to the
/// default.
///
/// The Xcode themes were byte-identical to the Apple ones — they were the
/// same Apple `.xccolortheme` files — so they were removed and the Apple
/// themes now carry Xcode's official code colours (T-SYNTAX-001).
fn legacy_slug_alias(slug: &str) -> &str {
    match slug {
        "xcode-dark" => "apple-dark",
        "xcode-light" => "apple-light",
        other => other,
    }
}

/// Test-only guard for the global `ACTIVE` index + `KAGI_LOG_DIR`; held by
/// every test that changes the active theme (incl. `avatar`'s colour test).
#[cfg(test)]
pub(crate) static ACTIVE_THEME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set the active theme by slug and persist it to `settings.json`.
/// Returns `true` if the slug was recognised.
pub fn set_active(slug: &str) -> bool {
    match index_of(slug) {
        Some(i) => {
            ACTIVE.store(i, Ordering::Relaxed);
            save_settings(slug);
            true
        }
        None => false,
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Settings persistence (hand-written JSON; no serde — mirrors oplog.rs)
// ──────────────────────────────────────────────────────────────────────────

/// Read the persisted theme slug from `settings.json`, if present and valid.
pub fn load_settings_slug() -> Option<String> {
    Settings::load().theme()
}

/// Persist the theme slug to `settings.json` (preserving other keys).
pub fn save_settings(slug: &str) {
    write_setting("theme", Some(slug));
}

/// Initialise the active theme at startup.
///
/// Priority: `KAGI_THEME` env override → persisted `settings.json` →
/// default (Catppuccin Mocha).  Logs `[kagi] theme: <slug> dark=<bool>`.
pub fn init_active() {
    let slug = std::env::var("KAGI_THEME")
        .ok()
        .filter(|s| !s.is_empty())
        .filter(|s| index_of(s).is_some())
        .or_else(load_settings_slug)
        .filter(|s| index_of(s).is_some());

    if let Some(slug) = slug {
        if let Some(i) = index_of(&slug) {
            ACTIVE.store(i, Ordering::Relaxed);
        }
    }
    let t = theme();
    klog!("theme: {} dark={}", t.slug, t.dark);
}

// ──────────────────────────────────────────────────────────────────────────
// W12-GCADOPT: gpui-component theme bridge (one-way push, kagi → gpui-component)
// ──────────────────────────────────────────────────────────────────────────

/// Convert a kagi `0xRRGGBB` colour to `gpui::Hsla` (opaque) via `gpui::rgb`.
/// `Hsla: From<Rgba>` is provided by gpui, so this never loses precision beyond
/// the RGB→HSL round-trip the renderer would do anyway.
/// Clamp a context-menu anchor so the menu stays inside the viewport.
///
/// `menu_w` / `menu_h` are the menu's design size in UNSCALED px (the caller's
/// best estimate for height is fine); the cursor `pos` is in raw window px.
/// Zoom scaling is applied here so callers never repeat it. Every
/// cursor-anchored overlay must route through this — hand-positioned menus
/// overflowing the right/bottom edge was a recurring bug class.
pub fn clamp_menu_pos(
    pos: gpui::Point<gpui::Pixels>,
    menu_w: f32,
    menu_h: f32,
    viewport: gpui::Size<gpui::Pixels>,
) -> gpui::Point<gpui::Pixels> {
    const MARGIN: f32 = 8.0;
    let z = zoom();
    let (w, h) = (menu_w * z, menu_h * z);
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    let (raw_x, raw_y) = (f32::from(pos.x), f32::from(pos.y));
    let x = if raw_x + w + MARGIN > vw {
        (vw - w - MARGIN).max(MARGIN)
    } else {
        raw_x.max(MARGIN)
    };
    let y = if raw_y + h + MARGIN > vh {
        (vh - h - MARGIN).max(MARGIN)
    } else {
        raw_y.max(MARGIN)
    };
    gpui::point(gpui::px(x), gpui::px(y))
}

/// Alpha for the text-selection tint. Matches the cap gpui-component applies to
/// its own bundled themes (`theme/schema.rs`), so text stays legible through it.
const SELECTION_ALPHA: f32 = 0.30;

#[inline]
fn to_hsla(rgb_u32: u32) -> Hsla {
    Hsla::from(rgb(rgb_u32))
}

/// Push kagi's active [`theme()`] palette into `gpui_component`'s global
/// `ThemeColor` so every adopted gpui-component widget (Input, Tooltip,
/// Scrollbar, Checkbox, …) renders with kagi's colours.
///
/// **One-way only** (kagi → gpui-component): kagi's `theme()` stays the single
/// source of truth (ADR-0036); nothing ever reads back from `ThemeColor`.
///
/// Call sites:
/// * startup, **after** `gpui_component::init(cx)` (which runs
///   `sync_system_appearance` and would otherwise leave system colours showing);
/// * every `View → Theme` switch (`KagiApp::set_theme`).
///
/// Only the fields the adopted components actually read are mapped; the other
/// ~70 `ThemeColor` fields keep their gpui-component defaults (the audit doc
/// confirms full coverage is unnecessary).  `mode` is set from `theme().dark`
/// so any dark/light-conditional logic inside gpui-component matches kagi.
pub fn sync_gpui_component_theme(cx: &mut App) {
    let k = theme();
    let gc = gpui_component::Theme::global_mut(cx);

    // ── Base preset (gpui-component 0.5.2) ──────────────────────
    // 0.5.2 grew ~40 new `ThemeColor` fields (the `button_*` family,
    // `input_background`, charts, …) that adopted widgets read directly.
    // Seed every field from the mode-matching preset first so anything kagi
    // doesn't explicitly map below is at least dark/light-appropriate —
    // otherwise the light defaults leak into the dark UI (white dropdown,
    // black radio, user-reported after the 0.5.1 → 0.5.2 bump).
    gc.colors = if k.dark {
        *gpui_component::theme::ThemeColor::dark()
    } else {
        *gpui_component::theme::ThemeColor::light()
    };

    // ── Surfaces ────────────────────────────────────────────────
    gc.colors.background = to_hsla(k.bg_base);
    gc.colors.foreground = to_hsla(k.text_main);
    // `border` is what gpui-component draws dividers with — table row lines
    // in TextView (PR descriptions), scrollbar tracks, tabs. `selected` is a
    // near-background neutral by design, which made markdown table rows
    // invisible (user report); a muted-foreground tint reads on both `surface`
    // and `bg_base` in every theme.
    gc.colors.border = to_hsla(k.text_muted).alpha(0.85);
    gc.colors.muted = to_hsla(k.surface);
    gc.colors.muted_foreground = to_hsla(k.text_muted);

    // ── Popover / overlay / selection (Tooltip, modals, Input) ──
    gc.colors.popover = to_hsla(k.modal);
    gc.colors.popover_foreground = to_hsla(k.text_main);
    gc.colors.overlay = to_hsla(k.modal_overlay);
    // Text selection is NOT `selected` (the list-row highlight): that colour is
    // deliberately a near-background neutral, and against an Input's background
    // it reads as "nothing happened" (user report). gpui-component paints the
    // selection *under* the text and caps its own themes at alpha 0.3, so an
    // accent tint at that alpha is both visible and safe for legibility.
    gc.colors.selection = to_hsla(k.color_branch).alpha(SELECTION_ALPHA);

    // ── Primary / accent (Checkbox checked, focus ring, links) ──
    gc.colors.primary = to_hsla(k.color_branch);
    gc.colors.primary_foreground = to_hsla(k.bg_base);
    gc.colors.primary_hover = to_hsla(k.color_branch);
    gc.colors.primary_active = to_hsla(k.color_branch);
    gc.colors.ring = to_hsla(k.color_branch);
    // `accent` is gpui-component's inline-code background (TextView) and a
    // hover tint in a few popovers. `selected` — the row highlight, near the
    // background by design — made `code` spans unreadable in PR descriptions
    // (user report). A muted-foreground tint at low alpha reads as a chip on
    // both `surface` and `bg_base`, in every theme.
    gc.colors.accent = to_hsla(k.text_muted).alpha(0.48);
    gc.colors.accent_foreground = to_hsla(k.text_main);
    gc.colors.link = to_hsla(k.color_branch);

    // ── Secondary / title-bar controls (gpui-component TitleBar) ──
    gc.colors.secondary = to_hsla(k.surface);
    gc.colors.secondary_foreground = to_hsla(k.text_main);
    gc.colors.secondary_hover = to_hsla(k.selected);
    gc.colors.secondary_active = to_hsla(k.surface);

    // ── Input border (Input, Checkbox unchecked) ────────────────
    gc.colors.input = to_hsla(k.text_muted);
    gc.colors.caret = to_hsla(k.text_main);

    // ── Buttons (0.5.2 reads `button_*`, not primary/secondary) ─
    // Default/neutral (Cherry-pick, Tree/Path, hash chip): kagi surface.
    gc.colors.button = to_hsla(k.surface);
    gc.colors.button_foreground = to_hsla(k.text_main);
    gc.colors.button_hover = to_hsla(k.selected);
    gc.colors.button_active = to_hsla(k.surface);
    // Primary (Branch here): kagi's branch accent, as before the bump.
    gc.colors.button_primary = to_hsla(k.color_branch);
    gc.colors.button_primary_foreground = to_hsla(k.bg_base);
    gc.colors.button_primary_hover = to_hsla(k.color_branch);
    gc.colors.button_primary_active = to_hsla(k.color_branch);
    gc.colors.button_secondary = to_hsla(k.surface);
    gc.colors.button_secondary_foreground = to_hsla(k.text_main);
    gc.colors.button_secondary_hover = to_hsla(k.selected);
    gc.colors.button_secondary_active = to_hsla(k.surface);
    gc.colors.button_danger = to_hsla(k.color_blocker);
    gc.colors.button_danger_foreground = to_hsla(0xffffff);
    gc.colors.button_danger_hover = to_hsla(k.color_blocker);
    gc.colors.button_danger_active = to_hsla(k.color_blocker);

    // ── Status colours (Notification, Alert, etc.) ──────────────
    gc.colors.success = to_hsla(k.color_success);
    gc.colors.warning = to_hsla(k.color_warning);
    gc.colors.danger = to_hsla(k.color_blocker);
    gc.colors.danger_hover = to_hsla(k.color_blocker);
    gc.colors.danger_active = to_hsla(k.color_blocker);
    gc.colors.danger_foreground = to_hsla(0xffffff);
    gc.colors.info = to_hsla(k.color_branch);

    // ── List / sidebar (PopupMenu, ListItem, Sidebar) ───────────
    gc.colors.list = to_hsla(k.bg_base);
    gc.colors.list_active = to_hsla(k.selected);
    gc.colors.list_hover = to_hsla(k.surface);
    gc.colors.sidebar = to_hsla(k.sidebar);
    gc.colors.sidebar_foreground = to_hsla(k.text_main);
    gc.colors.title_bar = to_hsla(k.panel);
    gc.colors.title_bar_border = to_hsla(k.surface);

    // ── Scrollbar (W12-GCADOPT §2.10) ───────────────────────────
    gc.colors.scrollbar = to_hsla(k.bg_base);
    gc.colors.scrollbar_thumb = to_hsla(k.text_muted);
    gc.colors.scrollbar_thumb_hover = to_hsla(k.text_sub);

    // ── Drag handle (resizable dividers, future adoption) ───────
    gc.colors.drag_border = to_hsla(k.color_branch);

    // ── Fonts ───────────────────────────────────────────────────
    // gpui-component's default theme uses `.SystemUIFont` (a macOS alias) for
    // `font_family` and a platform mono for `mono_font_family`. On Linux
    // `.SystemUIFont` doesn't resolve, so gpui-component widgets (Button, Input,
    // Tooltip, the commit-message editor, …) fell back to a system font while
    // kagi's own `UI_FONT` text rendered in the bundled Inter — buttons/commit
    // text looked like a different font (user-reported). Point gpui-component at
    // the same bundled families kagi loads via `add_fonts` (UI_FONT / MONO_FONT).
    gc.font_family = UI_FONT.into();
    gc.mono_font_family = MONO_FONT.into();

    // ── Mode (drives dark/light-conditional logic inside gpui-component) ──
    gc.mode = if k.dark {
        gpui_component::ThemeMode::Dark
    } else {
        gpui_component::ThemeMode::Light
    };

    // ── Code-editor highlight theme (CodeEditor InputState) ──────
    // The CodeEditor's editor background, current/active-line highlight and
    // line numbers come from `highlight_theme` (the Zed-format syntax theme),
    // NOT from `gc.colors`. gpui-component defaults this to `default_light()`,
    // so on kagi's dark UI the conflict editor's active line painted WHITE
    // (user report). Pick the matching preset, then override the editor
    // surfaces to kagi's own palette so the panes blend with the rest of the UI
    // (active line = the subtle row-highlight `selected`, not a bright bar).
    //
    // T-SYNTAX-001: the *syntax* colours now come from the active theme too,
    // via [`highlight_theme`] — previously only these five editor surfaces were
    // overridden and `style.syntax` kept gpui-component's bundled palette, so
    // every dark theme highlighted code identically.
    gc.highlight_theme = highlight_theme(k);

    // ── Tokens (0.5.2) ──────────────────────────────────────────
    // Widgets increasingly read `theme().tokens.*` (Radio, Select popup, …),
    // which is a snapshot derived from `colors` — rebuild it LAST or every
    // mapping above is invisible to token-reading widgets.
    gc.tokens = gpui_component::theme::ThemeTokens::from(&gc.colors);
}

/// Build the gpui-component highlight theme for `k` (T-SYNTAX-001).
///
/// Shared by the CodeEditor panes (through `gc.highlight_theme`) and the diff
/// views (`diff_view::highlight_diff_rows*`), so both render code the same way
/// — they previously disagreed, because the diff side called
/// `HighlightTheme::default_dark()` directly and never saw even the editor
/// surface overrides.
///
/// Built by serialising a Zed-format theme and deserialising it, rather than
/// constructing `SyntaxColors` field by field: gpui-component's `ThemeStyle`
/// keeps its `color`/`font_style` fields private, so JSON is the only public
/// way in. Cheap and done once per theme switch.
pub fn highlight_theme(k: &Theme) -> std::sync::Arc<gpui_component::highlighter::HighlightTheme> {
    let s = &k.syntax;
    // Roles the ten palette entries expand to. Derivations follow the source
    // palettes: booleans are numbers, enums/variants are types, constructors
    // and titles are functions, doc comments are comments.
    let hex = |c: u32| format!("#{:06x}", c & 0x00ff_ffff);
    let json = format!(
        r##"{{
          "name": "kagi-{slug}",
          "appearance": "{appearance}",
          "style": {{
            "editor.background": "{bg}",
            "editor.foreground": "{fg}",
            "editor.active_line.background": "{active}",
            "editor.line_number": "{lineno}",
            "editor.active_line_number": "{lineno_active}",
            "syntax": {{
              "keyword":                    {{ "color": "{keyword}" }},
              "operator":                   {{ "color": "{operator}" }},
              "punctuation":                {{ "color": "{punct}" }},
              "punctuation.bracket":        {{ "color": "{punct}" }},
              "punctuation.delimiter":      {{ "color": "{punct}" }},
              "punctuation.special":        {{ "color": "{operator}" }},
              "punctuation.list_marker":    {{ "color": "{punct}" }},
              "string":                     {{ "color": "{string}" }},
              "string.escape":              {{ "color": "{number}" }},
              "string.regex":               {{ "color": "{string}" }},
              "string.special":             {{ "color": "{string}" }},
              "string.special.symbol":      {{ "color": "{string}" }},
              "comment":                    {{ "color": "{comment}", "font_style": "italic" }},
              "comment.doc":                {{ "color": "{comment}", "font_style": "italic" }},
              "type":                       {{ "color": "{type_name}" }},
              "enum":                       {{ "color": "{type_name}" }},
              "variant":                    {{ "color": "{type_name}" }},
              "constructor":                {{ "color": "{function}" }},
              "function":                   {{ "color": "{function}" }},
              "title":                      {{ "color": "{function}" }},
              "number":                     {{ "color": "{number}" }},
              "boolean":                    {{ "color": "{number}" }},
              "constant":                   {{ "color": "{number}" }},
              "text.literal":               {{ "color": "{string}" }},
              "variable":                   {{ "color": "{variable}" }},
              "variable.special":           {{ "color": "{variable}" }},
              "property":                   {{ "color": "{variable}" }},
              "label":                      {{ "color": "{variable}" }},
              "attribute":                  {{ "color": "{attribute}" }},
              "tag":                        {{ "color": "{keyword}" }},
              "preproc":                    {{ "color": "{attribute}" }},
              "embedded":                   {{ "color": "{fg}" }},
              "primary":                    {{ "color": "{fg}" }},
              "hint":                       {{ "color": "{comment}" }},
              "predictive":                 {{ "color": "{comment}" }},
              "link_text":                  {{ "color": "{function}" }},
              "link_uri":                   {{ "color": "{string}" }},
              "emphasis":                   {{ "color": "{fg}" }},
              "emphasis.strong":            {{ "color": "{keyword}" }}
            }}
          }}
        }}"##,
        slug = k.slug,
        appearance = if k.dark { "dark" } else { "light" },
        bg = hex(k.bg_base),
        fg = hex(k.text_main),
        active = hex(k.bg_row_alt),
        lineno = hex(k.text_muted),
        lineno_active = hex(k.text_sub),
        keyword = hex(s.keyword),
        operator = hex(s.operator),
        punct = hex(s.punctuation),
        string = hex(s.string),
        comment = hex(s.comment),
        type_name = hex(s.type_name),
        function = hex(s.function),
        number = hex(s.number),
        variable = hex(s.variable),
        attribute = hex(s.attribute),
    );

    match serde_json::from_str(&json) {
        Ok(t) => std::sync::Arc::new(t),
        // A malformed literal above is a programming error, not a user-facing
        // one; fall back to the bundled preset rather than killing the app
        // mid-theme-switch. The unit test below keeps every theme honest.
        Err(_e) => {
            if k.dark {
                gpui_component::highlighter::HighlightTheme::default_dark()
            } else {
                gpui_component::highlighter::HighlightTheme::default_light()
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Theme registry — the built-in themes
// ──────────────────────────────────────────────────────────────────────────

/// All built-in themes.  Index 0 (Catppuccin Mocha) is the default.
// The default (Catppuccin Mocha) is pinned at index 0 — that invariant is relied
// on by `theme()` (the default ACTIVE index), the docs, and tests. The remaining
// themes are sorted alphabetically by display name so the picker stays tidy as
// themes are added.
pub static THEMES: &[Theme] = &[
    CATPPUCCIN_MOCHA,
    crate::theme_apple::APPLE_DARK,
    crate::theme_apple::APPLE_LIGHT,
    CATPPUCCIN_LATTE,
    DRACULA,
    IBM_PC,
    MONOKAI,
    ONE_DARK,
    ONE_LIGHT,
    PERIWINKLE,
    PINKY_BOO,
    TOKYO_NIGHT,
];

// ── Catppuccin Mocha (default) ───────────────────────────────────────────
//
// Byte-exact port of the previous hard-coded constants (mod.rs etc.).  The
// lane HSL values reproduce the previous `graph_view::lane_color` palette;
// avatar sat/light reproduce `avatar::avatar_color` (0.70 / 0.60); terminal
// values reproduce `terminal.rs`.
/// Lane colour palette for **dark-background** themes.
///
/// `oklch(0.77 0.174 H)` gamut-mapped to sRGB then stored as HSL. Lightness and
/// chroma are fixed so every lane reads at equal brightness/vividness; only hue
/// rotates, ordered so adjacent lane indices are maximally distinct (Gitru
/// swimlane philosophy, see ADR-0104).
const LANE_PALETTE_DARK: [(f32, f32, f32); 8] = [
    (0.937, 1.0, 0.749),   // pink   #ff7fb0
    (0.268, 0.546, 0.558), // green  #81cc51
    (0.619, 1.0, 0.770),   // blue   #8aabff
    (0.059, 1.0, 0.647),   // orange #ff8b4b
    (0.477, 1.0, 0.421),   // teal   #00d7b8
    (0.783, 1.0, 0.780),   // purple #dd8fff
    (0.129, 1.0, 0.437),   // gold   #dfad00
    (0.535, 1.0, 0.500),   // cyan   #00c9ff
];

/// Lane colour palette for **light-background** themes — same hues/chroma at a
/// lower lightness (`oklch(0.58 0.174 H)`) for contrast on light surfaces.
// The gold lightness (0.318) happens to sit near `FRAC_1_PI`; it is a colour
// component, not a maths constant, so silence the false positive.
#[allow(clippy::approx_constant)]
const LANE_PALETTE_LIGHT: [(f32, f32, f32); 8] = [
    (0.935, 0.542, 0.519), // rose   #c74276
    (0.250, 1.0, 0.281),   // green  #478f00
    (0.634, 0.692, 0.603), // blue   #546fe0
    (0.064, 1.0, 0.394),   // orange #c94e00
    (0.471, 1.0, 0.300),   // teal   #00997e
    (0.785, 0.460, 0.539), // purple #a053bf
    (0.117, 1.0, 0.318),   // gold   #a27100
    (0.550, 1.0, 0.389),   // blue2  #008bc6
];

const CATPPUCCIN_MOCHA: Theme = Theme {
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

// ── One Dark (Atom One Dark) ──────────────────────────────────────────────
//
// Atom / VS Code "One Dark" palette: bg #282c34, fg #abb2bf, red #e06c75,
// green #98c379, yellow #e5c07b, blue #61afef, purple #c678dd, cyan #56b6c2.
const ONE_DARK: Theme = Theme {
    slug: "one-dark",
    name: "One Dark",
    dark: true,

    bg_base: 0x282c34,
    bg_row_alt: 0x24272e,
    surface: 0x3a3f4b,
    selected: 0x4b5263,
    panel: 0x21252b,
    sidebar: 0x1c1f24,
    modal: 0x3a3f4b,
    modal_overlay: 0x000000,

    text_main: 0xabb2bf,
    text_sub: 0x9099a8,
    text_muted: 0x5c6370,
    text_label: 0x6b7280,

    color_head: 0xe06c75,   // red
    color_branch: 0x61afef, // blue
    color_remote: 0x98c379, // green
    color_tag: 0xe5c07b,    // yellow

    color_success: 0x98c379,
    color_warning: 0xe5c07b,
    color_blocker: 0xe06c75,
    color_blocker_muted: 0x8a4f55,

    diff_added_bg: 0x26392b,
    diff_removed_bg: 0x3a2526,
    diff_hunk: 0x61afef,

    change_added: 0x98c379,
    change_modified: 0xe5c07b,
    change_deleted: 0xe06c75,
    change_renamed: 0x61afef,
    change_typechange: 0x5c6370,
    change_dir: 0x6b7280,

    accent: 0xc678dd, // purple

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.55,
    avatar_light: 0.62,

    term_bg: (0x28, 0x2c, 0x34),
    term_fg: (0xab, 0xb2, 0xbf),
    term_cursor: (0x52, 0x8b, 0xff),
    term_black: (0x3f, 0x44, 0x51),
    term_red: (0xe0, 0x6c, 0x75),
    term_green: (0x98, 0xc3, 0x79),
    term_yellow: (0xe5, 0xc0, 0x7b),
    term_blue: (0x61, 0xaf, 0xef),
    term_magenta: (0xc6, 0x78, 0xdd),
    term_cyan: (0x56, 0xb6, 0xc2),
    term_white: (0xab, 0xb2, 0xbf),
    term_bright_black: (0x5c, 0x63, 0x70),
    term_bright_red: (0xe0, 0x6c, 0x75),
    term_bright_green: (0x98, 0xc3, 0x79),
    term_bright_yellow: (0xe5, 0xc0, 0x7b),
    term_bright_blue: (0x61, 0xaf, 0xef),
    term_bright_magenta: (0xc6, 0x78, 0xdd),
    term_bright_cyan: (0x56, 0xb6, 0xc2),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x3e, 0x44, 0x51, 0xcc),

    // Code colours: Atom One Dark (via One Dark Pro). Punctuation is plain.
    syntax: SyntaxPalette {
        keyword: 0xc678dd, // purple
        string: 0x98c379,  // green
        comment: 0x7f848e,
        type_name: 0xe5c07b,   // yellow
        function: 0x61afef,    // blue
        number: 0xd19a66,      // orange
        operator: 0x56b6c2,    // cyan
        punctuation: 0xabb2bf, // Foreground — flat by design
        variable: 0xe06c75,    // red
        attribute: 0x61afef,
    },
};

// ── One Light (Atom One Light) ────────────────────────────────────────────
//
// Atom / VS Code "One Light" palette: bg #fafafa, fg #383a42, red #e45649,
// green #50a14f, yellow/amber #c18401, blue #4078f2, purple #a626a4,
// cyan #0184bc.
const ONE_LIGHT: Theme = Theme {
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

// ── Monokai (= tomixrm Warm Hybrid, dark variant) ─────────────────────────
//
// Extracted from `docs/research/reference/tomixrm-warm-hybrid.json` (MIT):
// editor.background #2f2b31, editor.foreground #c8c8c8, cursor #ff9940,
// terminal.ansi* colours, plus tokenColors (keyword #ff668c, string #f4cd62,
// function #a4d671, type #7bdae7, parameter #fe9b69).  Accent is the warm
// orange #ff9940 (cursor) requested by the ticket.
//
// ── Vivid/contrast boost (T011) ──────────────────────────────────────────
// Backgrounds darkened ~7–9 steps to push contrast ratio up; accents
// saturated toward the reference tokenColor values.  WCAG target:
//   text_main (#f0ece8) vs bg_base (#28242a) ≈ 13:1  (≥ 7 ✓)
//   text_muted (#918d94) vs bg_base (#28242a) ≈ 3.5:1 (≥ 3 ✓)
//
// Key before → after pairs:
//   bg_base      #2f2b31 → #28242a   (darker, more contrast)
//   bg_row_alt   #2a272c → #221e24
//   panel        #272328 → #1f1b21
//   sidebar      #231f25 → #1a161c
//   surface      #403b44 → #3a3540   (delta to selected preserved)
//   selected     #4d4751 → #4a4454
//   text_main    #c8c8c8 → #f0ece8   (warmer white, ~13:1 vs bg_base)
//   text_sub     #a6a2a8 → #b8b4bc
//   text_muted   #807c82 → #918d94
//   color_head   #ff6b90 → #ff3d6f   (vivid pink, ref #ff668c)
//   color_remote #9ed06c → #a8e05a   (vivid green, ref #a4d671)
//   color_tag    #ff9940 → #ff8c1a   (punchier orange)
//   color_warning #e8c15d → #f4cd62  (match ref string yellow)
//   accent       #b39af5 → #b08fff   (vivid purple)
//   accent_alt   #7dd7e6 → #7be8f5   (vivid cyan, ref #7bdae7)
//   lane_hsl sat +0.05–0.10, lightness bumped for darker bg
//   term bright side: brighter/more saturated
const MONOKAI: Theme = Theme {
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

// ── Tokyo Night ───────────────────────────────────────────────────────────
//
// The popular "Tokyo Night" palette (enkia): bg #1a1b26, fg #c0caf5, blue
// #7aa2f7, cyan #7dcfff, green #9ece6a, red #f7768e, yellow #e0af68, magenta
// #bb9af7, comment #565f89, selection #283457.
const TOKYO_NIGHT: Theme = Theme {
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

// ── IBM PC ────────────────────────────────────────────────────────────────
//
// Classic IBM PC / DOS look: black background with the 16-colour CGA palette
// (bright blue #5555ff, green #55ff55, cyan #55ffff, red #ff5555, magenta
// #ff55ff, yellow #ffff55, white #ffffff), and the iconic blue selection bar.
const IBM_PC: Theme = Theme {
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

// ── Pinky Boo ───────────────────────────────────────────────────────────────
//
// Light theme ported from the "Pinky Boo" VS Code theme
// (github.com/kissa1001/pinky-boo-vscode-theme): editor bg #fbfbfb, soft signature
// pink #ffafeb (status bar / list selection), hot-pink accent #ff398d, neutral
// grey text #5a5a5a, with the theme's exact ANSI terminal palette.
const PINKY_BOO: Theme = Theme {
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

// ── Catppuccin Latte ─────────────────────────────────────────────────────────
//
// Official light flavour of Catppuccin (catppuccin.com): Base #eff1f5, Text
// #4c4f69, Mauve accent #8839ef. Token/terminal mapping mirrors the Mocha port
// above, swapped to the Latte palette.
const CATPPUCCIN_LATTE: Theme = Theme {
    slug: "catppuccin-latte",
    name: "Catppuccin Latte",
    dark: false,

    bg_base: 0xeff1f5,    // base
    bg_row_alt: 0xe6e9ef, // mantle
    surface: 0xccd0da,    // surface0
    selected: 0xbcc0cc,   // surface1
    panel: 0xe6e9ef,      // mantle
    sidebar: 0xdce0e8,    // crust
    modal: 0xccd0da,      // surface0
    modal_overlay: 0x4c4f69,

    text_main: 0x4c4f69,  // text
    text_sub: 0x6c6f85,   // subtext0
    text_muted: 0xacb0be, // surface2
    text_label: 0x9ca0b0, // overlay0

    color_head: 0xd20f39,   // red
    color_branch: 0x1e66f5, // blue
    color_remote: 0x40a02b, // green
    color_tag: 0xfe640b,    // peach

    color_success: 0x40a02b,
    color_warning: 0xdf8e1d,
    color_blocker: 0xd20f39,
    color_blocker_muted: 0xd98a9a,

    diff_added_bg: 0xdcf0d8,
    diff_removed_bg: 0xfbdde1,
    diff_hunk: 0x1e66f5,

    change_added: 0x40a02b,
    change_modified: 0xdf8e1d,
    change_deleted: 0xd20f39,
    change_renamed: 0x1e66f5,
    change_typechange: 0xacb0be,
    change_dir: 0x9ca0b0,

    accent: 0x8839ef, // mauve

    lane_hsl: LANE_PALETTE_LIGHT,

    avatar_sat: 0.55,
    avatar_light: 0.50,

    term_bg: (0xef, 0xf1, 0xf5),
    term_fg: (0x4c, 0x4f, 0x69),
    term_cursor: (0xea, 0x76, 0xcb),
    term_black: (0xbc, 0xc0, 0xcc),
    term_red: (0xd2, 0x0f, 0x39),
    term_green: (0x40, 0xa0, 0x2b),
    term_yellow: (0xdf, 0x8e, 0x1d),
    term_blue: (0x1e, 0x66, 0xf5),
    term_magenta: (0x88, 0x39, 0xef),
    term_cyan: (0x04, 0xa5, 0xe5),
    term_white: (0x5c, 0x5f, 0x77),
    term_bright_black: (0xac, 0xb0, 0xbe),
    term_bright_red: (0xd2, 0x0f, 0x39),
    term_bright_green: (0x40, 0xa0, 0x2b),
    term_bright_yellow: (0xdf, 0x8e, 0x1d),
    term_bright_blue: (0x1e, 0x66, 0xf5),
    term_bright_magenta: (0x88, 0x39, 0xef),
    term_bright_cyan: (0x04, 0xa5, 0xe5),
    term_bright_white: (0x4c, 0x4f, 0x69),
    term_selection: (0xac, 0xb0, 0xbe, 0x99),

    // Code colours: Catppuccin Latte — same role mapping as Mocha.
    syntax: SyntaxPalette {
        keyword: 0x8839ef,     // Mauve
        string: 0x40a02b,      // Green
        comment: 0x7c7f93,     // Overlay 2
        type_name: 0xdf8e1d,   // Yellow
        function: 0x1e66f5,    // Blue
        number: 0xfe640b,      // Peach
        operator: 0x04a5e5,    // Sky
        punctuation: 0x7c7f93, // Overlay 2
        variable: 0xe64553,    // Maroon
        attribute: 0xdf8e1d,   // Yellow
    },
};

// ── Dracula ──────────────────────────────────────────────────────────────────
//
// Official Dracula palette (draculatheme.com): Background #282a36, Foreground
// #f8f8f2, Purple accent #bd93f9, with the project's canonical ANSI terminal
// palette.
const DRACULA: Theme = Theme {
    slug: "dracula",
    name: "Dracula",
    dark: true,

    bg_base: 0x282a36,
    bg_row_alt: 0x21222c,
    surface: 0x343746,
    selected: 0x44475a,
    panel: 0x21222c,
    sidebar: 0x191a21,
    modal: 0x343746,
    modal_overlay: 0x000000,

    text_main: 0xf8f8f2,
    text_sub: 0xc8c8d4,
    text_muted: 0x6272a4,
    text_label: 0x7e84ad,

    color_head: 0xff79c6,   // pink
    color_branch: 0xbd93f9, // purple
    color_remote: 0x50fa7b, // green
    color_tag: 0xffb86c,    // orange

    color_success: 0x50fa7b,
    color_warning: 0xf1fa8c,
    color_blocker: 0xff5555,
    color_blocker_muted: 0x9a4d4d,

    diff_added_bg: 0x1d3b2b,
    diff_removed_bg: 0x3a1e22,
    diff_hunk: 0x8be9fd,

    change_added: 0x50fa7b,
    change_modified: 0xf1fa8c,
    change_deleted: 0xff5555,
    change_renamed: 0x8be9fd,
    change_typechange: 0x6272a4,
    change_dir: 0x7e84ad,

    accent: 0xbd93f9, // purple

    lane_hsl: LANE_PALETTE_DARK,

    avatar_sat: 0.70,
    avatar_light: 0.65,

    term_bg: (0x28, 0x2a, 0x36),
    term_fg: (0xf8, 0xf8, 0xf2),
    term_cursor: (0xf8, 0xf8, 0xf2),
    term_black: (0x21, 0x22, 0x2c),
    term_red: (0xff, 0x55, 0x55),
    term_green: (0x50, 0xfa, 0x7b),
    term_yellow: (0xf1, 0xfa, 0x8c),
    term_blue: (0xbd, 0x93, 0xf9),
    term_magenta: (0xff, 0x79, 0xc6),
    term_cyan: (0x8b, 0xe9, 0xfd),
    term_white: (0xf8, 0xf8, 0xf2),
    term_bright_black: (0x62, 0x72, 0xa4),
    term_bright_red: (0xff, 0x6e, 0x6e),
    term_bright_green: (0x69, 0xff, 0x94),
    term_bright_yellow: (0xff, 0xff, 0xa5),
    term_bright_blue: (0xd6, 0xac, 0xff),
    term_bright_magenta: (0xff, 0x92, 0xdf),
    term_bright_cyan: (0xa4, 0xff, 0xff),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x44, 0x47, 0x5a, 0x99),

    // Code colours: Dracula (official spec / VS Code theme). Punctuation and
    // plain variables are the foreground by design; operators reuse Pink.
    syntax: SyntaxPalette {
        keyword: 0xff79c6,     // Pink
        string: 0xf1fa8c,      // Yellow
        comment: 0x6272a4,     // Comment blue
        type_name: 0x8be9fd,   // Cyan
        function: 0x50fa7b,    // Green
        number: 0xbd93f9,      // Purple
        operator: 0xff79c6,    // Pink (no separate operator rule)
        punctuation: 0xf8f8f2, // Foreground — flat by design
        variable: 0xf8f8f2,    // Foreground
        attribute: 0x50fa7b,   // Green
    },
};

// ── Periwinkle ───────────────────────────────────────────────────────────────
//
// Built from a supplied palette: background #f8faff, label colours
// #d699ba / #95c0aa / #d1d48c / #000, text #172540.
//
// The three pastel labels measure 1.1-1.6:1 against the periwinkle they came
// with, so they cannot carry text. They stay at full strength where they are a
// *fill* (graph lanes, diff washes) and are darkened along their own hue for
// every text role. That split is the whole design: the palette's character
// lives in the lane colours, its legibility in the derived text ramp.
//
// The original #d4d6e9 background is now `selected` — on a near-white base it
// reads as a highlight, and keeps the periwinkle visible in the chrome.
const PERIWINKLE: Theme = Theme {
    slug: "periwinkle",
    name: "Periwinkle",
    dark: false,

    bg_base: 0xf8faff,    // supplied background
    bg_row_alt: 0xeef1fa, // zebra: one step down
    surface: 0xe8ebf7,    // chips/hover, one step further
    selected: 0xd4d6e9,   // the supplied periwinkle
    panel: 0xeaedf8,
    sidebar: 0xe2e5f2,
    modal: 0xffffff,
    modal_overlay: 0x172540,

    text_main: 0x172540, // supplied font colour
    text_sub: 0x3b4767,
    text_muted: 0x6a7592,
    text_label: 0x566180,

    color_head: 0xa5276b,   // pink label, darkened to 4.7:1
    color_branch: 0x2e549e, // blue drawn out of the #172540 text navy
    color_remote: 0x277c51, // green label, darkened
    color_tag: 0x70741b,    // olive label, darkened

    color_success: 0x277c51,
    color_warning: 0x8a5a10,
    color_blocker: 0xa32233,
    color_blocker_muted: 0xb08088,

    diff_added_bg: 0xdfeee5,   // the sage label as a wash
    diff_removed_bg: 0xf6e2ea, // the pink label as a wash
    diff_hunk: 0x2e549e,

    change_added: 0x277c51,
    change_modified: 0x8a5a10,
    change_deleted: 0xa32233,
    change_renamed: 0x2e549e,
    change_typechange: 0x87419f,
    change_dir: 0x566180,

    accent: 0xa5276b, // pink

    // The palette's own hues at a lightness that reads on the base (every lane
    // >= 4.4:1), ordered so adjacent indices stay distinct. Lane 7 is the
    // supplied #000 — the one label colour that needed no adjustment.
    lane_hsl: [
        (0.910, 0.62, 0.40), // pink   #a5276b
        (0.415, 0.52, 0.32), // green  #277c51
        (0.610, 0.55, 0.40), // blue   #2e549e
        (0.174, 0.62, 0.28), // olive  #70741b
        (0.500, 0.70, 0.30), // teal   #178282
        (0.790, 0.42, 0.44), // purple #87419f
        (0.065, 0.70, 0.36), // orange #9c4e1c
        (0.000, 0.00, 0.00), // black  #000000
    ],

    avatar_sat: 0.42,
    avatar_light: 0.46,

    term_bg: (0xf8, 0xfa, 0xff),
    term_fg: (0x17, 0x25, 0x40),
    term_cursor: (0xa5, 0x27, 0x6b),
    term_black: (0x17, 0x25, 0x40),
    term_red: (0xa3, 0x22, 0x33),
    term_green: (0x27, 0x7c, 0x51),
    term_yellow: (0x70, 0x74, 0x1b),
    term_blue: (0x2e, 0x54, 0x9e),
    term_magenta: (0xa5, 0x27, 0x6b),
    term_cyan: (0x17, 0x82, 0x82),
    term_white: (0x56, 0x61, 0x80),
    term_bright_black: (0x6a, 0x75, 0x92),
    term_bright_red: (0xc4, 0x3a, 0x4c),
    term_bright_green: (0x37, 0x9b, 0x69),
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
        string: 0x277c51,      // green
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

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_have_unique_slugs() {
        let mut slugs: Vec<&str> = THEMES.iter().map(|t| t.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), THEMES.len(), "theme slugs must be unique");
    }

    #[test]
    fn index_of_resolves_all_slugs() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(t.slug), Some(i));
        }
        assert_eq!(index_of("does-not-exist"), None);
    }

    /// `set_active` must store the *selected* theme's index — the picker reads
    /// it back via `active_index()`/`theme()`, and a `set_active` that always
    /// stored 0 passed the whole theme suite. Persists, hence the tempdir.
    #[test]
    fn set_active_selects_the_named_theme() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev = active_index();
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        // Every built-in theme, not just index 0.
        for (i, t) in THEMES.iter().enumerate() {
            assert!(set_active(t.slug), "{} must be recognised", t.slug);
            assert_eq!(
                active_index(),
                i,
                "active_index after set_active({})",
                t.slug
            );
            assert_eq!(theme().slug, t.slug, "theme() after set_active({})", t.slug);
        }

        // A retired slug resolves through the alias table; an unknown one is
        // rejected and leaves the active theme untouched.
        assert!(set_active("xcode-dark"));
        assert_eq!(theme().slug, "apple-dark");
        assert!(!set_active("nope"));
        assert_eq!(theme().slug, "apple-dark");

        ACTIVE.store(prev, Ordering::Relaxed);
        std::env::remove_var("KAGI_LOG_DIR");
    }

    #[test]
    fn lane_color_cycles() {
        let t = &THEMES[0];
        // 8-colour palette: lane 8 wraps back to lane 0.
        assert_eq!(t.lane_color(0), t.lane_color(8));
        assert_eq!(t.lane_color(3), t.lane_color(11));
    }

    #[test]
    fn zoom_clamps_to_bounds() {
        assert_eq!(clamp_zoom(0.5), ZOOM_MIN);
        assert_eq!(clamp_zoom(2.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(1.0), 1.0);
    }

    /// T-SYNTAX-001: every theme must produce a usable highlight theme —
    /// the JSON in `highlight_theme` is a format string, so a typo in any
    /// single theme would silently fall back to the bundled preset.
    #[test]
    fn every_theme_builds_a_syntax_palette() {
        for t in THEMES {
            let ht = highlight_theme(t);
            assert_eq!(ht.name, format!("kagi-{}", t.slug), "{} fell back", t.slug);
            let syn = &ht.style.syntax;
            for (name, present) in [
                ("keyword", syn.keyword.is_some()),
                ("string", syn.string.is_some()),
                ("comment", syn.comment.is_some()),
                ("type", syn.type_.is_some()),
                ("function", syn.function.is_some()),
                ("number", syn.number.is_some()),
                ("variable", syn.variable.is_some()),
                ("attribute", syn.attribute.is_some()),
                // The tokens the bundled palette omitted entirely, which is
                // why identifiers/operators/punctuation used to render as
                // plain text in every theme.
                ("operator", syn.operator.is_some()),
                ("punctuation", syn.punctuation.is_some()),
            ] {
                assert!(present, "{}: syntax.{} missing", t.slug, name);
            }
        }
    }

    /// The actual reported bug: Apple Dark and Catppuccin Mocha highlighted
    /// code identically because only `dark` was consulted.
    #[test]
    fn dark_themes_do_not_share_one_syntax_palette() {
        let apple = THEMES.iter().find(|t| t.slug == "apple-dark").unwrap();
        let mocha = THEMES.iter().find(|t| t.slug == "catppuccin").unwrap();
        assert!(apple.dark && mocha.dark);
        assert_ne!(apple.syntax, mocha.syntax);
        assert_ne!(apple.syntax.keyword, mocha.syntax.keyword);
    }

    /// Every syntax colour must stay legible on its own theme's background.
    ///
    /// This is the guard for a real bug: Pinky Boo inherited several token
    /// colours unchanged from its dark ancestor, and on its near-white
    /// background `string` measured 1.9:1 — the user saw code "turn white".
    ///
    /// The text-selection tint must actually differ from the surface it is
    /// drawn on. It used to be `selected` — the list-row neutral — which on an
    /// Input's background was invisible (user report: selecting text showed no
    /// highlight at all).
    #[test]
    fn selection_tint_is_distinguishable_from_the_input_background() {
        for t in THEMES {
            let (r, g, b) = (
                (t.color_branch >> 16) & 0xff,
                (t.color_branch >> 8) & 0xff,
                t.color_branch & 0xff,
            );
            let (br, bg_, bb) = (
                (t.bg_base >> 16) & 0xff,
                (t.bg_base >> 8) & 0xff,
                t.bg_base & 0xff,
            );
            // The tint as composited over the Input background.
            let blend = |fg: u32, bg: u32| {
                (fg as f32 * SELECTION_ALPHA + bg as f32 * (1.0 - SELECTION_ALPHA)) as i32
            };
            let delta = (blend(r, br) - br as i32).abs()
                + (blend(g, bg_) - bg_ as i32).abs()
                + (blend(b, bb) - bb as i32).abs();
            assert!(
                delta >= 24,
                "{}: selection tint is only {} away from the input background",
                t.slug,
                delta
            );
        }
    }

    /// `comment` is exempt: every theme deliberately mutes comments, and
    /// upstream palettes routinely put them at 2.5:1.
    #[test]
    fn syntax_colours_are_legible_on_their_background() {
        /// WCAG relative luminance.
        fn luminance(c: u32) -> f64 {
            let ch = |v: u32| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch((c >> 16) & 0xff) + 0.7152 * ch((c >> 8) & 0xff) + 0.0722 * ch(c & 0xff)
        }
        fn contrast(a: u32, b: u32) -> f64 {
            let (la, lb) = (luminance(a), luminance(b));
            (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
        }

        // Deliberately below WCAG AA (4.5) — several *officially published*
        // palettes (notably Catppuccin) sit in the 3s by design, and matching
        // upstream matters more than beating a threshold they never targeted.
        // 3.0 catches "illegible", not "lower contrast than I'd choose".
        const MIN: f64 = 3.0;

        // Catppuccin Latte ships these exact values upstream (Green #40a02b,
        // Yellow #df8e1d, … on Base #eff1f5) and sits in the 2.3-3.0 band by
        // its own design. Deviating would misrepresent a palette people pick
        // *because* they know how it looks, so it is exempted rather than
        // "corrected" — deliberately listed here so the choice stays visible.
        // Every other theme is held to MIN.
        const LOW_CONTRAST_BY_UPSTREAM_DESIGN: &[&str] = &["catppuccin-latte"];

        for t in THEMES {
            if LOW_CONTRAST_BY_UPSTREAM_DESIGN.contains(&t.slug) {
                continue;
            }
            let s = &t.syntax;
            for (name, colour) in [
                ("keyword", s.keyword),
                ("string", s.string),
                ("type_name", s.type_name),
                ("function", s.function),
                ("number", s.number),
                ("operator", s.operator),
                ("punctuation", s.punctuation),
                ("variable", s.variable),
                ("attribute", s.attribute),
            ] {
                let c = contrast(colour, t.bg_base);
                assert!(
                    c >= MIN,
                    "{}: syntax.{} {:#08x} is {:.1}:1 on bg {:#08x} — illegible",
                    t.slug,
                    name,
                    colour,
                    c,
                    t.bg_base
                );
            }
        }
    }

    /// Retired Xcode slugs must resolve to their Apple successors, so an
    /// existing settings.json doesn't silently drop to the default theme.
    #[test]
    fn legacy_xcode_slugs_alias_to_apple() {
        assert_eq!(index_of("xcode-dark"), index_of("apple-dark"));
        assert_eq!(index_of("xcode-light"), index_of("apple-light"));
        assert!(index_of("apple-dark").is_some());
        assert_eq!(index_of("still-not-a-theme"), None);
    }

    #[test]
    fn dark_and_light_counts() {
        let dark = THEMES.iter().filter(|t| t.dark).count();
        let light = THEMES.iter().filter(|t| !t.dark).count();
        // catppuccin, one-dark, monokai, tokyo-night, ibm-pc, dracula, apple-dark
        assert_eq!(dark, 7);
        // one-light, pinky-boo, catppuccin-latte, apple-light, periwinkle
        assert_eq!(light, 5);
    }
}
