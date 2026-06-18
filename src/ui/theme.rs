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
//! * [`THEMES`] lists the 6 built-in themes; index 0 (Catppuccin Mocha) is the
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

use super::settings::{read_setting, write_setting, Settings};

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

    // ── Graph lane palette (6 cycling colours, HSLA components) ───
    /// `(hue, saturation, lightness)` for each lane; alpha is always 1.0.
    pub lane_hsl: [(f32, f32, f32); 6],

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
}

impl Theme {
    /// HSLA colour for graph lane `i` (cycles through the 6-colour palette).
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
/// Dark themes: tinted chip — the ref colour at low alpha for the fill, a
/// stronger alpha for the border, white text. Light themes keep the solid
/// chip (tints wash out on light backgrounds).
///
/// Returns `(bg_rgba, border_rgba, text_rgb)` for use with
/// `gpui::rgba` / `gpui::rgb`.
#[inline]
pub fn badge_style(color: u32) -> (u32, u32, u32) {
    let t = theme();
    if t.dark {
        // 0x33 ≈ 20% fill, 0x66 ≈ 40% border (rgitui grammar).
        ((color << 8) | 0x33, (color << 8) | 0x66, 0xffffff)
    } else {
        // Solid chip: opaque fill/border, dark text from the theme base.
        ((color << 8) | 0xff, (color << 8) | 0xff, t.bg_base)
    }
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
    THEMES.iter().position(|t| t.slug == slug)
}

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

    // ── Surfaces ────────────────────────────────────────────────
    gc.colors.background = to_hsla(k.bg_base);
    gc.colors.foreground = to_hsla(k.text_main);
    gc.colors.border = to_hsla(k.selected);
    gc.colors.muted = to_hsla(k.surface);
    gc.colors.muted_foreground = to_hsla(k.text_muted);

    // ── Popover / overlay / selection (Tooltip, modals, Input) ──
    gc.colors.popover = to_hsla(k.modal);
    gc.colors.popover_foreground = to_hsla(k.text_main);
    gc.colors.overlay = to_hsla(k.modal_overlay);
    gc.colors.selection = to_hsla(k.selected);

    // ── Primary / accent (Checkbox checked, focus ring, links) ──
    gc.colors.primary = to_hsla(k.color_branch);
    gc.colors.primary_foreground = to_hsla(k.bg_base);
    gc.colors.primary_hover = to_hsla(k.color_branch);
    gc.colors.primary_active = to_hsla(k.color_branch);
    gc.colors.ring = to_hsla(k.color_branch);
    gc.colors.accent = to_hsla(k.selected);
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
    gc.font_family = super::UI_FONT.into();
    gc.mono_font_family = super::MONO_FONT.into();

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
    let mut ht = if k.dark {
        (*gpui_component::highlighter::HighlightTheme::default_dark()).clone()
    } else {
        (*gpui_component::highlighter::HighlightTheme::default_light()).clone()
    };
    ht.style.editor_background = Some(to_hsla(k.bg_base));
    ht.style.editor_foreground = Some(to_hsla(k.text_main));
    ht.style.editor_active_line = Some(to_hsla(k.bg_row_alt));
    ht.style.editor_line_number = Some(to_hsla(k.text_muted));
    ht.style.editor_active_line_number = Some(to_hsla(k.text_sub));
    gc.highlight_theme = std::sync::Arc::new(ht);
}

// ──────────────────────────────────────────────────────────────────────────
// Theme registry — 6 built-in themes
// ──────────────────────────────────────────────────────────────────────────

/// All built-in themes.  Index 0 (Catppuccin Mocha) is the default.
pub static THEMES: &[Theme] = &[
    CATPPUCCIN_MOCHA,
    XCODE_DARK,
    XCODE_LIGHT,
    ONE_DARK,
    ONE_LIGHT,
    MONOKAI,
    TOKYO_NIGHT,
    IBM_PC,
];

