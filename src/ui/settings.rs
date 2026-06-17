//! Settings persistence (hand-written JSON; no serde — mirrors oplog.rs).
//!
//! kagi stores all user preferences (active theme slug, UI zoom, compact graph,
//! auto-fetch, language, smart-commit options, session repos, column widths, …)
//! in a single flat `{ "k": "v", ... }` JSON object at
//! `$KAGI_LOG_DIR/settings.json` (or `$HOME/.kagi/settings.json`).  This module
//! owns reading/writing that file; `theme.rs` and the other UI modules call
//! [`read_setting`]/[`write_setting`] here.

use std::path::PathBuf;

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

/// Extract the value of `"<key>"` from a flat JSON object string.
///
/// Intentionally minimal — scans for `"<key>"` and extracts the following
/// double-quoted string value.  No JSON dependency is added.  Values written by
/// [`write_setting`] contain no escapes, so closing-quote scanning is exact.
pub fn parse_string_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = text.find(&needle)?;
    let after = &text[key_pos + needle.len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let open = rest.find('"')?;
    let value_start = open + 1;
    let close = rest[value_start..].find('"')?;
    Some(rest[value_start..value_start + close].to_string())
}

/// Read a single string-valued setting from `settings.json`.
pub fn read_setting(key: &str) -> Option<String> {
    let path = settings_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    parse_string_value(&text, key)
}

/// Persist (or remove with `value = None`) one string-valued setting in
/// `settings.json`, **preserving all other keys**.  Best-effort; failures are
/// logged but non-fatal.  Creates the parent directory if needed.
///
/// The file is treated as a flat `{ "k": "v", ... }` object (the only shape kagi
/// ever writes).  Existing keys are re-read, the target key is upserted, and the
/// whole object is rewritten — so smart-commit keys never clobber `theme` and
/// vice-versa.
pub fn write_setting(key: &str, value: Option<&str>) {
    let path = match settings_path() {
        Some(p) => p,
        None => return,
    };
    // Re-read existing keys so we don't drop them.
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for k in &SETTINGS_KEYS {
            if *k == key {
                continue;
            }
            if let Some(v) = parse_string_value(&text, k) {
                pairs.push(((*k).to_string(), v));
            }
        }
    }
    if let Some(v) = value {
        pairs.push((key.to_string(), v.to_string()));
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("  \"{}\": \"{}\"", k, settings_escape(v)))
        .collect();
    let json = format!("{{\n{}\n}}\n", body.join(",\n"));
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("[kagi] settings: write failed (non-fatal): {}", e);
    }
}

/// Minimal escaping for settings values (`"` and `\`).  kagi only stores slugs /
/// model names / flags, so this is sufficient.
fn settings_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// All known string-valued `settings.json` keys.  Listed so [`write_setting`]
/// can round-trip every key it doesn't recognise as the current target.
pub const SETTINGS_KEYS: [&str; 16] = [
    "theme",
    "lang",
    "ui_zoom",
    // Background auto-fetch toggle (periodic + on-focus). Defaults on.
    "auto_fetch",
    // T-SETTINGS-001: compact commit-graph row height toggle.
    "graph_compact",
    "smart_commit_llm_enabled",
    "smart_commit_model",
    "smart_commit_lang",
    "smart_commit_style",
    "session_repos",
    "session_active",
    // W33-CONFLICT-DASHBOARD: external merge-tool command template (ADR-0060).
    // Read-only from kagi's side (the user sets it); listed here so kagi's own
    // settings writes preserve it.
    "mergetool",
    // ADR-0082 auto-update: startup-check toggle + skipped-version tag.
    "update_auto_check",
    "update_skipped",
    // Commit-list column widths (BRANCH/TAG, GRAPH), persisted on resize.
    "badge_col_w",
    "graph_col_w",
];

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_theme_slug_basic() {
        assert_eq!(
            parse_string_value("{\n  \"theme\": \"one-dark\"\n}\n", "theme").as_deref(),
            Some("one-dark")
        );
        assert_eq!(parse_string_value("{}", "theme"), None);
        assert_eq!(parse_string_value("garbage", "theme"), None);
    }

    #[test]
    fn parse_string_value_preserves_multiple_keys() {
        // write_setting round-trips every known key — make sure parsing finds
        // each one independently (smart-commit keys must not clobber theme).
        let json = "{\n  \"theme\": \"one-dark\",\n  \"smart_commit_model\": \"gemma:2b\"\n}\n";
        assert_eq!(
            parse_string_value(json, "theme").as_deref(),
            Some("one-dark")
        );
        assert_eq!(
            parse_string_value(json, "smart_commit_model").as_deref(),
            Some("gemma:2b")
        );
        assert_eq!(parse_string_value(json, "missing"), None);
    }

    #[test]
    fn ui_zoom_in_settings_keys() {
        assert!(SETTINGS_KEYS.contains(&"ui_zoom"));
    }

    // T-SETTINGS-001: the Settings window persists graph_compact through the same
    // flat settings.json storage; the key must be registered so write_setting
    // round-trips it (and never clobbers it when writing a sibling key).
    #[test]
    fn graph_compact_in_settings_keys() {
        assert!(SETTINGS_KEYS.contains(&"graph_compact"));
    }
}
