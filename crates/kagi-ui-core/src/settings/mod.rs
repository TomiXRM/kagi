//! Settings persistence (serde-backed; issue #13 P4 / ADR-0091).
//!
//! kagi stores all user preferences (active theme slug, UI zoom, compact graph,
//! auto-fetch, language, smart-commit options, session repos, column widths, …)
//! in a single flat JSON object at `$KAGI_LOG_DIR/settings.json` (or
//! `$HOME/.kagi/settings.json`). Every value is written as a JSON **string**
//! (e.g. `"auto_fetch": "true"`, `"ui_zoom": "1000"`) — that on-disk shape is
//! unchanged from the original hand-rolled writer, so existing settings files
//! keep working.
//!
//! What changed in P4: the hand-written `{ "k": "v" }` scanner was replaced by a
//! real `serde_json` parse into a typed [`Settings`] value. Two long-standing
//! foot-guns are gone as a result:
//!
//! * **Unknown keys are preserved.** The previous writer re-read only the keys in
//!   a hard-coded `SETTINGS_KEYS` list, so any key not on that list was silently
//!   dropped whenever a sibling key was saved. [`write_setting`] now round-trips
//!   the *entire* object.
//! * **Robust parsing.** Whitespace, key ordering, and escaping are handled by
//!   `serde_json` rather than substring scanning.
//!
//! `theme.rs` and the other UI modules read through the typed [`Settings`]
//! accessors or the thin [`read_setting`]/[`write_setting`] string API here.
//!
//! What changed in #491: one owner for read-modify-write, in the [`store`]
//! submodule. The parsed document lives in a process-global store instead of
//! being re-read from disk for every key, saves go through a temp file +
//! rename instead of a truncating write, and a file that does not parse is
//! *kept* — moved aside under a reserved `settings.json.corrupt` name before a
//! fresh file is written, so a malformed file can no longer silently swallow
//! every other key (including `session_repos`, the restored tab set). A
//! *repeated* write of one key (a column-divider drag) updates memory and is
//! collapsed into one file write; every other write still lands immediately.

mod main_window_frame;
#[cfg(not(target_arch = "wasm32"))]
mod store;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod store_env_tests;
mod terminal_autolock;
mod window_frame;

pub use main_window_frame::MainWindowFrame;
#[cfg(not(target_arch = "wasm32"))]
pub use main_window_frame::{main_window_frame, write_main_window_frame};
#[cfg(not(target_arch = "wasm32"))]
pub use store::flush;
pub use terminal_autolock::terminal_auto_lock;
#[cfg(not(target_arch = "wasm32"))]
pub use terminal_autolock::{init_terminal_auto_lock, set_terminal_auto_lock};
pub use window_frame::SettingsWindowFrame;
#[cfg(not(target_arch = "wasm32"))]
pub use window_frame::{settings_window_frame, write_settings_window_frame};

use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use kagi_domain::compact::{DEFAULT_THRESHOLD_REM, MAX_THRESHOLD_REM, MIN_THRESHOLD_REM};

/// Toolbar caption preference; an unset preference follows the active preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarDisplay {
    IconOnly,
    IconAndText,
}

impl ToolbarDisplay {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IconOnly => "icon_only",
            Self::IconAndText => "icon_and_text",
        }
    }
}