// ── Catppuccin Mocha (default) ───────────────────────────────────────────
//
// Byte-exact port of the previous hard-coded constants (mod.rs etc.).  The
// lane HSL values reproduce the previous `graph_view::lane_color` palette;
// avatar sat/light reproduce `avatar::avatar_color` (0.70 / 0.60); terminal
// values reproduce `terminal.rs`.
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

    lane_hsl: [
        (0.583, 0.75, 0.65), // blue
        (0.333, 0.75, 0.65), // green
        (0.083, 0.75, 0.65), // yellow/gold
        (0.917, 0.75, 0.65), // pink
        (0.750, 0.75, 0.65), // purple
        (0.500, 0.75, 0.65), // cyan
    ],

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
};

// ── Xcode Dark ────────────────────────────────────────────────────────────
//
// Palette from Apple's Xcode "Default (Dark)" theme: editor bg #292a30,
// source-editor text #ffffff, syntax accent colours (keyword pink #ff7ab2,
// string red-orange #ff8170, type teal #6bdfff, number #d9c97c, etc.).
const XCODE_DARK: Theme = Theme {
    slug: "xcode-dark",
    name: "Xcode Dark",
    dark: true,

    bg_base: 0x292a30,
    bg_row_alt: 0x25262b,
    surface: 0x3a3c44,
    selected: 0x4b4e58,
    panel: 0x1f2024,
    sidebar: 0x191a1f,
    modal: 0x3a3c44,
    modal_overlay: 0x000000,

    text_main: 0xdfdfe0,
    text_sub: 0xb0b3bb,
    text_muted: 0x7f8493,
    text_label: 0x6c7080,

    color_head: 0xff8170,   // red-orange (strings)
    color_branch: 0x6bb0ff, // blue
    color_remote: 0x78c2b3, // teal/green
    color_tag: 0xd9c97c,    // sand/number

    color_success: 0x78c2b3,
    color_warning: 0xd9c97c,
    color_blocker: 0xff8170,
    color_blocker_muted: 0x8a544e,

    diff_added_bg: 0x1f3a2b,
    diff_removed_bg: 0x3a2222,
    diff_hunk: 0x6bb0ff,

    change_added: 0x78c2b3,
    change_modified: 0xd9c97c,
    change_deleted: 0xff8170,
    change_renamed: 0x6bb0ff,
    change_typechange: 0x7f8493,
    change_dir: 0x6c7080,

    accent: 0xdabaff, // purple (keyword-ish)

    lane_hsl: [
        (0.585, 0.70, 0.66),
        (0.430, 0.45, 0.60),
        (0.130, 0.55, 0.66),
        (0.020, 1.00, 0.72),
        (0.770, 0.65, 0.74),
        (0.500, 0.85, 0.70),
    ],

    avatar_sat: 0.60,
    avatar_light: 0.58,

    term_bg: (0x29, 0x2a, 0x30),
    term_fg: (0xdf, 0xdf, 0xe0),
    term_cursor: (0xff, 0xff, 0xff),
    term_black: (0x41, 0x43, 0x4a),
    term_red: (0xff, 0x81, 0x70),
    term_green: (0x78, 0xc2, 0xb3),
    term_yellow: (0xd9, 0xc9, 0x7c),
    term_blue: (0x6b, 0xb0, 0xff),
    term_magenta: (0xff, 0x7a, 0xb2),
    term_cyan: (0x6b, 0xdf, 0xff),
    term_white: (0xdf, 0xdf, 0xe0),
    term_bright_black: (0x7f, 0x84, 0x93),
    term_bright_red: (0xff, 0x8a, 0x7a),
    term_bright_green: (0x83, 0xc9, 0xba),
    term_bright_yellow: (0xff, 0xee, 0x9c),
    term_bright_blue: (0x4e, 0xb0, 0xcc),
    term_bright_magenta: (0xff, 0x85, 0xb8),
    term_bright_cyan: (0x8b, 0xe9, 0xff),
    term_bright_white: (0xff, 0xff, 0xff),
    term_selection: (0x64, 0x69, 0x78, 0x99),
};

