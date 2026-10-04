//! W9-THEME / ADR-0036: single-source colour theme registry.
//!
//! All UI colour comes from the active [`Theme`].  Modules call [`theme()`]
//! every render frame, so switching a theme is just an atomic/slot update +
//! `cx.notify()` — no signature churn anywhere.
//!
//! # Design
//!
//! * [`Theme`] holds **semantic** `u32` RGB fields (e.g. `bg_base`, `text_main`,
//!   `color_branch`) plus a few non-RGB values (lane HSLA palette, avatar
//!   saturation/lightness, terminal selection alpha) and a `dark: bool` flag.
//! * [`THEMES`] lists the built-in themes; index 0 (Catppuccin Mocha) is the
//!   default and a byte-exact port of the previously hard-coded constants, so
//!   the default look has zero regression.
//! * User themes (#922 / ADR-0220) are JSON files parsed by
//!   [`crate::theme_custom`] into owned `Arc<Theme>` snapshots. The runtime
//!   list is `THEMES` followed by those; [`ThemeHandle`] refers to either kind
//!   without leaking, so a reload frees a generation once nothing uses it.
//! * [`ACTIVE`] mirrors the active built-in index (lock-free fast path for
//!   [`theme()`]); a custom theme is held in the registry's active slot.
//!   [`set_active`] updates both (and persists to `settings.json`).
//!
//! # Persistence
//!
//! The active theme slug is stored in `~/.kagi/settings.json` through the
//! `settings` store, honouring `KAGI_LOG_DIR`.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use gpui::{hsla, rgb, App, Hsla};
use serde::Serialize;

use crate::settings::{read_setting, write_setting, Settings};

pub use crate::theme_custom::{themes_dir, ThemeLoadError};

// Compatibility re-export: font constants historically lived in `theme`.
pub use crate::fonts::{ui_font, CJK_FONT, MONO_FONT, UI_FONT};

// ──────────────────────────────────────────────────────────────────────────
// Theme struct
// ──────────────────────────────────────────────────────────────────────────

/// A complete colour theme.  All colour fields are `0xRRGGBB` `u32` (consumed
/// by `gpui::rgb`) except the lane palette (HSLA), the avatar saturation /
/// lightness scalars, and the terminal selection alpha.
#[derive(Clone, Debug, Serialize)]
pub struct Theme {
    /// Stable lowercase slug used for menus, settings, and `KAGI_THEME`.
    pub slug: Cow<'static, str>,
    /// Human-readable name shown in the View → Theme menu.
    pub name: Cow<'static, str>,
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
    /// Tint painted under selected text (Inputs, selectable `TextView`s).
    ///
    /// Its own token rather than `color_branch`, which it used to borrow: in a
    /// theme whose accent shares a hue with `diff_removed_bg`, selecting a
    /// context line in a diff produced the exact colour of a removed line
    /// (measured 12 apart, channel sum). Every theme but Flower Road keeps the
    /// old value, so this is inert for them.
    pub selection_tint: u32,

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
    /// Name for menus and Settings. Brand names stay as they are; the
    /// descriptive accessibility theme is localized (#354).
    pub fn display_name(&self) -> &str {
        match &*self.slug {
            "color-vision" => crate::i18n::Msg::ThemeColorVision.t(),
            _ => &self.name,
        }
    }

    /// The accent (`color_branch`) as text on `background`, kept readable
    /// (WCAG AA, 4.5:1): unchanged when it already is, otherwise the same hue
    /// and saturation with its lightness stepped down (light background) or up
    /// (dark one) until it reads. Flower Road's pink accent on its beige
    /// `surface` was 1.6:1 on the selected sidebar tab; it stays pink, deeper.
    pub fn accent_text_on(&self, background: u32) -> u32 {
        const MIN: f64 = 4.5;
        let accent = self.color_branch;
        if contrast_ratio(accent, background) >= MIN {
            return accent;
        }
        let base = to_hsla(accent);
        let step = if relative_luminance(background) > 0.5 {
            -0.005
        } else {
            0.005
        };
        let to_u32 = |c: Hsla| {
            let c: gpui::Rgba = c.into();
            let q = |x: f32| -> u32 { (x.clamp(0.0, 1.0) * 255.0).round() as u32 };
            (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
        };
        (1..=200)
            .map(|i| {
                to_u32(hsla(
                    base.h,
                    base.s,
                    (base.l + step * i as f32).clamp(0.0, 1.0),
                    1.0,
                ))
            })
            .find(|&c| contrast_ratio(c, background) >= MIN)
            .unwrap_or(self.text_main)
    }

    /// HSLA colour for graph lane `i` (cycles through the 8-colour palette).
    pub fn lane_color(&self, i: usize) -> Hsla {
        let (h, s, l) = self.lane_hsl[i % self.lane_hsl.len()];
        hsla(h, s, l, 1.0)
    }
}
// ──────────────────────────────────────────────────────────────────────────
// Runtime theme registry (built-ins + user themes) and the active theme
// ──────────────────────────────────────────────────────────────────────────

/// A cheap, owned reference to one theme: a built-in `&'static Theme` or a
/// user theme's shared snapshot. Cloning never allocates (the custom case is
/// an `Arc` increment), and a handle stays valid after a reload replaces or
/// removes its theme — the snapshot is freed when the last handle drops.
#[derive(Clone, Debug)]
pub struct ThemeHandle(HandleInner);

#[derive(Clone, Debug)]
enum HandleInner {
    Builtin(usize),
    Custom(u64, Arc<Theme>),
}

/// Identity of one theme *instance*, for caches keyed by "the theme this was
/// computed with". A reloaded user theme gets a new key even when its slug is
/// unchanged; keys are never reused within a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThemeKey(KeyInner);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum KeyInner {
    Builtin(usize),
    Custom(u64),
}