static TOOLBAR_DISPLAY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(not(target_arch = "wasm32"))]
pub fn init_toolbar_display() {
    let mode = Settings::load().toolbar_display();
    TOOLBAR_DISPLAY.store(
        match mode {
            None => 0,
            Some(ToolbarDisplay::IconOnly) => 1,
            Some(ToolbarDisplay::IconAndText) => 2,
        },
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn toolbar_display(theme_slug: &str) -> ToolbarDisplay {
    match TOOLBAR_DISPLAY.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ToolbarDisplay::IconOnly,
        2 => ToolbarDisplay::IconAndText,
        _ if matches!(
            theme_slug,
            "apple-light" | "apple-dark" | "nagi-light" | "nagi-dark"
        ) =>
        {
            ToolbarDisplay::IconOnly
        }
        _ => ToolbarDisplay::IconAndText,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_toolbar_display(mode: ToolbarDisplay) {
    write_setting("toolbar_display", Some(mode.as_str()));
    TOOLBAR_DISPLAY.store(
        match mode {
            ToolbarDisplay::IconOnly => 1,
            ToolbarDisplay::IconAndText => 2,
        },
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// The legacy scalar coercion: kagi writes strings, but tolerate bool/number.
fn scalar_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse the legacy scalar zoom representation, shared by typed reads and the
/// one-time base migration.
fn parse_ui_zoom(value: &serde_json::Value) -> Option<u32> {
    scalar_to_string(value)?.trim().parse::<u32>().ok()
}

/// Initial view for a newly opened Markdown buffer in the Editor workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownDefaultView {
    #[default]
    Preview,
    Source,
}

impl MarkdownDefaultView {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Source => "source",
        }
    }
}

/// What Cmd+C copies from the selected Graph row (`graph_copy_target`;
/// ADR-0170). Default [`CopyTarget::Hash`] — every commit has a hash, but not
/// every row carries a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyTarget {
    #[default]
    Hash,
    Branch,
}

/// Five sidebar pane weights in local / remote / worktree / tag / stash order
/// and a persisted snapshot of the collapsed-section set (#864).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarPaneLayout {
    pub weights: [u16; 5],
    pub collapsed_mask: u8,
}

impl Default for SidebarPaneLayout {
    fn default() -> Self {
        Self {
            weights: [3500, 2500, 2000, 1000, 1000],
            collapsed_mask: 0,
        }
    }
}

impl SidebarPaneLayout {
    pub fn parse(raw: &str) -> Option<Self> {
        let (version, rest) = raw.split_once(':')?;
        if version != "v1" {
            return None;
        }
        let (weights, mask) = rest.split_once(':')?;
        let mut parts = weights.split(',');
        let mut weights = [0_u16; 5];
        for weight in &mut weights {
            *weight = parts.next()?.parse().ok()?;
            if *weight == 0 {
                return None;
            }
        }
        if parts.next().is_some() {
            return None;
        }
        let collapsed_mask = mask.parse::<u8>().ok().filter(|&m| m < 32)?;
        Some(Self {
            weights,
            collapsed_mask,
        })
    }

    pub fn encode(self) -> String {
        let w = self.weights;
        format!(
            "v1:{},{},{},{},{}:{}",
            w[0], w[1], w[2], w[3], w[4], self.collapsed_mask
        )
    }
}