// ── Xcode Light ───────────────────────────────────────────────────────────
//
// Palette from Apple's Xcode "Default (Light)" theme: editor bg #ffffff,
// text #000000, keyword #9b2393, string #c41a16, type #0b4f79, number #1c00cf,
// comment #5d6c79.
const XCODE_LIGHT: Theme = Theme {
    slug: "xcode-light",
    name: "Xcode Light",
    dark: false,

    bg_base: 0xffffff,
    bg_row_alt: 0xf4f5f7,
    surface: 0xeceded,
    selected: 0xd5e3f7,
    panel: 0xf6f6f6,
    sidebar: 0xeceef1,
    modal: 0xffffff,
    modal_overlay: 0x32384a,

    text_main: 0x1a1a1a,
    text_sub: 0x4c4f54,
    text_muted: 0x8a8f99,
    text_label: 0x6f747e,

    color_head: 0xc41a16,   // string red
    color_branch: 0x0b4f79, // type blue
    color_remote: 0x2e8b57, // green
    color_tag: 0xb06000,    // amber

    color_success: 0x2e8b57,
    color_warning: 0xb06000,
    color_blocker: 0xc41a16,
    color_blocker_muted: 0xc98a87,

    diff_added_bg: 0xd6f2df,
    diff_removed_bg: 0xfbdcdc,
    diff_hunk: 0x0b4f79,

    change_added: 0x2e8b57,
    change_modified: 0xb06000,
    change_deleted: 0xc41a16,
    change_renamed: 0x0b4f79,
    change_typechange: 0x8a8f99,
    change_dir: 0x6f747e,

    accent: 0x9b2393, // keyword magenta

    lane_hsl: [
        (0.585, 0.70, 0.45),
        (0.380, 0.55, 0.38),
        (0.090, 0.85, 0.42),
        (0.940, 0.70, 0.48),
        (0.800, 0.55, 0.48),
        (0.520, 0.65, 0.42),
    ],

    avatar_sat: 0.55,
    avatar_light: 0.45,

    term_bg: (0xff, 0xff, 0xff),
    term_fg: (0x1a, 0x1a, 0x1a),
    term_cursor: (0x00, 0x00, 0x00),
    term_black: (0x32, 0x33, 0x37),
    term_red: (0xc4, 0x1a, 0x16),
    term_green: (0x2e, 0x8b, 0x57),
    term_yellow: (0xb0, 0x60, 0x00),
    term_blue: (0x0b, 0x4f, 0x79),
    term_magenta: (0x9b, 0x23, 0x93),
    term_cyan: (0x1c, 0x6f, 0x8b),
    term_white: (0xc8, 0xc8, 0xc8),
    term_bright_black: (0x8a, 0x8f, 0x99),
    term_bright_red: (0xd1, 0x2f, 0x1b),
    term_bright_green: (0x3c, 0xa0, 0x68),
    term_bright_yellow: (0xc8, 0x76, 0x00),
    term_bright_blue: (0x14, 0x66, 0x9b),
    term_bright_magenta: (0xb0, 0x3a, 0xa8),
    term_bright_cyan: (0x2a, 0x8a, 0xab),
    term_bright_white: (0x1a, 0x1a, 0x1a),
    term_selection: (0xb3, 0xcf, 0xf2, 0xcc),
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

    lane_hsl: [
        (0.585, 0.80, 0.66),
        (0.270, 0.42, 0.62),
        (0.110, 0.66, 0.69),
        (0.980, 0.70, 0.65),
        (0.810, 0.62, 0.67),
        (0.520, 0.45, 0.55),
    ],

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

    lane_hsl: [
        (0.605, 0.86, 0.60),
        (0.330, 0.34, 0.47),
        (0.090, 0.99, 0.38),
        (0.020, 0.74, 0.59),
        (0.825, 0.63, 0.40),
        (0.545, 0.99, 0.37),
    ],

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

    lane_hsl: [
        (0.585, 1.00, 0.70), // blue   (sat +0.05, l -0.01)
        (0.260, 0.65, 0.63), // green  (sat +0.10)
        (0.085, 1.00, 0.63), // orange (l +0.01)
        (0.945, 1.00, 0.70), // pink   (l -0.01)
        (0.730, 0.90, 0.76), // purple (sat +0.06)
        (0.510, 0.80, 0.70), // cyan   (sat +0.09)
    ],

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

    lane_hsl: [
        (0.600, 0.80, 0.72), // blue
        (0.250, 0.55, 0.61), // green
        (0.100, 0.63, 0.64), // yellow
        (0.960, 0.85, 0.72), // red/pink
        (0.740, 0.80, 0.78), // purple
        (0.550, 0.90, 0.74), // cyan
    ],

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

    lane_hsl: [
        (0.667, 1.0, 0.67), // bright blue
        (0.333, 1.0, 0.67), // bright green
        (0.167, 1.0, 0.67), // yellow
        (0.000, 1.0, 0.67), // bright red
        (0.833, 1.0, 0.67), // bright magenta
        (0.500, 1.0, 0.67), // bright cyan
    ],

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
};

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_themes_with_unique_slugs() {
        assert_eq!(THEMES.len(), 8);
        let mut slugs: Vec<&str> = THEMES.iter().map(|t| t.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), 8, "theme slugs must be unique");
    }

    #[test]
    fn default_is_catppuccin_exact() {
        // The default (index 0) must byte-match the previous hard-coded values.
        let t = &THEMES[0];
        assert_eq!(t.slug, "catppuccin");
        assert!(t.dark);
        assert_eq!(t.bg_base, 0x1e1e2e);
        assert_eq!(t.surface, 0x313244);
        assert_eq!(t.selected, 0x45475a);
        assert_eq!(t.panel, 0x181825);
        assert_eq!(t.sidebar, 0x11111b);
        assert_eq!(t.text_main, 0xcdd6f4);
        assert_eq!(t.color_head, 0xf38ba8);
        assert_eq!(t.color_branch, 0x89b4fa);
        assert_eq!(t.color_remote, 0xa6e3a1);
        assert_eq!(t.color_tag, 0xfab387);
        assert_eq!(t.diff_added_bg, 0x1c3a2a);
        assert_eq!(t.diff_removed_bg, 0x3a1c1c);
        assert_eq!(t.avatar_sat, 0.70);
        assert_eq!(t.avatar_light, 0.60);
        assert_eq!(t.term_selection, (0x58, 0x5b, 0x70, 0x99));
    }

    #[test]
    fn index_of_resolves_all_slugs() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(t.slug), Some(i));
        }
        assert_eq!(index_of("does-not-exist"), None);
    }

    #[test]
    fn lane_color_cycles() {
        let t = &THEMES[0];
        // lane 6 wraps to lane 0.
        assert_eq!(t.lane_color(0), t.lane_color(6));
    }

    #[test]
    fn zoom_clamps_to_bounds() {
        assert_eq!(clamp_zoom(0.5), ZOOM_MIN);
        assert_eq!(clamp_zoom(2.0), ZOOM_MAX);
        assert_eq!(clamp_zoom(1.0), 1.0);
        // rem_size_px tracks the default 1.0x (no settings.json mutation here).
        assert!((BASE_REM_PX - 16.0).abs() < f32::EPSILON);
    }

    // T-SETTINGS-001: the Settings Theme Select maps slug ↔ index purely; the
    // round-trip must be lossless for every built-in theme (the Select renders
    // by slug and reuses `set_active(slug)` / `active_index()` to apply).
    #[test]
    fn theme_slug_index_roundtrip() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(t.slug), Some(i));
            assert_eq!(THEMES[i].slug, t.slug);
        }
    }

    #[test]
    fn dark_and_light_counts() {
        let dark = THEMES.iter().filter(|t| t.dark).count();
        let light = THEMES.iter().filter(|t| !t.dark).count();
        // catppuccin, xcode-dark, one-dark, monokai, tokyo-night, ibm-pc
        assert_eq!(dark, 6);
        assert_eq!(light, 2); // xcode-light, one-light
    }
}