impl ThemeHandle {
    #[inline]
    fn builtin(i: usize) -> Self {
        Self(HandleInner::Builtin(i))
    }

    /// This theme instance's identity (see [`ThemeKey`]).
    #[inline]
    pub fn key(&self) -> ThemeKey {
        ThemeKey(match &self.0 {
            HandleInner::Builtin(i) => KeyInner::Builtin(*i),
            HandleInner::Custom(id, _) => KeyInner::Custom(*id),
        })
    }

    /// Whether this is a user theme loaded from a JSON file.
    #[inline]
    pub fn is_custom(&self) -> bool {
        matches!(self.0, HandleInner::Custom(..))
    }
}

impl Deref for ThemeHandle {
    type Target = Theme;

    #[inline]
    fn deref(&self) -> &Theme {
        match &self.0 {
            HandleInner::Builtin(i) => &THEMES[*i],
            HandleInner::Custom(_, t) => t,
        }
    }
}

/// The loaded user themes plus the active selection. Readers clone handles
/// out under the read lock; a reload swaps the whole list under the write
/// lock, so no reader ever sees half a generation.
struct Registry {
    /// User themes in load (file-name) order.
    custom: Vec<ThemeHandle>,
    /// The active theme; authoritative whenever [`ACTIVE`] is [`CUSTOM_ACTIVE`].
    active: ThemeHandle,
}

static REGISTRY: RwLock<Registry> = RwLock::new(Registry {
    custom: Vec::new(),
    active: ThemeHandle(HandleInner::Builtin(0)),
});

/// Source of [`ThemeKey`]s for user themes; never reused.
static NEXT_CUSTOM_ID: AtomicU64 = AtomicU64::new(0);

/// Load errors from startup, held until the UI can show them.
static STARTUP_ERRORS: Mutex<Vec<ThemeLoadError>> = Mutex::new(Vec::new());

/// [`ACTIVE`] value meaning "a user theme is active; read the registry".
const CUSTOM_ACTIVE: usize = usize::MAX;

/// Index into [`THEMES`] of the active built-in theme, or [`CUSTOM_ACTIVE`].
/// Defaults to 0 (Catppuccin Mocha). Written only under the registry's write
/// lock, so it always agrees with `Registry::active`.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn registry() -> RwLockReadGuard<'static, Registry> {
    REGISTRY.read().unwrap_or_else(|e| e.into_inner())
}

fn registry_mut() -> RwLockWriteGuard<'static, Registry> {
    REGISTRY.write().unwrap_or_else(|e| e.into_inner())
}

impl Registry {
    fn activate(&mut self, handle: ThemeHandle) {
        let index = match handle.0 {
            HandleInner::Builtin(i) => i,
            HandleInner::Custom(..) => CUSTOM_ACTIVE,
        };
        self.active = handle;
        ACTIVE.store(index, Ordering::Release);
    }

    fn find(&self, slug: &str) -> Option<ThemeHandle> {
        match index_of(slug) {
            Some(i) => Some(ThemeHandle::builtin(i)),
            None => self.custom.iter().find(|t| t.slug == slug).cloned(),
        }
    }
}

/// The currently-active theme.  Called from every render path: a built-in
/// theme is a lock-free atomic read, a user theme an `Arc` clone.
#[inline]
pub fn theme() -> ThemeHandle {
    let i = ACTIVE.load(Ordering::Acquire);
    if i < THEMES.len() {
        return ThemeHandle::builtin(i);
    }
    registry().active.clone()
}

/// Every selectable theme: [`THEMES`] in order, then the loaded user themes
/// in file-name order.
pub fn all_themes() -> Vec<ThemeHandle> {
    let reg = registry();
    (0..THEMES.len())
        .map(ThemeHandle::builtin)
        .chain(reg.custom.iter().cloned())
        .collect()
}

/// The theme with this slug (built-in, including retired aliases, or user).
pub fn theme_by_slug(slug: &str) -> Option<ThemeHandle> {
    registry().find(slug)
}

/// Re-read the user theme folder ([`themes_dir`]) without changing the active
/// registry. May perform slow filesystem I/O and JSON parsing; callers with a
/// live window must run this on a background executor. `Err` when the folder
/// exists but cannot be listed: keep the themes already installed.
pub fn read_custom_themes() -> Result<(Vec<Theme>, Vec<ThemeLoadError>), ThemeLoadError> {
    match themes_dir() {
        Some(dir) => crate::theme_custom::load_dir(&dir),
        None => Ok((Vec::new(), Vec::new())),
    }
}

/// Re-read and install user themes synchronously before the first window exists.
/// The native UI reload uses `read_custom_themes` off-thread and calls
/// `install_custom_themes` on the foreground after checking its request version.
/// An unlistable folder installs nothing and is reported like a bad file.
pub fn reload_custom_themes() -> Vec<ThemeLoadError> {
    match read_custom_themes() {
        Ok((themes, errors)) => {
            install_custom_themes(themes);
            errors
        }
        Err(error) => vec![error],
    }
}