/// Typed view of `settings.json`: a flat map of string-valued settings plus
/// typed accessors that apply the same coercions the call sites used to do
/// inline. Unknown keys are retained in `raw` so a save never drops them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(flatten)]
    raw: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    /// Snapshot of the live settings document. Served from the store — no read
    /// or parse unless the file changed behind our back (#491). A missing file
    /// yields `Settings::default()` (empty); an *unparsable* one also reads as
    /// empty, but the original is kept on disk and moved aside rather than
    /// overwritten.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load() -> Self {
        Self {
            raw: store::snapshot(),
        }
    }

    /// Raw string value for `key`, coercing the legacy scalar encodings (every
    /// value kagi writes is a JSON string, but tolerate bool/number too).
    pub fn get_str(&self, key: &str) -> Option<String> {
        scalar_to_string(self.raw.get(key)?)
    }

    /// Upsert `key` with a string value.
    pub fn set_str(&mut self, key: &str, value: &str) {
        self.raw.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    /// Remove `key` if present.
    pub fn remove(&mut self, key: &str) {
        self.raw.remove(key);
    }

    // ── Typed accessors (apply the call sites' historical coercions) ──────────

    /// Active theme slug (`"theme"`), if persisted.
    pub fn theme(&self) -> Option<String> {
        self.get_str("theme")
    }

    pub fn toolbar_display(&self) -> Option<ToolbarDisplay> {
        match self
            .raw
            .get("toolbar_display")
            .and_then(serde_json::Value::as_str)
        {
            Some("icon_only") => Some(ToolbarDisplay::IconOnly),
            Some("icon_and_text") => Some(ToolbarDisplay::IconAndText),
            _ => None,
        }
    }

    /// Markdown opening mode (`"preview"` / `"source"`). Missing or invalid
    /// values use Preview; changing this never changes an already-open buffer.
    pub fn editor_markdown_default_view(&self) -> MarkdownDefaultView {
        match self
            .raw
            .get("editor_markdown_default_view")
            .and_then(serde_json::Value::as_str)
        {
            Some("source") => MarkdownDefaultView::Source,
            _ => MarkdownDefaultView::Preview,
        }
    }

    /// Graph Cmd+C copy target (`graph_copy_target`, `"hash"`/`"branch"`;
    /// ADR-0170). Defaults to [`CopyTarget::Hash`] when unset or unrecognized.
    pub fn graph_copy_target(&self) -> CopyTarget {
        match self.get_str("graph_copy_target").as_deref().map(str::trim) {
            Some("branch") => CopyTarget::Branch,
            _ => CopyTarget::Hash,
        }
    }

    /// Last window size (`"window_size"`, `"1440x920"`). `None` when unset or
    /// unparsable; the caller validates against its minimum and falls back.
    pub fn window_size(&self) -> Option<(f32, f32)> {
        let raw = self.get_str("window_size")?;
        let (w, h) = raw.trim().split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }

    /// Bottom-panel body height in unscaled reference pixels (#1372).
    /// The UI applies its minimum and viewport-dependent maximum.
    pub fn bottom_panel_height(&self) -> Option<f32> {
        self.get_str("bottom_panel_height")?
            .parse::<f32>()
            .ok()
            .filter(|height| height.is_finite() && *height > 0.)
    }

    /// UI zoom, stored as a permille integer string (`"ui_zoom"`). Returns the
    /// parsed permille; the caller clamps and divides by 1000.
    pub fn ui_zoom_permille(&self) -> Option<u32> {
        self.raw.get("ui_zoom").and_then(parse_ui_zoom)
    }

    /// Compact-layout switch width in whole root rem (`"compact_threshold_rem"`).
    /// Missing, fractional, malformed or out-of-range values use the default;
    /// tolerate legacy integer JSON scalars as well as the normal string form.
    pub fn compact_threshold_rem(&self) -> u32 {
        self.get_str("compact_threshold_rem")
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|value| (MIN_THRESHOLD_REM..=MAX_THRESHOLD_REM).contains(value))
            .unwrap_or(DEFAULT_THRESHOLD_REM)
    }

    /// Compact-graph flag (`"graph_compact"`, `"true"`/`"false"`). `None` when
    /// unset so the caller keeps its default. NOTE: this controls the compact
    /// *row height*, not lane compaction — see [`Self::graph_lane_compact`].
    pub fn graph_compact(&self) -> Option<bool> {
        self.get_str("graph_compact").map(|s| s.trim() == "true")
    }

    /// Split-diff flag (`"diff_split"`, `"true"`/`"false"`; ADR-0124). `None`
    /// when unset so the caller keeps its default (unified).
    pub fn diff_split(&self) -> Option<bool> {
        self.get_str("diff_split").map(|s| s.trim() == "true")
    }

    /// Inline-blame toggle (`"blame_inline"`, `"true"`/`"false"`; issue #350).
    /// Persists the Editor Workspace's blame chip across sessions. Default off
    /// — only an explicit `"true"` turns it on.
    pub fn blame_inline(&self) -> bool {
        self.get_str("blame_inline")
            .map(|s| s.trim() == "true")
            .unwrap_or(false)
    }

    /// Swimlane-visuals flag (`"graph_lane_compact"`, `"true"`/`"false"`).
    /// When `true` the commit graph draws swimlane visuals (avatar nodes,
    /// lane tint band, lane padding). The lane *layout* itself is always
    /// the gitk-style `graph::layout` (ADR-0122) and is not affected by this key.
    /// `None` when unset so the caller keeps its default (on).
    pub fn graph_lane_compact(&self) -> Option<bool> {
        self.get_str("graph_lane_compact")
            .map(|s| s.trim() == "true")
    }

    /// Background auto-fetch flag (`"auto_fetch"`). `None` when unset (default
    /// on); only an explicit `"false"` disables it.
    pub fn auto_fetch(&self) -> Option<bool> {
        self.get_str("auto_fetch").map(|s| s.trim() != "false")
    }

    /// Reduce-motion flag (`"reduce_motion"`, `"true"`/`"false"`; issue #354 /
    /// ADR-0173). When on, kagi renders its looping decorative animations
    /// static (the loading dots stop bobbing). Default **off** — only an
    /// explicit `"true"` enables it.
    pub fn reduce_motion(&self) -> bool {
        self.get_str("reduce_motion")
            .map(|s| s.trim() == "true")
            .unwrap_or(false)
    }

    /// Terminal auto-lock opt-in (`"terminal_auto_lock"`, `"true"`/`"false"`;
    /// #772 / ADR-0208). When on, starting the embedded terminal in a linked
    /// worktree offers a `git worktree lock` plan with a Kagi-owned token, and
    /// the shell's exit offers its release. Default **off** — only an explicit
    /// `"true"` enables it.
    pub fn terminal_auto_lock(&self) -> bool {
        self.get_str("terminal_auto_lock")
            .map(|s| s.trim() == "true")
            .unwrap_or(false)
    }

    /// User-extensible agent-provenance detection patterns (`"agent_patterns"`,
    /// issue #337 / ADR-0150). A comma-separated list of `label:needle` entries
    /// (bare `needle` uses itself as the label) layered on top of the built-in
    /// defaults in `kagi_domain::provenance`. Empty when unset — the built-ins
    /// still apply. Kept as a flat string on disk per the settings rules.
    pub fn agent_patterns(&self) -> Vec<kagi_domain::provenance::AgentPattern> {
        self.get_str("agent_patterns")
            .map(|s| kagi_domain::provenance::AgentPattern::parse_list(&s))
            .unwrap_or_default()
    }

    /// Automatic pre-destructive savepoint snapshots (`"auto_snapshot"`,
    /// ADR-0154 / #335). Default ON — only an explicit `"false"` disables it.
    /// The UI feeds this to `Backend::set_auto_snapshot`.
    pub fn auto_snapshot(&self) -> bool {
        self.get_str("auto_snapshot")
            .map(|s| s.trim() != "false")
            .unwrap_or(true)
    }

    /// Per-worktree port range (`"worktree.port_range"`, e.g. `"3000-3099"`;
    /// issue #342 / ADR-0171). Returned as `(start, end)` inclusive. Defaults to
    /// `(3000, 3099)` when unset or unparsable so worktree port allocation
    /// always has a valid range to draw from.
    pub fn worktree_port_range(&self) -> (u16, u16) {
        self.get_str("worktree.port_range")
            .as_deref()
            .and_then(kagi_domain::worktree_ports::parse_port_range)
            .map(|r| (r.start, r.end))
            .unwrap_or((3000, 3099))
    }

    /// How many consecutive ports each worktree reserves
    /// (`"worktree.ports_per_worktree"`; issue #342 / ADR-0171). Defaults to
    /// `10` (the Conductor `CONDUCTOR_PORT` precedent). A `0` or unparsable
    /// value falls back to the default so allocation never reserves nothing.
    pub fn worktree_ports_per_worktree(&self) -> u16 {
        self.get_str("worktree.ports_per_worktree")
            .and_then(|s| s.trim().parse::<u16>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(10)
    }

    /// `"worktree_run_mode"` (#859 / ADR-0213): `"nonconcurrent"` lets one
    /// worktree of a repository at a time run a terminal shell. Anything else,
    /// unset included, is `Concurrent`.
    pub fn worktree_run_mode(&self) -> kagi_domain::worktree_run_mode::RunMode {
        self.get_str("worktree_run_mode")
            .as_deref()
            .map(kagi_domain::worktree_run_mode::RunMode::parse)
            .unwrap_or_default()
    }

    /// Five Graph sidebar pane weights and collapse bits (#864). A missing,
    /// malformed (including the unreleased six-weight form), zero-weight or
    /// unsupported version uses defaults in memory; reading never repairs the
    /// raw value on disk.
    pub fn sidebar_pane_layout(&self) -> SidebarPaneLayout {
        self.get_str("sidebar_panes")
            .as_deref()
            .and_then(SidebarPaneLayout::parse)
            .unwrap_or_default()
    }
}

