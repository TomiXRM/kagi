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

    /// Lane-compaction flag (`"graph_lane_compact"`, `"true"`/`"false"`). When
    /// `true` the commit graph uses Gitru-style swimlane compaction
    /// (`GraphLayoutMode::Compact`); otherwise the gitk-stable layout. `None`
    /// when unset so the caller defaults to Stable.
    pub fn graph_lane_compact(&self) -> Option<bool> {
        self.get_str("graph_lane_compact")
            .map(|s| s.trim() == "true")
    }

    /// Background auto-fetch flag (`"auto_fetch"`). `None` when unset (default
    /// on); only an explicit `"false"` disables it.
    pub fn auto_fetch(&self) -> Option<bool> {
        self.get_str("auto_fetch").map(|s| s.trim() != "false")
    }
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
