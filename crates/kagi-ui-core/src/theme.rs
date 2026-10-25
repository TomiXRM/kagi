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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

use gpui::{hsla, rgb, App, Hsla};
use serde::Serialize;

use crate::settings::{read_setting, write_setting, Settings};

#[cfg(any(test, feature = "gui-e2e"))]
pub use crate::system_accent::AccentInjection;
pub use crate::system_accent::{
    detected_system_accent, follows_system_accent, refresh_system_accent,
    set_follows_system_accent, SystemAccent,
};
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
    /// Unemphasized selection when the window is not key.
    pub inactive_selected: u32,
    /// Detail panel / tab strip background (mantle).
    pub panel: u32,
    /// Sidebar background (crust).
    pub sidebar: u32,
    /// Floating surface; Apple Dark uses this for every elevated card.
    pub modal: u32,
    /// Full-screen modal scrim (alpha applied at the call site).
    pub modal_overlay: u32,

    // ── Text ─────────────────────────────────────────────────────
    pub text_main: u32,
    pub text_sub: u32,
    pub text_muted: u32,
    /// Field labels in the detail panel.
    pub text_label: u32,
    /// Body-link foreground, independent of filled ref/Primary accents.
    pub link: u32,

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
    /// Apple Dark and its restrained derivative share the same neutral chrome.
    #[inline]
    pub fn uses_neutral_dark_chrome(&self) -> bool {
        matches!(&*self.slug, "apple-dark" | "nagi-dark")
    }

    /// Neutral dark themes use a single floating fill.
    #[inline]
    pub fn elevated_surface(&self, existing: u32) -> u32 {
        if self.uses_neutral_dark_chrome() {
            self.modal
        } else {
            existing
        }
    }

    /// Neutral identifying boundary for the floating layer.
    #[inline]
    pub fn elevated_border(&self, existing: u32) -> u32 {
        if self.uses_neutral_dark_chrome() {
            crate::theme_apple_dark::ELEVATED_BORDER
        } else {
            existing
        }
    }

    /// Window chrome stays lighter than panels but darker than elevated cards.
    #[inline]
    pub fn chrome_surface(&self) -> u32 {
        if self.uses_neutral_dark_chrome() {
            crate::theme_apple_dark::CHROME
        } else {
            self.panel
        }
    }

    /// Chrome accent. Git ref/lane/status tokens and custom themes stay fixed.
    #[inline]
    pub fn ui_accent(&self) -> u32 {
        #[cfg(target_os = "macos")]
        if matches!(&*self.slug, "apple-light" | "apple-dark")
            && crate::system_accent::follows_system_accent_for(self.dark)
        {
            return detected_system_accent().rgb(self.dark);
        }
        self.color_branch
    }

    #[inline]
    pub fn ui_selection_tint(&self) -> u32 {
        #[cfg(target_os = "macos")]
        if matches!(&*self.slug, "apple-light" | "apple-dark")
            && crate::system_accent::follows_system_accent_for(self.dark)
        {
            return detected_system_accent().rgb(self.dark);
        }
        self.selection_tint
    }

    /// Apple key selection paints the opaque chrome accent, like Finder.
    #[inline]
    pub fn ui_selected(&self) -> u32 {
        if matches!(&*self.slug, "apple-light" | "apple-dark") {
            return self.ui_accent();
        }
        self.selected
    }

    /// Selection presentation never changes the selected identity.
    pub fn selected_for_window(&self, active: bool) -> Hsla {
        if active {
            to_hsla(self.ui_selected())
        } else {
            to_hsla(self.inactive_selected)
        }
    }

    /// Graph-only selection: accent over normal stripes/lane tints, normal ink.
    /// Other presets and non-key windows retain their existing selection fill.
    pub fn selection_wash(&self, active: bool) -> Hsla {
        if active && matches!(&*self.slug, "apple-light" | "apple-dark") {
            to_hsla(self.ui_accent()).alpha(0.30)
        } else {
            self.selected_for_window(active)
        }
    }

    /// Segmented controls, modes and tabs retain a subtle fill, not row accent.
    #[inline]
    pub fn segment_selected_bg(&self) -> Hsla {
        to_hsla(if matches!(&*self.slug, "apple-light" | "apple-dark") {
            self.inactive_selected
        } else {
            self.selected
        })
    }

    /// Opaque active Apple and Nagi Dark selections replace ordinary/semantic labels.
    #[inline]
    pub fn selection_uses_on_accent(&self, selected: bool, window_active: bool) -> bool {
        selected
            && window_active
            && matches!(&*self.slug, "apple-light" | "apple-dark" | "nagi-dark")
    }

    /// Primary selected label follows the actual selection fill, not the UI accent.
    pub fn selection_foreground(&self, active: bool) -> Hsla {
        if self.selection_uses_on_accent(true, active) {
            to_hsla(on_accent_foreground(self.ui_selected()))
        } else {
            to_hsla(self.text_main)
        }
    }

    /// Nagi Dark needs solid selected secondary ink; Apple retains its 70% ink.
    pub fn selection_secondary_foreground(&self, active: bool) -> Hsla {
        if self.selection_uses_on_accent(true, active) {
            self.selection_foreground(true)
                .alpha(if self.slug == "nagi-dark" { 1.0 } else { 0.7 })
        } else {
            to_hsla(self.text_sub)
        }
    }

    /// Foreground on the opaque chrome accent; other presets keep their identity.
    pub fn on_accent(&self) -> u32 {
        if matches!(&*self.slug, "apple-light" | "apple-dark") {
            on_accent_foreground(self.ui_accent())
        } else {
            filled_button_foreground(self.ui_accent(), self)
        }
    }

    /// Name for menus and Settings. Brand names stay as they are; the
    /// descriptive accessibility theme is localized (#354).
    pub fn display_name(&self) -> &str {
        match &*self.slug {
            "color-vision" => crate::i18n::Msg::ThemeColorVision.t(),
            _ => &self.name,
        }
    }

    /// The chrome accent as text on `background`, kept readable
    /// (WCAG AA, 4.5:1): unchanged when it already is, otherwise the same hue
    /// and saturation with its lightness stepped down (light background) or up
    /// (dark one) until it reads. Flower Road's pink accent on its beige
    /// `surface` was 1.6:1 on the selected sidebar tab; it stays pink, deeper.
    pub fn accent_text_on(&self, background: u32) -> u32 {
        self.readable_text_on(self.ui_accent(), &[background], 4.5)
    }

    /// Destructive text keeps its hue across every composited button state.
    /// A small rounding margin preserves 4.5:1 after HSLA and native painting.
    pub fn destructive_text_on(&self, backgrounds: &[u32]) -> u32 {
        self.readable_text_on(self.color_blocker, backgrounds, 4.6)
    }

    /// Hue-preserving caption and outline ink against the row and its hover fill.
    /// Cache both semantic roles: long file lists reuse these exact backgrounds.
    pub fn status_label_text_on(&self, color: u32, background: u32, hover: u32) -> u32 {
        #[derive(Clone, Copy)]
        struct CachedInk {
            key: [u32; 4],
            ink: u32,
        }
        thread_local! {
            static LAST: std::cell::Cell<[CachedInk; 2]> =
                const { std::cell::Cell::new([CachedInk { key: [u32::MAX, 0, 0, 0], ink: 0 }; 2]) };
        }
        let key = [color, background, hover, self.text_main];
        LAST.with(|cache| {
            let entries = cache.get();
            if let Some(entry) = entries.iter().find(|entry| entry.key == key) {
                return entry.ink;
            }
            let ink = self.readable_text_on(color, &[background, hover], 4.6);
            // Near middle luminance, even black/white can fall just below the
            // rounding margin. Keep the AA requirement rather than that margin.
            let ink = if contrast_ratio(ink, background) >= 4.5 && contrast_ratio(ink, hover) >= 4.5
            {
                ink
            } else {
                self.readable_text_on(color, &[background, hover], 4.5)
            };
            cache.set([CachedInk { key, ink }, entries[0]]);
            ink
        })
    }

    /// Caption ink for an opaque destructive button across its actual fills.
    /// Compute when syncing the palette, never in the button render path.
    pub fn destructive_fill_text_on(&self, backgrounds: &[u32]) -> u32 {
        self.readable_text_on(
            filled_button_foreground(self.color_blocker, self),
            backgrounds,
            4.5,
        )
    }

    fn readable_text_on(&self, color: u32, backgrounds: &[u32], minimum: f64) -> u32 {
        let readable = |color| {
            backgrounds
                .iter()
                .all(|&bg| contrast_ratio(color, bg) >= minimum)
        };
        if readable(color) {
            return color;
        }
        let base = to_hsla(color);
        // Mid-luminance fills can need dark ink even in a dark theme. Choose
        // the endpoint with the best worst-state contrast, not theme mode.
        let endpoint_contrast = |ink| {
            backgrounds
                .iter()
                .map(|&bg| contrast_ratio(ink, bg))
                .fold(f64::INFINITY, f64::min)
        };
        let step = if endpoint_contrast(0x000000) >= endpoint_contrast(0xffffff) {
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
            .find(|&c| readable(c))
            .unwrap_or(self.text_main)
    }

    /// HSLA colour for graph lane `i`; Nagi cycles three chromatic families.
    pub fn lane_color(&self, i: usize) -> Hsla {
        let count = if matches!(&*self.slug, "nagi-light" | "nagi-dark") {
            3
        } else {
            self.lane_hsl.len()
        };
        let (h, s, l) = self.lane_hsl[i % count];
        hsla(h, s, l, 1.0)
    }

    /// Graph-column band opacity; labels remain on the separate neutral row.
    pub fn lane_band_alpha(&self, hovered: bool) -> f32 {
        match (&*self.slug, self.dark, hovered) {
            ("nagi-dark", _, false) => 0.40,
            ("nagi-dark", _, true) => 0.45,
            (_, true, false) => 0.18,
            (_, true, true) => 0.24,
            (_, false, false) => 0.11,
            (_, false, true) => 0.15,
        }
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
/// Low-alpha ref fill with a stronger border. Dark themes use white text;
/// light themes use their main near-black ink, preserving ADR-0126's treatment.
/// Non-graph semantic chips may retain their own foreground instead.
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

/// Graph badges need an opaque Nagi Dark fill to prevent selected-row tint
/// stacking. Generic semantic chips retain [`badge_style`]'s subtle fill.
#[inline]
pub fn graph_badge_style(color: u32) -> (u32, u32, u32) {
    let t = theme();
    if t.slug == "nagi-dark" {
        // An opaque base-anchored chip cannot stack its tint over selection.
        let fill = to_hsla(t.bg_base).blend(to_hsla(color).alpha(0.40));
        let fill = u32::from(fill.to_rgb());
        return (fill, (color << 8) | 0xff, 0xffffff);
    }
    badge_style(color)
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

/// Global text/layout zoom supports 70%–200%, including migrated legacy values.
pub const ZOOM_MIN: f32 = 0.7;
pub const ZOOM_MAX: f32 = 2.0;

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

/// Finder-inspired file-tree typography, before shared density and UI zoom.
/// Use `scaled_px(FILE_TREE_FONT_PX)` with `text_size` on dense file/ref labels.
pub const FILE_TREE_FONT_PX: f32 = 13.;

/// Dense toolbar count role, before the shared 0.9 density and UI zoom.
/// 13px resolves to 11.7 logical points at 100%, above the 11pt minimum.
pub const TOOLBAR_COUNT_TEXT: f32 = 13.0;
pub const TOOLBAR_COUNT_LINE_HEIGHT: f32 = 16.0;
pub const TOOLBAR_COUNT_HEIGHT: f32 = 18.0;
/// Keep the enlarged pill to the icon's right quadrant; wider counts grow outwards.
pub const TOOLBAR_COUNT_ICON_OVERLAP: f32 = 8.0;

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

// Compact layout shares zoom's startup/persisted-preference ownership, but its
// render-time read must not snapshot the settings document or touch the disk.
static COMPACT_THRESHOLD_REM: AtomicU32 =
    AtomicU32::new(kagi_domain::compact::DEFAULT_THRESHOLD_REM);

/// Active compact-layout switch width in whole root rem. Allocation-/I/O-free.
#[inline]
pub fn compact_threshold_rem() -> u32 {
    COMPACT_THRESHOLD_REM.load(Ordering::Relaxed)
}

/// Set and persist the compact switch width, clamped to the supported range.
/// Returns the usable whole-rem value that is now active.
pub fn set_compact_threshold_rem(threshold: u32) -> u32 {
    let threshold = threshold.clamp(
        kagi_domain::compact::MIN_THRESHOLD_REM,
        kagi_domain::compact::MAX_THRESHOLD_REM,
    );
    COMPACT_THRESHOLD_REM.store(threshold, Ordering::Relaxed);
    write_setting("compact_threshold_rem", Some(&threshold.to_string()));
    threshold
}

/// Restore the compact switch width once at startup. Invalid or missing saved
/// values use the default without rewriting the user's settings document.
pub fn init_compact_threshold_rem() {
    COMPACT_THRESHOLD_REM.store(Settings::load().compact_threshold_rem(), Ordering::Relaxed);
    klog!("compact_threshold_rem: {}", compact_threshold_rem());
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

/// Effective Reduce Motion: neither app-off nor system-off overrides an opt-in.
#[inline]
pub fn reduce_motion() -> bool {
    crate::accessibility::system_display().0 || app_reduce_motion()
}

/// The app-only opt-in displayed and edited by the Appearance switch.
pub fn app_reduce_motion() -> bool {
    REDUCE_MOTION.load(Ordering::Relaxed)
}

pub fn reduce_transparency() -> bool {
    crate::accessibility::system_display().1
}

/// Fully opaque equivalent of the dashboard's 35%-black overlay on the page.
pub fn pr_card_opaque_bg() -> u32 {
    let base = theme().bg_base;
    let channel = |shift: u32| (((base >> shift) & 255) * 65 + 50) / 100;
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
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
    crate::system_accent::init();
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
    let z = scaled(1.0);
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

/// WCAG contrast ratio of two opaque `0xRRGGBB` colours.
pub fn contrast_ratio(a: u32, b: u32) -> f64 {
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

/// Finder's white accent label, falling back to black below 3:1 (owner #1468).
/// Cache the resolved RGB pair: selected rows must not repeat luminance work.
pub fn on_accent_foreground(background: u32) -> u32 {
    thread_local! {
        static LAST: std::cell::Cell<(u32, u32)> = const {
            std::cell::Cell::new((u32::MAX, 0))
        };
    }
    LAST.with(|last| {
        let (cached_background, foreground) = last.get();
        if cached_background == background {
            return foreground;
        }
        let foreground = if contrast_ratio(background, 0xffffff) >= 3.0 {
            0xffffff
        } else {
            0x000000
        };
        last.set((background, foreground));
        foreground
    })
}

/// Readable label on the chrome accent, shared by primary buttons/count chips.
pub fn primary_button_foreground(theme: &Theme) -> u32 {
    theme.on_accent()
}

fn filled_button_interaction_colors(fill: u32, foreground: u32) -> (u32, Hsla, Hsla) {
    let background = to_hsla(fill);
    // The chosen label, not theme mode, determines which direction preserves
    // contrast: a dark theme can have a dark custom accent with a light label.
    let light_label = relative_luminance(foreground) > relative_luminance(fill);
    let away = if light_label { 0x000000 } else { 0xffffff };
    let toward = away ^ 0xffffff;
    let channels = background.to_rgb();
    let channel_sum = (channels.r + channels.g + channels.b) * 255.0;
    let away_headroom = if light_label {
        channel_sum
    } else {
        765.0 - channel_sum
    };
    // Measure actual sRGB blend headroom, not HSL lightness: saturated navy
    // can have middling HSL lightness but almost no distance toward black.
    // Reserve at least 25 channel steps per transition (one step above the
    // regression floor). Near an endpoint, move inward instead.
    let (target, headroom, preferred_strength) = if away_headroom < 160.0 {
        (toward, 765.0 - away_headroom, 0.16_f32)
    } else {
        (away, away_headroom, 0.32_f32)
    };
    let strength = preferred_strength.max(50.0 / headroom);
    let target = to_hsla(target);
    // Blend into an opaque fill, rather than fading the button onto an
    // arbitrary Commit-panel/modal surface.
    let hover = background.blend(target.alpha(strength / 2.0));
    let pressed = background.blend(target.alpha(strength));
    // A custom label can meet AA at rest but fail on the inward shade.
    // Keep one label for every state, preserving the existing choice wherever
    // possible; only filled Button needs this stronger accessibility fallback.
    let foreground = if contrast_ratio(u32::from(pressed.to_rgb()) >> 8, foreground) >= 4.5 {
        foreground
    } else if contrast_ratio(fill, 0x000000) >= contrast_ratio(fill, 0xffffff) {
        0x000000
    } else {
        0xffffff
    };
    (foreground, hover, pressed)
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
    to_hsla(theme().ui_selection_tint()).alpha(SELECTION_ALPHA)
}

pub fn sync_gpui_component_theme(cx: &mut App) {
    let k = theme();
    let gc = gpui_component::Theme::global_mut(cx);
    apply_gpui_component_theme(&k, gc);
}

/// Resolve the pinned component library's shared list role for this window's frame.
pub fn sync_window_selection_theme(active: bool, cx: &mut App) {
    gpui_component::Theme::global_mut(cx).colors.list_active = theme().selected_for_window(active);
}

fn apply_gpui_component_theme(k: &Theme, gc: &mut gpui_component::Theme) {
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
    let border = if k.uses_neutral_dark_chrome() {
        crate::theme_apple_dark::INPUT_BORDER
    } else {
        k.text_muted
    };
    gc.colors.border = to_hsla(border).alpha(0.85);
    gc.colors.muted = to_hsla(k.surface);
    gc.colors.muted_foreground = to_hsla(k.text_muted);

    // ── Popover / overlay / selection (Tooltip, modals, Input) ──
    gc.colors.popover = to_hsla(k.elevated_surface(k.modal));
    gc.colors.popover_foreground = to_hsla(k.text_main);
    gc.colors.overlay = to_hsla(k.modal_overlay);
    // Input text selection is separate from opaque row selection: it retains
    // normal text. gpui-component paints underneath that text and caps its own
    // themes at alpha 0.3, so the accent tint is visible without hiding glyphs.
    gc.colors.selection = to_hsla(k.ui_selection_tint()).alpha(SELECTION_ALPHA);

    // ── Primary / accent (Checkbox checked, focus ring) ────────
    gc.colors.primary = to_hsla(k.ui_accent());
    let primary_foreground = primary_button_foreground(k);
    // Indicators follow Finder's accent rule (3:1 UI components), like primary buttons.
    gc.colors.primary_foreground = to_hsla(on_accent_foreground(k.ui_accent()));
    // Keep these generic tokens unchanged: outline Primary uses their tint;
    // filled Primary has its own interaction tokens below.
    gc.colors.primary_hover = to_hsla(k.ui_accent());
    gc.colors.primary_active = to_hsla(k.ui_accent());
    gc.colors.ring = to_hsla(k.ui_accent());
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
    gc.colors.link = to_hsla(k.link);

    // ── Secondary / title-bar controls (gpui-component TitleBar) ──
    gc.colors.secondary = to_hsla(k.surface);
    gc.colors.secondary_foreground = to_hsla(k.text_main);
    gc.colors.secondary_hover = to_hsla(k.selected);
    gc.colors.secondary_active = to_hsla(k.surface);

    // ── Input border (Input, Checkbox unchecked) ────────────────
    // Apple presets separate identifying boundaries from readable labels.
    // Custom and other palettes retain their existing boundary/focus contrast.
    gc.colors.input = to_hsla(match &*k.slug {
        "apple-light" | "nagi-light" => crate::theme_apple_light::INPUT_BORDER,
        "apple-dark" | "nagi-dark" => crate::theme_apple_dark::INPUT_BORDER,
        _ => k.text_muted,
    });
    gc.colors.caret = to_hsla(k.text_main);

    // ── Buttons (0.5.2 reads `button_*`, not primary/secondary) ─
    // Default/neutral (Cherry-pick, Tree/Path, hash chip): kagi surface.
    gc.colors.button = to_hsla(k.surface);
    gc.colors.button_foreground = to_hsla(k.text_main);
    gc.colors.button_hover = to_hsla(k.selected);
    gc.colors.button_active = to_hsla(k.surface);
    // Primary (including the Commit button): preserve the chrome accent at
    // rest and the established label when it is legible across every state.
    let (button_primary_foreground, primary_hover, primary_active) =
        filled_button_interaction_colors(k.ui_accent(), primary_foreground);
    gc.colors.button_primary = to_hsla(k.ui_accent());
    gc.colors.button_primary_foreground = to_hsla(button_primary_foreground);
    gc.colors.button_primary_hover = primary_hover;
    gc.colors.button_primary_active = primary_active;
    // Incoming side choices must follow the same remote token as their pane.
    let info_foreground = filled_button_foreground(k.color_remote, k);
    let (info_foreground, info_hover, info_active) =
        filled_button_interaction_colors(k.color_remote, info_foreground);
    gc.colors.button_info = to_hsla(k.color_remote);
    gc.colors.button_info_foreground = to_hsla(info_foreground);
    gc.colors.button_info_hover = info_hover;
    gc.colors.button_info_active = info_active;
    gc.colors.button_secondary = to_hsla(k.surface);
    gc.colors.button_secondary_foreground = to_hsla(k.text_main);
    gc.colors.button_secondary_hover = to_hsla(k.selected);
    gc.colors.button_secondary_active = to_hsla(k.surface);
    let danger_foreground = filled_button_foreground(k.color_blocker, k);
    let (_, danger_hover, danger_active) =
        filled_button_interaction_colors(k.color_blocker, danger_foreground);
    let danger_foreground = k.destructive_fill_text_on(&[
        k.color_blocker,
        u32::from(danger_hover.to_rgb()) >> 8,
        u32::from(danger_active.to_rgb()) >> 8,
    ]);
    gc.colors.button_danger = to_hsla(k.color_blocker);
    gc.colors.button_danger_foreground = to_hsla(danger_foreground);
    gc.colors.button_danger_hover = danger_hover;
    gc.colors.button_danger_active = danger_active;
    let warning_foreground = filled_button_foreground(k.color_warning, k);
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
    gc.colors.list_active = k.selected_for_window(true);
    gc.colors.list_hover = to_hsla(k.surface);
    gc.colors.sidebar = to_hsla(k.sidebar);
    gc.colors.sidebar_foreground = to_hsla(k.text_main);
    gc.colors.title_bar = to_hsla(k.chrome_surface());
    gc.colors.title_bar_border = to_hsla(k.surface);

    // ── Scrollbar (W12-GCADOPT §2.10) ───────────────────────────
    gc.colors.scrollbar = to_hsla(k.bg_base);
    // Preserve hover/drag feedback even when secondary text roles coincide.
    gc.colors.scrollbar_thumb = to_hsla(k.text_muted).alpha(if k.uses_neutral_dark_chrome() {
        0.8
    } else if k.text_muted == k.text_sub {
        0.65
    } else {
        1.0
    });
    gc.colors.scrollbar_thumb_hover = to_hsla(k.text_sub);

    // ── Drag handle (resizable dividers, future adoption) ───────
    gc.colors.drag_border = to_hsla(k.ui_accent());

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
    if matches!(&*k.slug, "nagi-light" | "nagi-dark") {
        crate::theme_nagi::complete_component_palette(k, &mut gc.colors);
    }

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
        lineno_active = hex(if k.text_muted == k.text_sub {
            k.text_main
        } else {
            k.text_sub
        }),
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
    crate::theme_nagi::NAGI_DARK,
    crate::theme_nagi::NAGI_LIGHT,
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
#[path = "theme_nagi_tests.rs"]
mod nagi_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_ink_uses_all_fill_states_not_theme_mode() {
        for palette in [
            &crate::theme_apple_light::APPLE_LIGHT,
            &crate::theme_apple_dark::APPLE_DARK,
        ] {
            for backgrounds in [
                [0x888888, 0x999999, 0xaaaaaa],
                [0x333333, 0x444444, 0x555555],
            ] {
                let ink = palette.readable_text_on(0x777777, &backgrounds, 4.5);
                for background in backgrounds {
                    assert!(contrast_ratio(ink, background) >= 4.5);
                }
            }
        }
    }

    #[test]
    fn status_label_ink_reads_on_rows_and_keeps_semantic_hues() {
        for t in THEMES {
            for color in [t.color_blocker, t.color_warning] {
                for (background, hover) in [
                    (t.panel, t.surface),
                    (t.inactive_selected, t.inactive_selected),
                    (t.diff_removed_bg, t.diff_removed_bg),
                ] {
                    let ink = t.status_label_text_on(color, background, hover);
                    for fill in [background, hover] {
                        assert!(
                            contrast_ratio(ink, fill) >= 4.5,
                            "{} status #{color:06x} on #{fill:06x}",
                            t.slug
                        );
                    }
                    let original = to_hsla(color);
                    let adjusted = to_hsla(ink);
                    let hue_delta = (original.h - adjusted.h).abs();
                    assert!(hue_delta.min(1.0 - hue_delta) < 0.02, "{}", t.slug);
                    assert!((original.s - adjusted.s).abs() < 0.03, "{}", t.slug);
                    assert_eq!(t.status_label_text_on(color, background, hover), ink);
                }
            }
        }
    }

    #[test]
    fn status_label_selected_ink_preserves_on_accent_semantics_and_aa() {
        let t = &crate::theme_apple_light::APPLE_LIGHT;
        for channel in 0..=255 {
            let background = channel * 0x010101;
            let primary = on_accent_foreground(background);
            let ink = t.status_label_text_on(primary, background, background);
            assert!(contrast_ratio(ink, background) >= 4.5);
            assert_eq!((ink >> 16) & 255, (ink >> 8) & 255);
            assert_eq!((ink >> 8) & 255, ink & 255);
            if contrast_ratio(primary, background) >= 4.6 {
                assert_eq!(ink, primary);
            }
        }
    }

    #[test]
    fn destructive_text_meets_aa_on_composited_conflict_chrome() {
        use gpui_component::Colorize as _;
        for t in [
            &crate::theme_apple_light::APPLE_LIGHT,
            &crate::theme_apple_dark::APPLE_DARK,
        ] {
            let mut kit = gpui_component::Theme::default();
            apply_gpui_component_theme(t, &mut kit);
            for background in [t.sidebar, t.surface] {
                let backgrounds = [0.2, 0.4].map(|amount| {
                    let fill = kit.colors.button.mix_oklab(kit.transparent, amount);
                    u32::from(to_hsla(background).blend(fill).to_rgb()) >> 8
                });
                let ink = t.destructive_text_on(&backgrounds);
                let original = to_hsla(t.color_blocker);
                let adjusted = to_hsla(ink);
                assert!((adjusted.h - original.h).abs() < 0.015);
                assert!((adjusted.s - original.s).abs() < 0.02);
                for bg in backgrounds {
                    assert!(
                        contrast_ratio(ink, bg) >= 4.5,
                        "{} destructive ink on #{bg:06x}",
                        t.slug
                    );
                }
            }
        }
    }

    #[test]
    fn apple_secondary_text_meets_wcag_aa_without_changing_light() {
        let light = &crate::theme_apple_light::APPLE_LIGHT;
        assert_eq!(light.text_sub, 0x606060);
        assert_eq!(light.text_label, 0x606060);
        assert_eq!(light.text_muted, 0x666666);
        assert_eq!(light.panel, 0xf7f7f7);
        assert_eq!(light.bg_base, 0xffffff);

        for t in [light, &crate::theme_apple_dark::APPLE_DARK] {
            for (role, foreground) in [
                ("text_sub", t.text_sub),
                ("text_label", t.text_label),
                ("text_muted", t.text_muted),
            ] {
                for (surface, background) in [("panel", t.panel), ("base", t.bg_base)] {
                    let contrast = contrast_ratio(foreground, background);
                    assert!(
                        contrast >= 4.5,
                        "{} {role} #{foreground:06x} on {surface} #{background:06x}: \
                         {contrast:.3}:1 is below 4.5:1",
                        t.slug
                    );
                }
            }
        }
    }

    #[test]
    fn apple_pr_dashboard_draft_text_meets_wcag_aa_on_row_backgrounds() {
        for t in [
            &crate::theme_apple_light::APPLE_LIGHT,
            &crate::theme_apple_dark::APPLE_DARK,
        ] {
            for (state, background) in [("rest", t.bg_base), ("hover", t.surface)] {
                for (role, foreground) in [("title", t.text_main), ("metadata/Draft", t.text_muted)]
                {
                    let contrast = contrast_ratio(foreground, background);
                    assert!(
                        contrast >= 4.5,
                        "{} draft row {role} on {state}: {contrast:.3}:1 < 4.5:1",
                        t.slug
                    );
                }
            }
        }
    }

    #[test]
    fn apple_dark_elevation_is_ordered_and_readable_with_light_pinned() {
        let dark = &crate::theme_apple_dark::APPLE_DARK;
        let chrome = dark.chrome_surface();
        assert!(
            relative_luminance(dark.bg_base) < relative_luminance(dark.panel),
            "Apple Dark base must be darker than its panel"
        );
        assert!(relative_luminance(dark.panel) < relative_luminance(chrome));
        assert!(relative_luminance(chrome) < relative_luminance(dark.modal));
        assert_eq!(dark.modal, 0x2a2a2c, "one elevated fill");
        for foreground in [
            dark.text_main,
            dark.text_sub,
            dark.text_label,
            dark.text_muted,
        ] {
            assert!(contrast_ratio(foreground, dark.modal) >= 4.5);
        }
        // Chip outlines separate the inset without sacrificing label contrast.
        assert!(contrast_ratio(dark.text_sub, dark.surface) >= 4.5);
        assert!(contrast_ratio(dark.text_main, dark.selected) >= 4.5);
        assert!(contrast_ratio(dark.elevated_border(dark.surface), chrome) >= 1.3);
        assert!(contrast_ratio(dark.elevated_border(dark.surface), dark.modal) >= 1.3);
        let mut dark_gc = gpui_component::Theme::default();
        apply_gpui_component_theme(dark, &mut dark_gc);
        assert_eq!(dark_gc.colors.popover, to_hsla(dark.modal));
        assert_eq!(dark_gc.colors.border, to_hsla(0x67676a).alpha(0.85));
        let light = &crate::theme_apple_light::APPLE_LIGHT;
        assert_eq!(
            [
                light.bg_base,
                light.panel,
                light.sidebar,
                light.surface,
                light.modal
            ],
            [0xffffff, 0xf7f7f7, 0xf7f7f7, 0xe5e5e5, 0xffffff]
        );
        let mut gc = gpui_component::Theme::default();
        apply_gpui_component_theme(light, &mut gc);
        assert_eq!(gc.colors.popover, to_hsla(0xffffff));
        assert_eq!(gc.colors.border, to_hsla(0x666666).alpha(0.85));
        let mut custom = dark.clone();
        custom.slug = Cow::Borrowed("custom-dark");
        custom.panel = 0x123456;
        custom.modal = 0x654321;
        assert_eq!(custom.elevated_surface(custom.panel), 0x123456);
        assert_eq!(custom.chrome_surface(), 0x123456);
        apply_gpui_component_theme(&custom, &mut gc);
        assert_eq!(gc.colors.popover, to_hsla(0x654321));
    }

    #[test]
    fn apple_dark_chrome_keeps_secondary_and_accent_contrast() {
        let dark = &crate::theme_apple_dark::APPLE_DARK;
        let mut gc = gpui_component::Theme::default();
        apply_gpui_component_theme(dark, &mut gc);
        let highlight = highlight_theme(dark);
        for foreground in [
            dark.text_label,
            dark.text_muted,
            dark.color_branch,
            u32::from(highlight.style.editor_line_number.unwrap().to_rgb()) >> 8,
            u32::from(highlight.style.editor_active_line_number.unwrap().to_rgb()) >> 8,
        ] {
            assert!(contrast_ratio(foreground, dark.bg_base) >= 4.5);
        }
        for thumb in [gc.colors.scrollbar_thumb, gc.colors.scrollbar_thumb_hover] {
            let rgba = thumb.to_rgb();
            let bg = gpui::rgb(dark.bg_base);
            let channel =
                |fg: f32, bg: f32| ((fg * rgba.a + bg * (1. - rgba.a)) * 255.).round() as u32;
            let painted = (channel(rgba.r, bg.r) << 16)
                | (channel(rgba.g, bg.g) << 8)
                | channel(rgba.b, bg.b);
            assert!(
                contrast_ratio(painted, dark.bg_base) >= 4.5,
                "Apple Dark scrollbar #{painted:06x} must remain readable on base"
            );
        }
    }

    #[test]
    fn apple_and_nagi_inputs_keep_identifying_contrast() {
        for t in THEMES.iter().filter(|t| {
            matches!(
                &*t.slug,
                "apple-light" | "apple-dark" | "nagi-light" | "nagi-dark"
            )
        }) {
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(t, &mut gc);
            let border = u32::from(gc.colors.input.to_rgb()) >> 8;
            for background in [t.bg_base, t.panel] {
                assert!(
                    contrast_ratio(border, background) >= 3.0,
                    "{} input boundary must remain identifiable",
                    t.slug
                );
            }
        }
    }

    #[test]
    fn equal_secondary_roles_keep_interaction_affordances() {
        for preset in THEMES {
            let mut t = preset.clone();
            t.text_muted = t.text_sub;
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(&t, &mut gc);
            assert_ne!(
                gc.colors.scrollbar_thumb, gc.colors.scrollbar_thumb_hover,
                "{}: scrollbar hover must remain distinct",
                t.slug
            );
            let highlight = highlight_theme(&t);
            assert_ne!(
                highlight.style.editor_line_number, highlight.style.editor_active_line_number,
                "{}: active line number must remain distinct",
                t.slug
            );
        }
    }

    #[test]
    fn structural_surfaces_have_a_visible_luminance_step() {
        for t in THEMES {
            for background in [t.panel, t.bg_base] {
                let contrast = contrast_ratio(t.surface, background);
                assert!(
                    contrast >= 1.15,
                    "{} surface contrast {contrast:.3}:1 on {background:#08x}",
                    t.slug
                );
            }
        }
    }

    #[test]
    fn body_links_meet_wcag_aa_in_every_theme() {
        for t in THEMES {
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(t, &mut gc);
            assert_eq!(
                gc.colors.link,
                to_hsla(t.link),
                "{}: owned body-link token",
                t.slug
            );
            assert!(
                contrast_ratio(t.link, t.bg_base) >= 4.5,
                "{}: link palette",
                t.slug
            );
            let link = u32::from(gc.colors.link.to_rgb()) >> 8;
            let contrast = contrast_ratio(link, t.bg_base);
            assert!(
                contrast >= 4.5,
                "{}: body link {link:#08x} is only {contrast:.2}:1 on {:#08x}",
                t.slug,
                t.bg_base,
            );
        }
    }

    #[test]
    fn body_link_override_is_independent_of_ref_and_primary_palettes() {
        let mut custom = crate::theme_apple_light::APPLE_LIGHT.clone();
        custom.slug = Cow::Borrowed("custom-link-test");
        assert_eq!(custom.link, 0x0066cc);
        assert_eq!(custom.color_branch, 0x0088ff);
        custom.link = 0x773399;
        let mut gc = gpui_component::Theme::default();
        apply_gpui_component_theme(&custom, &mut gc);
        assert_eq!(gc.colors.link, to_hsla(custom.link));
        assert_eq!(gc.colors.primary, to_hsla(0x0088ff));
        assert_eq!(gc.colors.button_primary, to_hsla(0x0088ff));
        custom.color_branch = 0xff9900;
        apply_gpui_component_theme(&custom, &mut gc);
        assert_eq!(gc.colors.link, to_hsla(0x773399));
        assert_eq!(gc.colors.button_primary, to_hsla(0xff9900));
    }

    #[test]
    fn incoming_button_tokens_follow_remote_in_every_theme() {
        for t in THEMES {
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(t, &mut gc);
            assert_eq!(
                gc.colors.button_info,
                to_hsla(t.color_remote),
                "{}: Incoming fill must match its pane/marker",
                t.slug,
            );
            let foreground = filled_button_foreground(t.color_remote, t);
            let (foreground, hover, active) =
                filled_button_interaction_colors(t.color_remote, foreground);
            assert_eq!(
                gc.colors.button_info_foreground,
                to_hsla(foreground),
                "{}",
                t.slug
            );
            assert_eq!(gc.colors.button_info_hover, hover, "{}", t.slug);
            assert_eq!(gc.colors.button_info_active, active, "{}", t.slug);
            for background in [
                gc.colors.button_info,
                gc.colors.button_info_hover,
                gc.colors.button_info_active,
            ] {
                assert!(background.is_opaque(), "{}: filled Incoming state", t.slug);
                assert!(
                    contrast_ratio(u32::from(background.to_rgb()) >> 8, foreground) >= 4.5,
                    "{}: Incoming label must stay readable in every state",
                    t.slug,
                );
            }
        }
    }

    #[test]
    fn destructive_button_labels_and_interactions_are_readable_in_every_theme() {
        for t in THEMES {
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(t, &mut gc);
            assert_eq!(gc.colors.button_danger, to_hsla(t.color_blocker));
            let foreground = u32::from(gc.colors.button_danger_foreground.to_rgb()) >> 8;
            let backgrounds = [
                gc.colors.button_danger,
                gc.colors.button_danger_hover,
                gc.colors.button_danger_active,
            ]
            .map(|fill| u32::from(fill.to_rgb()) >> 8);
            // Match the palette's HSL conversion, not a lossy RGB round-trip
            // of either the original fill or the generated caption.
            assert_eq!(
                gc.colors.button_danger_foreground,
                to_hsla(t.destructive_fill_text_on(&[
                    t.color_blocker,
                    backgrounds[1],
                    backgrounds[2],
                ])),
            );
            // The pinned Danger Button paints tokens, not just ThemeColor.
            assert_eq!(
                gpui::Background::from(gc.tokens.button_danger),
                gpui::Background::from(gc.colors.button_danger),
            );
            assert_eq!(
                gpui::Background::from(gc.tokens.button_danger_hover),
                gpui::Background::from(gc.colors.button_danger_hover),
            );
            assert_eq!(
                gpui::Background::from(gc.tokens.button_danger_active),
                gpui::Background::from(gc.colors.button_danger_active),
            );
            for (state, background) in [
                ("rest", gc.colors.button_danger),
                ("hover", gc.colors.button_danger_hover),
                ("pressed", gc.colors.button_danger_active),
            ] {
                assert!(background.is_opaque(), "{}: {state}", t.slug);
                let contrast = contrast_ratio(u32::from(background.to_rgb()) >> 8, foreground);
                assert!(
                    contrast >= 4.5,
                    "{}: destructive {state} label is only {contrast:.2}:1",
                    t.slug
                );
            }
            assert_ne!(
                gc.colors.button_danger_hover, gc.colors.button_danger,
                "{}: hover feedback",
                t.slug
            );
            assert_ne!(
                gc.colors.button_danger_active, gc.colors.button_danger_hover,
                "{}: pressed feedback",
                t.slug
            );
        }
    }

    #[test]
    fn incoming_button_fill_tracks_custom_remote_without_changing_status() {
        for base in THEMES {
            let mut custom = base.clone();
            custom.slug = Cow::Borrowed("custom-remote-test");
            custom.color_remote = 0x7a3da9;
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(&custom, &mut gc);
            assert_eq!(gc.colors.button_info, to_hsla(custom.color_remote));
            assert_eq!(gc.colors.button_primary, to_hsla(base.color_branch));
            assert_eq!(gc.colors.success, to_hsla(base.color_success));
            assert_eq!(gc.colors.danger, to_hsla(base.color_blocker));
        }
    }

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
                to_hsla(t.ui_selection_tint()).alpha(SELECTION_ALPHA),
                "{}: selection_overlay must use the resolved selection tint",
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
        assert_eq!(clamp_zoom(2.5), ZOOM_MAX);
        assert_eq!(clamp_zoom(2.0), 2.0, "200% is supported");
        assert_eq!(clamp_zoom(1.0), 1.0);
        assert_eq!(clamp_zoom(1.556), 1.556, "legacy 140% stays visible");
        assert_eq!(clamp_zoom(1.667), 1.667, "legacy 150% stays visible");
    }

    #[test]
    fn zoom_steps_follow_presets_and_clamp() {
        assert_eq!(step_zoom(1.111, true), 1.2);
        assert_eq!(step_zoom(1.111, false), 1.1);
        assert_eq!(step_zoom(1.0, true), 1.1);
        assert_eq!(step_zoom(1.0, false), 0.9);
        assert_eq!(step_zoom(ZOOM_MIN, false), ZOOM_MIN);
        assert_eq!(step_zoom(ZOOM_MAX, true), ZOOM_MAX);
        assert_eq!(step_zoom(1.667, false), 1.6);
        assert_eq!(step_zoom(1.667, true), 1.7);
        assert_eq!(step_zoom(1.67, false), 1.6);
        assert_eq!(step_zoom(1.9, true), 2.0);
        assert_eq!(step_zoom(2.0, false), 1.9);
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
    fn primary_button_labels_meet_theme_contrast_policy() {
        for t in THEMES {
            let foreground = primary_button_foreground(t);
            let contrast = contrast_ratio(t.ui_accent(), foreground);
            assert!(
                contrast
                    >= if matches!(&*t.slug, "apple-light" | "apple-dark") {
                        3.0
                    } else {
                        4.5
                    },
                "{}: primary button label {foreground:#08x} is only {contrast:.2}:1 on {:#08x}",
                t.slug,
                t.ui_accent(),
            );
        }
    }

    #[test]
    fn selection_on_accent_matches_activation_and_theme_identity() {
        for t in THEMES {
            let apple = matches!(&*t.slug, "apple-light" | "apple-dark");
            let on_selection = apple || t.slug == "nagi-dark";
            for selected in [false, true] {
                for window_active in [false, true] {
                    assert_eq!(
                        t.selection_uses_on_accent(selected, window_active),
                        on_selection && selected && window_active,
                        "{} selected={selected} window_active={window_active}",
                        t.slug
                    );
                }
            }
            let active = u32::from(t.selection_foreground(true).to_rgb()) >> 8;
            assert!(
                contrast_ratio(t.ui_selected(), active) >= 3.0,
                "{} selected label",
                t.slug
            );
            assert_eq!(t.selection_foreground(false), to_hsla(t.text_main));
            assert_eq!(t.selection_secondary_foreground(false), to_hsla(t.text_sub));
            if apple {
                assert_eq!(t.ui_selected(), t.ui_accent());
                assert_eq!(active, t.on_accent());
                assert_eq!(t.segment_selected_bg(), to_hsla(t.inactive_selected));
                assert_ne!(t.segment_selected_bg(), to_hsla(t.ui_accent()));
                assert_eq!(
                    t.selection_secondary_foreground(true),
                    t.selection_foreground(true).alpha(0.7)
                );
            } else {
                assert_eq!(t.ui_selected(), t.selected);
                assert_eq!(t.segment_selected_bg(), to_hsla(t.selected));
                if t.slug == "nagi-dark" {
                    assert_eq!(active, on_accent_foreground(t.selected));
                    assert_eq!(
                        t.selection_secondary_foreground(true),
                        t.selection_foreground(true)
                    );
                } else {
                    assert_eq!(t.selection_foreground(true), to_hsla(t.text_main));
                }
                assert_eq!(t.on_accent(), filled_button_foreground(t.ui_accent(), t));
            }
        }
    }

    #[test]
    fn graph_selection_wash_preserves_normal_primary_contrast() {
        for slug in ["apple-light", "apple-dark"] {
            let t = THEMES.iter().find(|t| &*t.slug == slug).unwrap();
            let accent = if t.dark {
                SystemAccent::Yellow
            } else {
                SystemAccent::Blue
            };
            let _injection = AccentInjection::new(accent, Some(true));
            assert_eq!(t.selection_wash(true).a, 0.30);
            assert_eq!(t.selection_wash(false), to_hsla(t.inactive_selected));
            for stripe in [t.bg_base, t.bg_row_alt] {
                let selected = to_hsla(stripe).blend(t.selection_wash(true));
                let selected_rgb = u32::from(selected.to_rgb()) >> 8;
                assert!(
                    contrast_ratio(t.text_main, selected_rgb) >= 4.5,
                    "{slug}: normal Graph primary ink on selected stripe"
                );
            }
        }
    }

    #[test]
    fn graph_selection_wash_keeps_non_apple_selection_colours() {
        for t in THEMES
            .iter()
            .filter(|t| !matches!(&*t.slug, "apple-light" | "apple-dark"))
        {
            for active in [false, true] {
                assert_eq!(
                    t.selection_wash(active),
                    t.selected_for_window(active),
                    "{}",
                    t.slug
                );
            }
        }
    }

    #[test]
    fn finder_accent_prefers_white_at_three_to_one_and_black_for_light_accents() {
        for accent in [
            SystemAccent::Blue,
            SystemAccent::Purple,
            SystemAccent::Red,
            SystemAccent::Pink,
        ] {
            for dark in [false, true] {
                let fill = accent.rgb(dark);
                assert_eq!(on_accent_foreground(fill), 0xffffff);
                assert!(contrast_ratio(fill, on_accent_foreground(fill)) >= 3.0);
            }
        }
        for accent in [
            SystemAccent::Yellow,
            SystemAccent::Green,
            SystemAccent::Orange,
        ] {
            for dark in [false, true] {
                let fill = accent.rgb(dark);
                assert_eq!(on_accent_foreground(fill), 0x000000);
                assert!(contrast_ratio(fill, on_accent_foreground(fill)) >= 3.0);
            }
        }
    }

    fn assert_primary_button_interaction_states(t: &Theme) {
        let foreground = primary_button_foreground(t);
        let (foreground, hover, pressed) =
            filled_button_interaction_colors(t.ui_accent(), foreground);
        assert!(
            hover.is_opaque() && pressed.is_opaque(),
            "{}: filled states",
            t.slug,
        );
        // A channel-sum floor rejects effectively identical fills; native
        // input/screenshots, not this arithmetic check, judge the appearance.
        let distance = |a: Hsla, b: Hsla| {
            let (a, b) = (a.to_rgb(), b.to_rgb());
            ((a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs()) * 255.0
        };
        let rest = to_hsla(t.ui_accent());
        for (a, b) in [(rest, hover), (hover, pressed), (rest, pressed)] {
            assert!(
                distance(a, b) >= 24.0,
                "{}: primary states {a:?} and {b:?} are too close",
                t.slug,
            );
        }
        let hover = u32::from(hover.to_rgb()) >> 8;
        let pressed = u32::from(pressed.to_rgb()) >> 8;
        for (state, background) in [
            ("rest", t.ui_accent()),
            ("hover", hover),
            ("pressed", pressed),
        ] {
            let contrast = contrast_ratio(background, foreground);
            assert!(
                contrast
                    >= if matches!(&*t.slug, "apple-light" | "apple-dark") {
                        3.0
                    } else {
                        4.5
                    },
                "{}: {state} label is only {contrast:.2}:1 on {background:#08x}",
                t.slug,
            );
        }
    }

    #[test]
    fn primary_button_interaction_states_are_distinct_and_legible() {
        for t in THEMES {
            assert_primary_button_interaction_states(t);
            let foreground = primary_button_foreground(t);
            assert_eq!(
                filled_button_interaction_colors(t.ui_accent(), foreground).0,
                foreground,
                "{}: stock foreground must remain unchanged",
                t.slug,
            );
        }
    }

    #[test]
    fn primary_button_custom_accent_edges_are_distinct_and_legible() {
        // Custom themes can inherit either mode and override the accent.
        // Exercise endpoint, near-endpoint, neutral and saturated colours
        // against every stock foreground choice, not just default Mocha.
        for base in THEMES {
            for accent in [
                0x000000, 0x010101, 0x202020, 0x808080, 0xe0e0e0, 0xfefefe, 0xffffff, 0xff0000,
                0x00ff00, 0x0000ff, 0xffff00, 0x00ffff, 0xff00ff, 0x000080, 0x800000, 0x006400,
                0x303030, 0xffff80, 0x80ffff, 0x00004d, 0xb3ffff,
            ] {
                let mut custom = base.clone();
                custom.slug = Cow::Borrowed("custom-primary-edges");
                custom.color_branch = accent;
                assert_primary_button_interaction_states(&custom);
            }
        }
    }

    #[test]
    fn primary_button_boundary_labels_use_one_legible_fallback() {
        for (accent, label) in [(0xffffff, 0x767676), (0x000000, 0x757575)] {
            let mut custom = THEMES[0].clone();
            custom.color_branch = accent;
            custom.bg_base = label;
            let established = primary_button_foreground(&custom);
            let (foreground, _, _) =
                filled_button_interaction_colors(custom.color_branch, established);
            assert_ne!(
                foreground, established,
                "a rest-only AA label cannot suppress endpoint feedback",
            );
            assert_primary_button_interaction_states(&custom);
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
            if contrast_ratio(t.ui_accent(), t.surface) >= 4.5 {
                assert_eq!(fg, t.ui_accent(), "{} keeps its accent", t.slug);
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

#[cfg(test)]
mod accent_indicator_tests {
    use super::*;

    #[test]
    fn component_indicators_prefer_white_at_three_to_one() {
        for theme in THEMES {
            let accent = theme.ui_accent();
            let expected = if contrast_ratio(accent, 0xffffff) >= 3.0 {
                0xffffff
            } else {
                0x000000
            };
            let mut component = gpui_component::Theme::default();
            apply_gpui_component_theme(theme, &mut component);
            assert_eq!(
                component.colors.primary_foreground,
                to_hsla(expected),
                "{}",
                theme.slug
            );
            assert!(contrast_ratio(accent, expected) >= 3.0, "{}", theme.slug);
            if matches!(&*theme.slug, "nagi-light" | "nagi-dark") {
                for foreground in [
                    component.colors.switch_thumb,
                    component.colors.slider_thumb,
                    component.colors.sidebar_primary_foreground,
                    component.colors.info_foreground,
                ] {
                    assert_eq!(foreground, to_hsla(expected), "{}", theme.slug);
                }
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod system_accent_tests {
    use super::*;

    #[test]
    fn apple_dark_default_keeps_yellow_chrome() {
        let _injected = AccentInjection::new(SystemAccent::Blue, None);
        let dark = &crate::theme_apple_dark::APPLE_DARK;
        let mut gc = gpui_component::Theme::default();
        apply_gpui_component_theme(dark, &mut gc);
        assert_eq!(dark.ui_accent(), dark.color_branch);
        assert_eq!(gc.colors.button_primary, to_hsla(dark.color_branch));
        assert_eq!(gc.colors.ring, to_hsla(dark.color_branch));
        assert_eq!(
            gc.colors.selection,
            to_hsla(dark.selection_tint).alpha(SELECTION_ALPHA)
        );
        assert_eq!(dark.ui_selected(), dark.ui_accent());
        let light = &crate::theme_apple_light::APPLE_LIGHT;
        assert_eq!(light.ui_accent(), SystemAccent::Blue.rgb(false));
    }

    #[test]
    fn theme_accent_source_restores_both_preset_palettes() {
        let _injected = AccentInjection::new(SystemAccent::Purple, Some(false));
        for theme in [
            &crate::theme_apple_light::APPLE_LIGHT,
            &crate::theme_apple_dark::APPLE_DARK,
        ] {
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(theme, &mut gc);
            assert_eq!(theme.ui_accent(), theme.color_branch);
            assert_eq!(theme.ui_selection_tint(), theme.selection_tint);
            assert_eq!(theme.ui_selected(), theme.ui_accent());
            assert_eq!(gc.colors.primary, to_hsla(theme.color_branch));
            assert_eq!(gc.colors.ring, to_hsla(theme.color_branch));
            assert_eq!(gc.colors.button_primary, to_hsla(theme.color_branch));
            assert_eq!(
                gc.colors.selection,
                to_hsla(theme.selection_tint).alpha(SELECTION_ALPHA)
            );
            let label = u32::from(gc.colors.button_primary_foreground.to_rgb()) >> 8;
            assert!(contrast_ratio(theme.color_branch, label) >= 3.0);
            for fill in [
                gc.colors.button_primary_hover,
                gc.colors.button_primary_active,
            ] {
                assert!(contrast_ratio(u32::from(fill.to_rgb()) >> 8, label) >= 3.0);
            }
        }
    }

    #[test]
    fn system_accent_chrome_mapping_and_contrast() {
        let _lock = ACTIVE_THEME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for value in -2..=6 {
            let accent = SystemAccent::from_default(Some(value));
            let _injected = AccentInjection::new(accent, Some(true));
            for theme in [
                &crate::theme_apple_light::APPLE_LIGHT,
                &crate::theme_apple_dark::APPLE_DARK,
            ] {
                let mut gc = gpui_component::Theme::default();
                apply_gpui_component_theme(theme, &mut gc);
                let expected = accent.rgb(theme.dark);
                assert_eq!(
                    gc.colors.ring,
                    to_hsla(expected),
                    "{value} dark={}",
                    theme.dark
                );
                assert_eq!(gc.colors.button_primary, to_hsla(expected));
                assert_eq!(
                    gc.colors.selection,
                    to_hsla(expected).alpha(SELECTION_ALPHA)
                );
                assert_eq!(theme.ui_selection_tint(), expected);
                assert_eq!(theme.ui_selected(), expected);
                let label = u32::from(gc.colors.button_primary_foreground.to_rgb()) >> 8;
                assert!(contrast_ratio(expected, label) >= 3.0);
                for fill in [
                    gc.colors.button_primary_hover,
                    gc.colors.button_primary_active,
                ] {
                    assert!(contrast_ratio(u32::from(fill.to_rgb()) >> 8, label) >= 3.0);
                }
                assert!(contrast_ratio(theme.accent_text_on(theme.bg_base), theme.bg_base) >= 4.5);
                assert!(contrast_ratio(theme.accent_text_on(theme.surface), theme.surface) >= 4.5);
                assert!(contrast_ratio(theme.on_accent(), theme.ui_selected()) >= 3.0);
            }
        }
    }

    #[test]
    fn system_accent_keeps_custom_and_other_theme_palettes() {
        let _injected = AccentInjection::new(SystemAccent::Purple, Some(true));
        assert!(
            !refresh_system_accent(),
            "injection must bypass native defaults"
        );
        for base in THEMES {
            if !matches!(&*base.slug, "apple-light" | "apple-dark") {
                assert_eq!(base.ui_accent(), base.color_branch);
                assert_eq!(base.ui_selection_tint(), base.selection_tint);
                assert_eq!(base.ui_selected(), base.selected);
                let mut gc = gpui_component::Theme::default();
                apply_gpui_component_theme(base, &mut gc);
                assert_eq!(gc.colors.primary, to_hsla(base.color_branch));
                assert_eq!(gc.colors.ring, to_hsla(base.color_branch));
            }
            let mut custom = base.clone();
            custom.slug = Cow::Borrowed("custom-accent");
            custom.color_branch = 0x804020;
            custom.selection_tint = 0x224466;
            let mut gc = gpui_component::Theme::default();
            apply_gpui_component_theme(&custom, &mut gc);
            assert_eq!(gc.colors.primary, to_hsla(0x804020));
            assert_eq!(gc.colors.ring, to_hsla(0x804020));
            assert_eq!(
                gc.colors.selection,
                to_hsla(0x224466).alpha(SELECTION_ALPHA)
            );
            assert_eq!(gc.colors.success, to_hsla(base.color_success));
            assert_eq!(gc.colors.button_info, to_hsla(base.color_remote));
            assert_eq!(custom.lane_hsl, base.lane_hsl);
        }
        assert_eq!(crate::theme_apple_light::APPLE_LIGHT.color_branch, 0x0088ff);
        assert_eq!(crate::theme_apple_dark::APPLE_DARK.color_branch, 0xffd600);
    }
}

#[cfg(test)]
mod inactive_selection_tests {
    use super::*;

    #[test]
    fn inactive_selection_primary_label_is_readable_in_every_preset() {
        for theme in THEMES {
            assert!(
                contrast_ratio(theme.text_main, theme.inactive_selected) >= 4.5,
                "{}: primary label must read on inactive selection",
                theme.slug
            );
            for (role, background) in [
                ("sidebar", theme.sidebar),
                ("base", theme.bg_base),
                ("alternate", theme.bg_row_alt),
                ("hover", theme.surface),
            ] {
                // Latte's unchanged label and hover luminances leave no 4.5:1
                // label fill reaching 1.15:1 against hover; #c3c3c3 gives 4.531
                // for the label and 1.142 against hover. Readability wins.
                let minimum = if theme.slug == "catppuccin-latte" && role == "hover" {
                    1.14
                } else {
                    1.15
                };
                assert!(
                    contrast_ratio(theme.inactive_selected, background) >= minimum,
                    "{}: inactive selection must differ from {role}",
                    theme.slug
                );
            }
            assert_eq!(
                theme.selected_for_window(false),
                rgb(theme.inactive_selected).into()
            );
            assert_eq!(
                theme.selected_for_window(true),
                to_hsla(theme.ui_selected())
            );
        }
    }

    #[test]
    fn apple_light_non_key_selection_is_neutral_and_restores_accent() {
        let theme = THEMES.iter().find(|t| t.slug == "apple-light").unwrap();
        assert_eq!(
            u32::from(theme.selected_for_window(false).to_rgb()) >> 8,
            0xd3d3d3
        );
        assert_ne!(
            theme.selected_for_window(false),
            theme.selected_for_window(true)
        );
        assert_eq!(
            theme.selected_for_window(true),
            to_hsla(theme.ui_selected())
        );
    }
}