/// Default contents of the `analyze_ignore` file (gitignore syntax), seeded on
/// first run. There are **no hardcoded exclusions** beyond this editable file —
/// clear it to analyze everything (ADR-0119).
pub const DEFAULT_ANALYZE_IGNORE: &str = "\
# Analyze ignore — gitignore syntax. Files matching any pattern are excluded
# from Hotspots / Coupling / Ownership. Edit freely: wildcards (* ** ?) and
# negation (!) work exactly like .gitignore. Delete everything to analyze all.

# Documents
*.pdf

# Images
*.png
*.jpg
*.jpeg
*.gif
*.bmp
*.webp
*.ico
*.icns
*.tif
*.tiff
*.svg
*.heic
*.heif
*.avif
*.psd
*.ai
*.eps

# CAD / 3D models
*.step
*.stp
*.stl
*.iges
*.igs
*.3mf

# Fonts
*.ttf
*.otf
*.ttc
*.woff
*.woff2
*.eot

# Archives
*.zip

# KiCad
*.kicad_*
fp-info-cache
";

/// Path to the `analyze_ignore` file (sibling of `settings.json`).
#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_ignore_path() -> Option<PathBuf> {
    Some(settings_path()?.with_file_name("analyze_ignore"))
}

/// Read the full text of the `analyze_ignore` file, seeding it with
/// [`DEFAULT_ANALYZE_IGNORE`] on first run so the user has an editable,
/// documented starting point.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_analyze_ignore_text() -> String {
    let Some(path) = analyze_ignore_path() else {
        return DEFAULT_ANALYZE_IGNORE.to_string();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                let _ = write_analyze_ignore(DEFAULT_ANALYZE_IGNORE);
            }
            DEFAULT_ANALYZE_IGNORE.to_string()
        }
    }
}

