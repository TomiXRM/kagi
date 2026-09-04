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

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What Cmd+C copies from the selected Graph row (`graph_copy_target`;
/// ADR-0170). Default [`CopyTarget::Hash`] — every commit has a hash, but not
/// every row carries a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyTarget {
    #[default]
    Hash,
    Branch,
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
    /// Load and parse `settings.json`. A missing or unparsable file yields
    /// `Settings::default()` (empty) — settings are always best-effort.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Persist the whole object to `settings.json` (pretty, trailing newline),
    /// creating the parent directory if needed. Best-effort; failures are logged.
    pub fn save(&self) {
        let Some(path) = settings_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, format!("{json}\n")) {
                    klog!("settings: write failed (non-fatal): {e}");
                }
            }
            Err(e) => klog!("settings: serialize failed (non-fatal): {e}"),
        }
    }

    /// Raw string value for `key`, coercing the legacy scalar encodings (every
    /// value kagi writes is a JSON string, but tolerate bool/number too).
    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.raw.get(key)? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
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

    /// UI zoom, stored as a permille integer string (`"ui_zoom"`). Returns the
    /// parsed permille; the caller clamps and divides by 1000.
    pub fn ui_zoom_permille(&self) -> Option<u32> {
        self.get_str("ui_zoom")?.trim().parse::<u32>().ok()
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
    /// `None` when unset so the caller defaults to off.
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
pub fn analyze_ignore_path() -> Option<PathBuf> {
    Some(settings_path()?.with_file_name("analyze_ignore"))
}

/// Read the full text of the `analyze_ignore` file, seeding it with
/// [`DEFAULT_ANALYZE_IGNORE`] on first run so the user has an editable,
/// documented starting point.
pub fn read_analyze_ignore_text() -> String {
    let Some(path) = analyze_ignore_path() else {
        return DEFAULT_ANALYZE_IGNORE.to_string();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, DEFAULT_ANALYZE_IGNORE);
            DEFAULT_ANALYZE_IGNORE.to_string()
        }
    }
}

/// Persist new `analyze_ignore` contents (the Settings pane editor). Best-effort.
pub fn write_analyze_ignore(content: &str) {
    let Some(path) = analyze_ignore_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

/// The Analyze exclude patterns (gitignore syntax, one per line) — comments /
/// blanks included (the matcher ignores them).
pub fn analyze_ignore_patterns() -> Vec<String> {
    read_analyze_ignore_text()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Resolve the path to `settings.json` (`$KAGI_LOG_DIR/settings.json` first,
/// then `$HOME/.kagi/settings.json`).  Returns `None` if no directory can be
/// determined.
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
pub fn read_setting(key: &str) -> Option<String> {
    Settings::load().get_str(key)
}

/// Persist (or remove with `value = None`) one string-valued setting in
/// `settings.json`, **preserving every other key** — including ones this build
/// doesn't know about. Best-effort; failures are logged but non-fatal.
pub fn write_setting(key: &str, value: Option<&str>) {
    let mut settings = Settings::load();
    match value {
        Some(v) => settings.set_str(key, v),
        None => settings.remove(key),
    }
    settings.save();
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