/// Replace the user-theme generation and re-resolve the active theme.
/// The selected slug survives a reload if still valid, otherwise it falls back
/// to the default for this run without rewriting `settings.json`.
pub fn install_custom_themes(themes: Vec<Theme>) {
    let custom: Vec<ThemeHandle> = themes
        .into_iter()
        .map(|t| {
            let id = NEXT_CUSTOM_ID.fetch_add(1, Ordering::Relaxed);
            ThemeHandle(HandleInner::Custom(id, Arc::new(t)))
        })
        .collect();
    let mut reg = registry_mut();
    let old = std::mem::replace(&mut reg.custom, custom);
    if reg.active.is_custom() {
        let next = reg
            .custom
            .iter()
            .find(|t| t.slug == reg.active.slug)
            .cloned()
            .unwrap_or_else(|| ThemeHandle::builtin(0));
        reg.activate(next);
    }
    drop(reg);
    // The previous generation is freed here unless a caller still holds it.
    drop(old);
}

/// Errors from loading user themes at startup ([`init_active`]); returns
/// them once so the UI can report each file after it is ready.
pub fn take_theme_load_errors() -> Vec<ThemeLoadError> {
    std::mem::take(&mut *STARTUP_ERRORS.lock().unwrap_or_else(|e| e.into_inner()))
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

/// Fill + border for an inset panel inside a modal card (#454 section panels).
///
/// Derived from the theme's own text colour at low alpha rather than from the
/// `surface` token, because `surface == modal` in several themes — Apple Dark
/// (`0x2c2c2e` for both), IBM PC, Monokai, One Light — where a `surface` panel
/// on a `modal` card is invisible and the card reads as the flat text column
/// the panels were meant to replace (user report 2026-09-06 on Apple Dark,
/// after the same panels looked right on Catppuccin Mocha).
///
/// Text is light on dark themes and dark on light ones, so one alpha lifts the
/// panel *away* from the card in both directions: slightly lighter on a dark
/// card, slightly darker on a light one.
///
/// Returns `(bg_rgba, border_rgba)` for `gpui::rgba`.
#[inline]
pub fn panel_style() -> (u32, u32) {
    let t = theme();
    // 0x0f ≈ 6% fill, 0x2b ≈ 17% border: enough to read as a box at a glance
    // without turning the card into a stack of filled blocks.
    let tint = if t.dark { 0xffffff } else { t.text_main };
    ((tint << 8) | 0x0f, (tint << 8) | 0x2b)
}

/// The active theme's lane colour `i` packed as a `0xRRGGBB` u32, for feeding
/// the lane hue into [`badge_style`] (lane-driven ref pills in swimlane mode).
#[inline]
pub fn lane_color_u32(i: usize) -> u32 {
    let c: gpui::Rgba = theme().lane_color(i).into();
    let q = |x: f32| -> u32 { (x.clamp(0.0, 1.0) * 255.0).round() as u32 };
    (q(c.r) << 16) | (q(c.g) << 8) | q(c.b)
}

/// Every theme ordered by name (case-insensitive), for every list a user
/// picks from. The key is the theme's own `name`, not the localized display
/// name: a Settings list built in one language stays in order after
/// switching to the other (#921 review). [`all_themes`] keeps its own order:
/// index 0 is the default theme.
pub fn themes_by_name() -> Vec<ThemeHandle> {
    let mut themes = all_themes();
    themes.sort_by_cached_key(|t| t.name.to_lowercase());
    themes
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

/// Base (1.0×) rem size in pixels; matches the previous 0.9× visual scale.
pub const BASE_REM_PX: f32 = 14.4;

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

/// Move to the next 10% preset in the requested direction. Migrated zooms
/// between presets (e.g. 1111 permille) land on 1200 or 1100 rather than
/// inheriting that offset on every subsequent click.
pub fn step_zoom(z: f32, increase: bool) -> f32 {
    let step = (ZOOM_STEP * 1000.0).round() as i32;
    let current = (clamp_zoom(z) * 1000.0).round() as i32;
    let next = if increase {
        (current / step + 1) * step
    } else {
        ((current - 1) / step) * step
    };
    (next as f32 / 1000.0).clamp(ZOOM_MIN, ZOOM_MAX)
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
/// layout constant as `scaled_px(N)` makes it track the same physical factor
/// as the text: the 14.4px rem base relative to gpui's 16px default, times
/// `zoom()`.
///
/// Use for **layout** dimensions, not for text sizes (text already scales via
/// rem). Callers that want crisp 1px hairlines may keep a literal `px(1.)`.
#[inline]
pub fn scaled_px(n: f32) -> gpui::Pixels {
    gpui::px(n * (BASE_REM_PX / 16.0) * zoom())
}

/// W28: bare-`f32` sibling of [`scaled_px`] for coordinate math.
///
/// The commit-graph path-builder computes lane x-centres, node radii, corner
/// radii and edge widths as plain `f32` before wrapping the final point in
/// `px(..)`. Routing those intermediate values through `scaled(..)` makes the
/// graph geometry track the same physical factor as the (rem-scaled) row text,
/// so the graph stays aligned. Identical to `scaled_px` except it returns the
/// bare `f32` instead of `Pixels`.
#[inline]
pub fn scaled(n: f32) -> f32 {
    n * (BASE_REM_PX / 16.0) * zoom()
}

/// Last known window (viewport) height in logical pixels; `0` = not yet seen.
///
/// #454: modal list boxes cap their height so a long file/commit list cannot
/// bury the confirm button, and that cap has to follow the **window**, not a
/// hard-coded row count — a 2200px-tall window should show far more rows than
/// a 700px one.  The value is read by leaf renderers deep inside the modal
/// tree (`modal_shell`, the plan card, the amend/discard lists) which have no
/// `&Window`: the shared plan card alone has 20 call sites that would each
/// have to grow a `window` parameter to pass it down.  So it lives here next
/// to `zoom()` — the same frame-global-UI-scalar idiom as `UI_ZOOM_PERMILLE`
/// and `REDUCE_MOTION` — and is refreshed once per frame from the one place
/// that owns the authoritative value, `KagiApp::render`.
static VIEWPORT_H: AtomicUsize = AtomicUsize::new(0);

/// Publish this frame's viewport height. Called once per frame from `render`.
#[inline]
pub fn set_viewport_h(h: f32) {
    VIEWPORT_H.store(h.max(0.0).round() as usize, Ordering::Relaxed);
}

/// This frame's viewport height, or `None` before the first frame (headless
/// snapshot tests, `--headless` startup) so callers can fall back to a
/// deterministic size instead of laying out against a bogus `0`.
#[inline]
pub fn viewport_h() -> Option<f32> {
    match VIEWPORT_H.load(Ordering::Relaxed) {
        0 => None,
        h => Some(h as f32),
    }
}
/// Last viewport width in logical pixels, for fitting MD confirmation cards
/// against the actual window rather than a self-relative flex percentage.
static VIEWPORT_W: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn set_viewport_w(w: f32) {
    VIEWPORT_W.store(w.max(0.0).round() as usize, Ordering::Relaxed);
}

#[inline]
pub fn viewport_w() -> Option<f32> {
    match VIEWPORT_W.load(Ordering::Relaxed) {
        0 => None,
        w => Some(w as f32),
    }
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
/// stored as a permille integer). Convert legacy values and record the scale
/// version together before reading; missing or unparsable zoom uses 1.0×,
/// while out-of-range values are clamped.
pub fn init_zoom() {
    if let Some(permille) = crate::settings::migrate_ui_scale_base() {
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
/// `"graph_lane_compact"`, `"true"`/`"false"`; missing → **on**, 2026-10-02).
///
/// This flag drives the swimlane *visuals* only — avatar commit nodes, the
/// lane tint band, and the graph lane padding (bin crate `render_helpers.rs`).
/// The lane *layout* is always the gitk-style `graph::layout` (ADR-0122) and does
/// not read this flag. Backed by an atomic (like `graph_compact`) so the
/// Settings-screen toggle can flip it live; the loaded value is logged here so
/// the startup state is debuggable alongside the other settings lines.
static GRAPH_LANE_COMPACT: AtomicBool = AtomicBool::new(true);

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

/// Reduce-motion flag (issue #354 / ADR-0173). When on, kagi's looping
/// decorative animations render static. Defaults to **off**. Read cheaply at
/// render time (this crate's UI has no direct dependency on GPUI's own
/// reduce-motion API — the pinned build's animation element does not honor it).
static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

/// The currently-active reduce-motion flag (read every render frame that draws
/// a looping animation, e.g. the tab-loading dots).
#[inline]
pub fn reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

/// Set + persist the reduce-motion flag to `settings.json` (key `reduce_motion`).
pub fn set_reduce_motion(on: bool) {
    REDUCE_MOTION.store(on, Ordering::Relaxed);
    write_setting("reduce_motion", Some(if on { "true" } else { "false" }));
}

/// Initialise the reduce-motion flag at startup from `settings.json`
/// (`"reduce_motion"`). Missing → off; only an explicit `"true"` enables it.
pub fn init_reduce_motion() {
    REDUCE_MOTION.store(Settings::load().reduce_motion(), Ordering::Relaxed);
    klog!("reduce_motion: {}", reduce_motion());
}

/// Look up a **built-in** theme's index in [`THEMES`] by slug (retired slugs
/// resolve to their successors). User themes: [`theme_by_slug`].
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
        // ADR-0175's two lane-palette candidates folded into Flower Road.
        "flower-road-bloom" | "flower-road-vivid" => "flower-road",
        other => other,
    }
}

/// Test-only guard for the global `ACTIVE` index + `KAGI_LOG_DIR`; held by
/// every test that changes the active theme (incl. `avatar`'s colour test).
#[cfg(test)]
pub(crate) static ACTIVE_THEME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set the active theme (built-in or user) by slug and persist the slug to
/// `settings.json`. Returns `true` if the slug was recognised.
pub fn set_active(slug: &str) -> bool {
    let mut reg = registry_mut();
    let Some(next) = reg.find(slug) else {
        return false;
    };
    reg.activate(next);
    drop(reg);
    save_settings(slug);
    true
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
/// Loads the user themes first (errors are kept for
/// [`take_theme_load_errors`]), then resolves `KAGI_THEME` env override →
/// persisted `settings.json` → default (Catppuccin Mocha) against the full
/// list.  Logs `[kagi] theme: <slug> dark=<bool>`.
pub fn init_active() {
    let errors = reload_custom_themes();
    *STARTUP_ERRORS.lock().unwrap_or_else(|e| e.into_inner()) = errors;

    let mut reg = registry_mut();
    let chosen = std::env::var("KAGI_THEME")
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| reg.find(&s))
        .or_else(|| load_settings_slug().and_then(|s| reg.find(&s)));
    if let Some(handle) = chosen {
        reg.activate(handle);
    }
    drop(reg);
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
/// Alpha the selection tint is painted at. Reach it through
/// [`selection_overlay`] rather than re-applying it at a call site.
const SELECTION_ALPHA: f32 = 0.30;

/// Alpha of the inline-code chip (gpui-component's `accent`).
///
/// 0.16 is the lightest single value the whole set tolerates: one step further
/// (0.13) drops One Dark to 22, under the floor that exists because `selected`
/// once left code spans invisible. Going lighter than this needs a per-theme
/// value, not a smaller constant.
const CODE_CHIP_ALPHA: f32 = 0.16;

#[inline]
fn to_hsla(rgb_u32: u32) -> Hsla {
    Hsla::from(rgb(rgb_u32))
}

/// WCAG relative luminance for an `0xRRGGBB` colour.
fn relative_luminance(c: u32) -> f64 {
    let channel = |value: u32| {
        let value = value as f64 / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel((c >> 16) & 0xff)
        + 0.7152 * channel((c >> 8) & 0xff)
        + 0.0722 * channel(c & 0xff)
}

fn contrast_ratio(a: u32, b: u32) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Label colour for a filled semantic button.
///
/// Most themes originally used their base background as the label, which
/// preserves their intended dark/light appearance. Retain that choice only
/// when it meets WCAG AA, then prefer the theme's text colour; pure black/white
/// is the final fallback and always gives one accessible option.
fn filled_button_foreground(background: u32, theme: &Theme) -> u32 {
    const MIN_CONTRAST: f64 = 4.5;

    if contrast_ratio(background, theme.bg_base) >= MIN_CONTRAST {
        theme.bg_base
    } else if contrast_ratio(background, theme.text_main) >= MIN_CONTRAST {
        theme.text_main
    } else if contrast_ratio(background, 0x000000) >= contrast_ratio(background, 0xffffff) {
        0x000000
    } else {
        0xffffff
    }
}

fn primary_button_foreground(theme: &Theme) -> u32 {
    filled_button_foreground(theme.color_branch, theme)
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
/// The selection wash, ready to paint.
///
/// The diff panes draw their own line selection instead of going through
/// gpui-component's text selection, so they need this colour too. They used to
/// rebuild it inline from `color_branch` at a hardcoded `0.30` — and when the
/// token moved, they silently kept the old colour. One function, one place to
/// change.
pub fn selection_overlay() -> Hsla {
    to_hsla(theme().selection_tint).alpha(SELECTION_ALPHA)
}

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
    gc.colors.selection = to_hsla(k.selection_tint).alpha(SELECTION_ALPHA);

    // ── Primary / accent (Checkbox checked, focus ring, links) ──
    gc.colors.primary = to_hsla(k.color_branch);
    let primary_foreground = primary_button_foreground(&k);
    gc.colors.primary_foreground = to_hsla(primary_foreground);
    gc.colors.primary_hover = to_hsla(k.color_branch);
    gc.colors.primary_active = to_hsla(k.color_branch);
    gc.colors.ring = to_hsla(k.color_branch);
    // `accent` is gpui-component's inline-code background (TextView) and a
    // hover tint in a few popovers — always a background, never a foreground.
    //
    // This value has been wrong in both directions. `selected` — the row
    // highlight, near the background by design — left `code` spans invisible
    // (user report); a muted-foreground tint at 0.48 then made them a dark
    // block, hardest in the light themes (user report), because `text_muted`
    // is a *foreground* colour and half of it is still most of the way to one.
    // The alpha keeps the same hue relationship and lands every theme inside
    // the band `code_chip_is_visible_but_not_a_block` pins.
    gc.colors.accent = to_hsla(k.text_muted).alpha(CODE_CHIP_ALPHA);
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
    // Primary (including the Commit button): kagi's branch accent. Preserve
    // the established label colour when it is legible; otherwise use the
    // theme's accessible primary-button foreground.
    gc.colors.button_primary = to_hsla(k.color_branch);
    gc.colors.button_primary_foreground = to_hsla(primary_foreground);
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
    let warning_foreground = filled_button_foreground(k.color_warning, &k);
    gc.colors.button_warning = to_hsla(k.color_warning);
    gc.colors.button_warning_foreground = to_hsla(warning_foreground);
    gc.colors.button_warning_hover = to_hsla(k.color_warning);
    gc.colors.button_warning_active = to_hsla(k.color_warning);

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
    gc.highlight_theme = highlight_theme(&k);

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
    crate::theme_catppuccin_mocha::CATPPUCCIN_MOCHA,
    crate::theme_apple_dark::APPLE_DARK,
    crate::theme_apple_light::APPLE_LIGHT,
    crate::theme_catppuccin_latte::CATPPUCCIN_LATTE,
    crate::theme_color_vision::COLOR_VISION,
    crate::theme_dracula::DRACULA,
    crate::theme_flower_road::FLOWER_ROAD,
    crate::theme_ibm_pc::IBM_PC,
    crate::theme_monokai::MONOKAI,
    crate::theme_one_dark::ONE_DARK,
    crate::theme_one_light::ONE_LIGHT,
    crate::theme_pinky_boo::PINKY_BOO,
    crate::theme_tokyo_night::TOKYO_NIGHT,
];
// ── Shared swimlane palettes ──────────────────────────────────────────────
/// Lane colour palette for **dark-background** themes.
///
/// `oklch(0.77 0.174 H)` gamut-mapped to sRGB then stored as HSL. Lightness and
/// chroma are fixed so every lane reads at equal brightness/vividness; only hue
/// rotates, ordered so adjacent lane indices are maximally distinct (Gitru
/// swimlane philosophy, see ADR-0104).
pub(crate) const LANE_PALETTE_DARK: [(f32, f32, f32); 8] = [
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
pub(crate) const LANE_PALETTE_LIGHT: [(f32, f32, f32); 8] = [
    (0.935, 0.542, 0.519), // rose   #c74276
    (0.250, 1.0, 0.281),   // green  #478f00
    (0.634, 0.692, 0.603), // blue   #546fe0
    (0.064, 1.0, 0.394),   // orange #c94e00
    (0.471, 1.0, 0.300),   // teal   #00997e
    (0.785, 0.460, 0.539), // purple #a053bf
    (0.117, 1.0, 0.318),   // gold   #a27100
    (0.550, 1.0, 0.389),   // blue2  #008bc6
];

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The diff panes paint their own line selection; this is the one colour
    /// they share with gpui-component's text selection. It used to be rebuilt
    /// inline from `color_branch`, so moving `selection_tint` changed text
    /// selection and left the diff panes on the old colour — the whole bug.
    #[test]
    fn selection_overlay_follows_the_active_theme_token() {
        let _guard = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: single-threaded under the lock above.
        unsafe { std::env::set_var("KAGI_LOG_DIR", dir.path()) };
        let restore = theme();
        for t in THEMES {
            assert!(set_active(&t.slug), "{}", t.slug);
            let got = selection_overlay();
            assert_eq!(
                got,
                to_hsla(t.selection_tint).alpha(SELECTION_ALPHA),
                "{}: selection_overlay must be selection_tint, not another token",
                t.slug
            );
        }
        registry_mut().activate(restore);
        unsafe { std::env::remove_var("KAGI_LOG_DIR") };
    }

    /// The inline-code chip has to be visible without being a block.
    ///
    /// It has been wrong in both directions: first `selected`, which sits near
    /// the background by design and left `code` spans invisible; then a
    /// foreground colour at 0.48, which made them a dark slab. The band is
    /// what neither end satisfied.
    #[test]
    fn code_chip_is_visible_but_not_a_block() {
        for t in THEMES {
            let chip = [16u32, 8, 0].map(|sh| {
                let (f, b) = (
                    ((t.text_muted >> sh) & 0xff) as f32,
                    ((t.bg_base >> sh) & 0xff) as f32,
                );
                (f * CODE_CHIP_ALPHA + b * (1.0 - CODE_CHIP_ALPHA)) as i32
            });
            let base = [16u32, 8, 0].map(|sh| ((t.bg_base >> sh) & 0xff) as i32);
            let delta: i32 = chip
                .iter()
                .zip(base.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            assert!(
                delta >= 24,
                "{}: the code chip is only {} from the background — invisible",
                t.slug,
                delta
            );
            assert!(
                delta <= 100,
                "{}: the code chip is {} from the background — reads as a block",
                t.slug,
                delta
            );
        }
    }

    /// Selected text must not look like a diff line.
    ///
    /// The regression: Flower Road's accent is the same pink as its
    /// `diff_removed_bg`, and the selection tint was derived from that accent —
    /// so selecting a context line in a diff painted it 12 (channel sum) from
    /// the colour of a removed line. Indistinguishable, and actively
    /// misleading in the one view where "was this line removed?" is the
    /// question being asked.
    #[test]
    fn selection_tint_is_not_mistakable_for_a_diff_line() {
        /// Composite `fg` at the selection alpha over `bg`.
        fn tinted(fg: u32, bg: u32) -> [i32; 3] {
            [16u32, 8, 0].map(|sh| {
                let (f, b) = (((fg >> sh) & 0xff) as f32, ((bg >> sh) & 0xff) as f32);
                (f * SELECTION_ALPHA + b * (1.0 - SELECTION_ALPHA)) as i32
            })
        }
        fn channels(c: u32) -> [i32; 3] {
            [16u32, 8, 0].map(|sh| ((c >> sh) & 0xff) as i32)
        }
        fn delta(a: [i32; 3], b: [i32; 3]) -> i32 {
            a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
        }

        for t in THEMES {
            // A selected *context* line: the tint over the plain background.
            let selected_context = tinted(t.selection_tint, t.bg_base);
            for (name, wash) in [
                ("diff_added_bg", t.diff_added_bg),
                ("diff_removed_bg", t.diff_removed_bg),
            ] {
                assert!(
                    delta(selected_context, channels(wash)) >= 24,
                    "{}: a selected context line is only {} from {} — it reads as that kind of line",
                    t.slug,
                    delta(selected_context, channels(wash)),
                    name
                );
                // …and the selection has to remain visible when the line it
                // covers already has a diff wash under it.
                assert!(
                    delta(tinted(t.selection_tint, wash), channels(wash)) >= 24,
                    "{}: selection is invisible on a {} line",
                    t.slug,
                    name
                );
            }
        }
    }

    #[test]
    fn themes_have_unique_slugs() {
        let mut slugs: Vec<&str> = THEMES.iter().map(|t| &*t.slug).collect();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), THEMES.len(), "theme slugs must be unique");
    }

    #[test]
    fn index_of_resolves_all_slugs() {
        for (i, t) in THEMES.iter().enumerate() {
            assert_eq!(index_of(&t.slug), Some(i));
        }
        assert_eq!(index_of("does-not-exist"), None);
    }

    /// `set_active` must store the *selected* theme's index — the picker reads
    /// it back via `theme()`, and a `set_active` that always
    /// stored 0 passed the whole theme suite. Persists, hence the tempdir.
    #[test]
    fn set_active_selects_the_named_theme() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let prev = theme();
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        // Every built-in theme, not just index 0.
        for (i, t) in THEMES.iter().enumerate() {
            assert!(set_active(&t.slug), "{} must be recognised", t.slug);
            assert_eq!(
                theme().key(),
                ThemeHandle::builtin(i).key(),
                "theme() after set_active({})",
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

        registry_mut().activate(prev);
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

    #[test]
    fn zoom_steps_follow_presets_and_clamp() {
        assert_eq!(step_zoom(1.111, true), 1.2);
        assert_eq!(step_zoom(1.111, false), 1.1);
        assert_eq!(step_zoom(1.0, true), 1.1);
        assert_eq!(step_zoom(1.0, false), 0.9);
        assert_eq!(step_zoom(ZOOM_MIN, false), ZOOM_MIN);
        assert_eq!(step_zoom(ZOOM_MAX, true), ZOOM_MAX);
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
                (t.selection_tint >> 16) & 0xff,
                (t.selection_tint >> 8) & 0xff,
                t.selection_tint & 0xff,
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
            if LOW_CONTRAST_BY_UPSTREAM_DESIGN.contains(&&*t.slug) {
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
                let c = contrast_ratio(colour, t.bg_base);
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

    #[test]
    fn primary_button_labels_meet_wcag_aa() {
        for t in THEMES {
            let foreground = primary_button_foreground(t);
            let contrast = contrast_ratio(t.color_branch, foreground);
            assert!(
                contrast >= 4.5,
                "{}: primary button label {foreground:#08x} is only {contrast:.2}:1 on {:#08x}",
                t.slug,
                t.color_branch,
            );
        }
    }

    #[test]
    fn warning_button_labels_meet_wcag_aa() {
        for t in THEMES {
            let foreground = filled_button_foreground(t.color_warning, t);
            let contrast = contrast_ratio(t.color_warning, foreground);
            assert!(
                contrast >= 4.5,
                "{}: warning button label {foreground:#08x} is only {contrast:.2}:1 on {:#08x}",
                t.slug,
                t.color_warning,
            );
        }
    }

    /// Retired Xcode slugs must resolve to their Apple successors, so an
    /// existing settings.json doesn't silently drop to the default theme.
    #[test]
    fn legacy_xcode_slugs_alias_to_apple() {
        assert_eq!(index_of("xcode-dark"), index_of("apple-dark"));
        // ADR-0175's candidates were folded into Flower Road (2026-10-02).
        assert_eq!(index_of("flower-road-bloom"), index_of("flower-road"));
        assert_eq!(index_of("flower-road-vivid"), index_of("flower-road"));
        assert_eq!(index_of("xcode-light"), index_of("apple-light"));
        assert!(index_of("apple-dark").is_some());
        assert_eq!(index_of("still-not-a-theme"), None);
    }

    #[test]
    fn dark_and_light_counts() {
        let dark = THEMES.iter().filter(|t| t.dark).count();
        let light = THEMES.iter().filter(|t| !t.dark).count();
        // catppuccin, one-dark, monokai, tokyo-night, ibm-pc, dracula, apple-dark,
        // color-vision
        assert_eq!(dark, 8);
        // one-light, pinky-boo, catppuccin-latte, apple-light, flower-road
        assert_eq!(light, 5);
    }
}

#[cfg(test)]
mod accent_text_tests {
    use super::{contrast_ratio, THEMES};

    /// The selected Graph / PRs / Issues tab draws `accent_text_on(surface)` on
    /// `surface`: it must be readable in every theme (4.5:1), and keep the
    /// accent colour wherever the accent itself is readable.
    #[test]
    fn the_selected_sidebar_tab_is_readable_in_every_theme() {
        for t in THEMES {
            let fg = t.accent_text_on(t.surface);
            assert!(
                contrast_ratio(fg, t.surface) >= 4.5,
                "{}: {:06x} on {:06x} is {:.2}:1",
                t.slug,
                fg,
                t.surface,
                contrast_ratio(fg, t.surface)
            );
            if contrast_ratio(t.color_branch, t.surface) >= 4.5 {
                assert_eq!(fg, t.color_branch, "{} keeps its accent", t.slug);
            } else {
                assert_ne!(
                    fg, t.text_main,
                    "{} deepens its accent, not the body text",
                    t.slug
                );
            }
        }
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn custom(slug: &str, name: &str, base: &Theme) -> Theme {
        Theme {
            slug: Cow::Owned(slug.to_owned()),
            name: Cow::Owned(name.to_owned()),
            ..base.clone()
        }
    }

    /// Puts the registry back to "no user themes, default active" even when
    /// an assertion fails, so other tests never see this test's themes.
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            install_custom_themes(Vec::new());
            registry_mut().activate(ThemeHandle::builtin(0));
        }
    }

    fn weak(h: &ThemeHandle) -> std::sync::Weak<Theme> {
        match &h.0 {
            HandleInner::Custom(_, t) => Arc::downgrade(t),
            HandleInner::Builtin(_) => panic!("{} is built in", h.slug),
        }
    }

    #[test]
    fn every_theme_once_in_name_order() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = Reset;
        install_custom_themes(vec![
            custom("zz", "aardvark", &THEMES[1]),
            custom("mm", "Midnight", &THEMES[2]),
        ]);

        let sorted = themes_by_name();
        assert_eq!(sorted.len(), THEMES.len() + 2);
        let names: Vec<String> = sorted.iter().map(|t| t.name.to_lowercase()).collect();
        let mut expected = names.clone();
        expected.sort();
        assert_eq!(names, expected);
        assert_eq!(&*sorted[0].slug, "zz", "user themes sort among built-ins");

        let all = all_themes();
        assert_eq!(all[0].slug, "catppuccin", "the default stays first");
        let tail: Vec<&str> = all[THEMES.len()..].iter().map(|t| &*t.slug).collect();
        assert_eq!(
            tail,
            ["zz", "mm"],
            "user themes follow THEMES in load order"
        );
    }

    #[test]
    fn user_themes_are_selectable_and_persisted_by_slug() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = Reset;
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());
        install_custom_themes(vec![custom("mine", "Mine", &THEMES[5])]);

        assert!(set_active("mine"));
        let t = theme();
        assert!(t.is_custom());
        assert_eq!((&*t.slug, t.bg_base), ("mine", THEMES[5].bg_base));
        assert_eq!(load_settings_slug().as_deref(), Some("mine"));
        assert_eq!(theme_by_slug("mine").map(|h| h.key()), Some(t.key()));
        assert_eq!(
            theme_by_slug("xcode-dark").map(|h| h.key()),
            index_of("apple-dark").map(|i| ThemeHandle::builtin(i).key())
        );
        assert!(theme_by_slug("nope").is_none());
        std::env::remove_var("KAGI_LOG_DIR");
    }

    /// A reload keeps the active slug as the new snapshot, keeps old handles
    /// usable, frees a generation once nothing holds it, and falls back to
    /// the default — without persisting it — when the active theme is gone.
    #[test]
    fn reload_swaps_generations_without_leaking() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = Reset;
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());
        let themes = tmp.path().join("themes");
        std::fs::create_dir(&themes).unwrap();
        let write = |slug: &str, bg: &str| {
            std::fs::write(
                themes.join(format!("{slug}.json")),
                format!(
                    r#"{{"slug":"{slug}","name":"{slug}","extends":"one-dark","bg_base":"{bg}"}}"#
                ),
            )
            .unwrap()
        };
        write("mine", "#111111");
        assert!(reload_custom_themes().is_empty());
        assert!(set_active("mine"));

        let first = theme();
        let first_gen = weak(&first);
        write("mine", "#222222");
        std::fs::write(themes.join("bad.json"), r#"{"slug":"bad"}"#).unwrap();
        let errors = reload_custom_themes();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].path, themes.join("bad.json"));

        let second = theme();
        assert_eq!((&*second.slug, second.bg_base), ("mine", 0x222222));
        assert_ne!(second.key(), first.key(), "a reload is a new instance");
        assert_eq!(first.bg_base, 0x111111, "an old handle stays readable");
        drop(first);
        assert!(first_gen.upgrade().is_none(), "the old generation is freed");

        let second_gen = weak(&second);
        drop(second);
        std::fs::remove_file(themes.join("mine.json")).unwrap();
        reload_custom_themes();
        assert_eq!(theme().key(), ThemeHandle::builtin(0).key());
        assert!(second_gen.upgrade().is_none(), "the removed theme is freed");
        assert_eq!(
            load_settings_slug().as_deref(),
            Some("mine"),
            "the fallback is not persisted"
        );

        // A built-in selection is untouched by reloads.
        assert!(set_active("dracula"));
        write("mine", "#333333");
        reload_custom_themes();
        assert_eq!(theme().slug, "dracula");
        std::env::remove_var("KAGI_LOG_DIR");
    }

    /// Startup loads user themes before resolving the persisted slug, and
    /// keeps the load errors for the UI.
    #[test]
    fn init_active_restores_a_persisted_user_theme() {
        let _g = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _reset = Reset;
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());
        std::env::remove_var("KAGI_THEME");
        let themes = tmp.path().join("themes");
        std::fs::create_dir(&themes).unwrap();
        std::fs::write(
            themes.join("mine.json"),
            r#"{"slug":"mine","name":"Mine","extends":"tokyo-night"}"#,
        )
        .unwrap();
        std::fs::write(themes.join("broken.json"), "{").unwrap();
        save_settings("mine");

        init_active();
        assert_eq!(theme().slug, "mine");
        let errors = take_theme_load_errors();
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].to_string().starts_with("broken.json: "),
            "{errors:?}"
        );
        assert!(take_theme_load_errors().is_empty(), "errors are taken once");
        std::env::remove_var("KAGI_LOG_DIR");
    }
}