/// Persist Analyze ignore text through its settings owner, returning the OS cause.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_analyze_ignore(content: &str) -> std::io::Result<()> {
    let path = analyze_ignore_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Analyze ignore settings directory unavailable",
        )
    })?;
    write_analyze_ignore_at(&path, content)
        .map_err(|error| std::io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_analyze_ignore_at(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::atomic_file::write_atomic(path, content.as_bytes())
}

/// The Analyze exclude patterns (gitignore syntax, one per line) — comments /
/// blanks included (the matcher ignores them).
#[cfg(not(target_arch = "wasm32"))]
pub fn analyze_ignore_patterns() -> Vec<String> {
    read_analyze_ignore_text()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Resolve the path to `settings.json` (`$KAGI_LOG_DIR/settings.json` first,
/// then `$HOME/.kagi/settings.json`).  Returns `None` if no directory can be
/// determined.
#[cfg(not(target_arch = "wasm32"))]
pub fn settings_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("settings.json"));
        }
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())?;
    Some(PathBuf::from(home).join(".kagi").join("settings.json"))
}

/// Read a single string-valued setting from `settings.json`.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_setting(key: &str) -> Option<String> {
    store::read(key)
}

/// Persist (or remove with `value = None`) one string-valued setting in
/// `settings.json`, **preserving every other key** — including ones this build
/// doesn't know about. Best-effort; failures are logged but non-fatal.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_setting(key: &str, value: Option<&str>) {
    store::write(key, value);
}

/// Migrate saved zoom from the previous 90%-sized UI base to the new base.
/// Returns the current saved permille when it parses, or `None` when absent
/// or malformed. The store persists the zoom and version marker atomically.
#[cfg(not(target_arch = "wasm32"))]
pub fn migrate_ui_scale_base() -> Option<u32> {
    store::migrate_ui_scale_base()
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Settings {
        serde_json::from_str(text).unwrap_or_default()
    }

    #[test]
    fn bottom_panel_body_height_is_finite_and_positive() {
        assert_eq!(
            parse(r#"{"bottom_panel_height":"202.5"}"#).bottom_panel_height(),
            Some(202.5)
        );
        for value in ["NaN", "inf", "-inf", "0", "-2", "invalid"] {
            let mut settings = Settings::default();
            settings.set_str("bottom_panel_height", value);
            assert_eq!(settings.bottom_panel_height(), None, "{value}");
        }
        assert_eq!(Settings::default().bottom_panel_height(), None);
    }

    #[test]
    fn compact_threshold_decodes_whole_rem_strings_and_legacy_integers() {
        for value in [MIN_THRESHOLD_REM, 70, 80, MAX_THRESHOLD_REM] {
            for encoded in [format!("\" {value} \""), value.to_string()] {
                let settings = parse(&format!(r#"{{"compact_threshold_rem":{encoded}}}"#));
                assert_eq!(settings.compact_threshold_rem(), value, "{encoded}");
            }
        }
    }

    #[test]
    fn compact_threshold_defaults_for_missing_or_unusable_values() {
        assert_eq!(
            Settings::default().compact_threshold_rem(),
            DEFAULT_THRESHOLD_REM
        );
        for value in [
            "",
            "invalid",
            "NaN",
            "inf",
            "-inf",
            "Infinity",
            "-Infinity",
            "70.5",
            "70.0",
            "7e1",
            "80.0",
            "8e1",
            "-70",
            "0",
            "39",
            "161",
            "4294967296",
        ] {
            let mut settings = Settings::default();
            settings.set_str("compact_threshold_rem", value);
            assert_eq!(
                settings.compact_threshold_rem(),
                DEFAULT_THRESHOLD_REM,
                "{value}"
            );
        }
        for encoded in [
            "null", "true", "false", "[]", "{}", "70.5", "70.0", "80.0", "8e1", "-70", "39", "161",
        ] {
            let settings = parse(&format!(r#"{{"compact_threshold_rem":{encoded}}}"#));
            assert_eq!(
                settings.compact_threshold_rem(),
                DEFAULT_THRESHOLD_REM,
                "{encoded}"
            );
        }
        // A profile predating this preference keeps its other settings.
        let legacy = parse(r#"{"ui_zoom":"1250","future":{"preserve":true}}"#);
        assert_eq!(legacy.compact_threshold_rem(), DEFAULT_THRESHOLD_REM);
        assert_eq!(legacy.ui_zoom_permille(), Some(1250));
        let saved = serde_json::to_value(&legacy).expect("serialize");
        assert_eq!(saved["future"]["preserve"], true);
    }

    #[test]
    fn editor_markdown_default_view_defaults_to_preview() {
        assert_eq!(
            Settings::default().editor_markdown_default_view(),
            MarkdownDefaultView::Preview
        );
        for value in [
            r#""invalid""#,
            r#""""#,
            r#""SOURCE""#,
            "true",
            "42",
            "null",
            "[]",
            "{}",
        ] {
            let settings = parse(&format!(r#"{{"editor_markdown_default_view":{value}}}"#));
            assert_eq!(
                settings.editor_markdown_default_view(),
                MarkdownDefaultView::Preview,
                "{value}"
            );
        }
    }

    #[test]
    fn editor_markdown_default_view_round_trips_flat_strings() {
        for mode in [MarkdownDefaultView::Preview, MarkdownDefaultView::Source] {
            let mut settings = parse(r#"{"future":{"preserve":true},"ui_zoom":"1250"}"#);
            settings.set_str("editor_markdown_default_view", mode.as_str());
            let serialized = serde_json::to_value(&settings).unwrap();
            assert_eq!(serialized["editor_markdown_default_view"], mode.as_str());
            assert_eq!(serialized["future"]["preserve"], true);
            let reloaded: Settings = serde_json::from_value(serialized).unwrap();
            assert_eq!(reloaded.editor_markdown_default_view(), mode);
            assert_eq!(reloaded.ui_zoom_permille(), Some(1250));
        }
    }

    #[test]
    fn toolbar_display_round_trips_flat_strings() {
        for mode in [ToolbarDisplay::IconOnly, ToolbarDisplay::IconAndText] {
            let mut settings = parse(r#"{"future":"preserve"}"#);
            settings.set_str("toolbar_display", mode.as_str());
            let serialized = serde_json::to_string(&settings).unwrap();
            assert_eq!(parse(&serialized).toolbar_display(), Some(mode));
            assert_eq!(
                parse(&serialized).get_str("future").as_deref(),
                Some("preserve")
            );
        }
        assert_eq!(parse("{}").toolbar_display(), None);
        assert_eq!(
            parse(r#"{"toolbar_display":"invalid"}"#).toolbar_display(),
            None
        );
    }

    #[test]
    fn get_str_basic() {
        assert_eq!(
            parse("{\n  \"theme\": \"one-dark\"\n}\n")
                .get_str("theme")
                .as_deref(),
            Some("one-dark")
        );
        assert_eq!(parse("{}").get_str("theme"), None);
        // Malformed input parses to an empty Settings (best-effort).
        assert_eq!(parse("garbage").get_str("theme"), None);
    }

    #[test]
    fn get_str_finds_each_key_independently() {
        // Smart-commit keys must not clobber theme.
        let s = parse("{\n  \"theme\": \"one-dark\",\n  \"smart_commit_model\": \"gemma:2b\"\n}\n");
        assert_eq!(s.get_str("theme").as_deref(), Some("one-dark"));
        assert_eq!(s.get_str("smart_commit_model").as_deref(), Some("gemma:2b"));
        assert_eq!(s.get_str("missing"), None);
    }

    #[test]
    fn typed_accessors_apply_legacy_coercions() {
        let s: Settings = serde_json::from_str(
            r#"{ "theme": "one-dark", "ui_zoom": "1250", "graph_compact": "true", "auto_fetch": "false" }"#,
        )
        .unwrap();
        assert_eq!(s.theme().as_deref(), Some("one-dark"));
        assert_eq!(s.ui_zoom_permille(), Some(1250));
        assert_eq!(s.graph_compact(), Some(true));
        assert_eq!(s.auto_fetch(), Some(false));

        // Unset typed flags return None so callers keep their defaults.
        let empty = Settings::default();
        assert_eq!(empty.graph_compact(), None);
        assert_eq!(empty.auto_fetch(), None);
        assert_eq!(empty.ui_zoom_permille(), None);
    }

    #[test]
    fn worktree_port_accessors_default_and_parse() {
        // Defaults when unset (issue #342 / ADR-0171).
        let empty = Settings::default();
        assert_eq!(empty.worktree_port_range(), (3000, 3099));
        assert_eq!(empty.worktree_ports_per_worktree(), 10);

        // Parsed from the flat string values.
        let s: Settings = serde_json::from_str(
            r#"{ "worktree.port_range": "4000-4099", "worktree.ports_per_worktree": "5" }"#,
        )
        .unwrap();
        assert_eq!(s.worktree_port_range(), (4000, 4099));
        assert_eq!(s.worktree_ports_per_worktree(), 5);

        // Unparsable / zero fall back to the defaults.
        let bad: Settings = serde_json::from_str(
            r#"{ "worktree.port_range": "nope", "worktree.ports_per_worktree": "0" }"#,
        )
        .unwrap();
        assert_eq!(bad.worktree_port_range(), (3000, 3099));
        assert_eq!(bad.worktree_ports_per_worktree(), 10);
    }

    #[test]
    fn sidebar_pane_layout_rejects_invalid_weights_and_mask_without_repairing_raw() {
        let valid = SidebarPaneLayout {
            weights: [8, 13, 21, 34, 55],
            collapsed_mask: 0b10101,
        };
        let encoded = valid.encode();
        let parsed = parse(&format!(r#"{{"sidebar_panes":"{encoded}"}}"#));
        assert_eq!(parsed.sidebar_pane_layout(), valid);
        assert_eq!(
            parsed.get_str("sidebar_panes").as_deref(),
            Some(encoded.as_str())
        );

        for invalid in [
            "garbage",
            "v2:1,2,3,4,5:0",
            "v1:1,2,3,4:0",
            // The unreleased six-pane value (with a PR pane) is not migrated.
            "v1:1,2,3,4,5,6:0",
            "v1:1,2,0,4,5:0",
            "v1:1,2,3,4,65536:0",
            "v1:1,2,3,4,5:32",
            "v1:1,2,3,4,5:0:extra",
        ] {
            let settings = parse(&format!(r#"{{"sidebar_panes":"{invalid}"}}"#));
            assert_eq!(
                settings.sidebar_pane_layout(),
                SidebarPaneLayout::default(),
                "{invalid}"
            );
            assert_eq!(
                settings.get_str("sidebar_panes").as_deref(),
                Some(invalid),
                "a read must not rewrite malformed user settings"
            );
        }
    }

    #[test]
    fn analyze_ignore_writer_reports_directory_and_parent_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analyze_ignore");
        std::fs::create_dir(&path).unwrap();
        let original = path.join("original");
        std::fs::write(&original, b"saved bytes").unwrap();
        assert!(write_analyze_ignore_at(&path, "draft").is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"saved bytes");
        std::fs::remove_file(&original).unwrap();
        std::fs::remove_dir(&path).unwrap();
        write_analyze_ignore_at(&path, "draft").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "draft");
        assert!(write_analyze_ignore_at(&path.join("child"), "draft").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "draft");
    }

    #[test]
    fn analyze_ignore_failed_atomic_replacement_preserves_saved_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("analyze_ignore");
        write_analyze_ignore_at(&path, "saved bytes").unwrap();
        let temp = dir
            .path()
            .join(format!(".analyze_ignore.{}.tmp", std::process::id()));
        std::fs::create_dir(&temp).unwrap();
        assert!(write_analyze_ignore_at(&path, "draft").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved bytes");
        std::fs::remove_dir(&temp).unwrap();
        write_analyze_ignore_at(&path, "draft").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "draft");
        assert!(!temp.exists());
    }

    #[test]
    fn reduce_motion_default_off_and_parses_true() {
        // Default off when unset (issue #354 / ADR-0173).
        assert!(!Settings::default().reduce_motion());
        // Explicit "true" enables it; anything else stays off.
        let on: Settings = serde_json::from_str(r#"{ "reduce_motion": "true" }"#).unwrap();
        assert!(on.reduce_motion());
        let off: Settings = serde_json::from_str(r#"{ "reduce_motion": "false" }"#).unwrap();
        assert!(!off.reduce_motion());
    }

    #[test]
    fn write_preserves_unknown_keys() {
        // The old writer dropped keys not in SETTINGS_KEYS; the serde writer
        // round-trips the *whole* object, so an unknown key survives when a
        // sibling key is set. Tested purely through the same serialize/parse path
        // `write_setting` + `Settings::load` use — no global `KAGI_LOG_DIR` env or
        // file, so it stays isolated under parallel `cargo test` (other tests
        // mutate `KAGI_LOG_DIR` concurrently).
        let mut s: Settings =
            serde_json::from_str("{\n  \"future_only_key\": \"keepme\"\n}\n").unwrap();
        s.set_str("theme", "one-dark");

        let serialized = serde_json::to_string_pretty(&s).unwrap();
        let reloaded: Settings = serde_json::from_str(&serialized).unwrap();

        assert_eq!(
            reloaded.get_str("future_only_key").as_deref(),
            Some("keepme")
        );
        assert_eq!(reloaded.get_str("theme").as_deref(), Some("one-dark"));
    }
}
