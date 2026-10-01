//! Shared-state guard for the GUI E2E runner (#516 slice 3).
//!
//! Every scenario runs in one process and shares files under `$KAGI_LOG_DIR`
//! and the runner's temporary directory. A scenario that passes must leave them
//! as it found them, apart from what the product records by design. After each
//! scenario the runner compares, and a difference fails the scenario like an
//! assertion would (with the #516 slice 1 evidence):
//!
//! - `settings.json`: every key and value, except the session the product
//!   saves as tabs open and close (`session_repos`, `session_active`,
//!   `recent_repos`).
//! - `worktree_ports.json` (the port store): byte for byte. A scenario that
//!   starts a terminal puts the store back with [`PortStore`].
//! - `operations.jsonl` (the oplog): operations append to it, so it may grow.
//!   What was there must stay byte for byte, and every appended entry must
//!   name a repository under the runner's temporary directory or a remote one.
//! - The runner's temporary directory (`TMPDIR` points there for the whole
//!   run, so `tempfile` and child processes use it): no new direct entry may
//!   remain.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Settings the product writes as runtime state, not configuration.
const SESSION_KEYS: [&str; 3] = ["session_repos", "session_active", "recent_repos"];

fn log_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("KAGI_LOG_DIR").expect("the runner sets KAGI_LOG_DIR"))
}

fn port_store_path() -> PathBuf {
    log_dir().join("worktree_ports.json")
}

/// The port store as it was, put back on drop. A scenario that starts a
/// terminal holds one for its whole length.
pub(crate) struct PortStore(Option<Vec<u8>>);

impl PortStore {
    pub(crate) fn keep() -> Self {
        Self(std::fs::read(port_store_path()).ok())
    }
}

impl Drop for PortStore {
    fn drop(&mut self) {
        let path = port_store_path();
        match &self.0 {
            Some(bytes) => std::fs::write(&path, bytes).expect("restore the port store"),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Saved settings keys as they were, put back on drop. A scenario whose
/// restore goes through a setter that also saves its key (`set_lang`,
/// `set_theme`, `set_zoom`, `set_diff_split`, …) holds one, so a key that was
/// absent is absent again.
pub(crate) struct SavedKeys(Vec<(&'static str, Option<String>)>);

impl SavedKeys {
    pub(crate) fn keep(keys: &[&'static str]) -> Self {
        Self(
            keys.iter()
                .map(|key| (*key, kagi::ui::settings::read_setting(key)))
                .collect(),
        )
    }
}

impl Drop for SavedKeys {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            kagi::ui::settings::write_setting(key, value.as_deref());
        }
    }
}

/// The shared state before a scenario.
pub(crate) struct Before {
    settings: Settings,
    ports: Option<Vec<u8>>,
    oplog: Vec<u8>,
    tmp: BTreeSet<String>,
}

/// `settings.json` as it reads: its keys (absent counts as no keys: the store
/// creates the file on the first write, often of a session key), or why it
/// does not parse — a broken file must not compare equal to an empty one
/// (#899 review).
#[derive(PartialEq)]
enum Settings {
    Keys(BTreeMap<String, serde_json::Value>),
    Unreadable { text: String, why: String },
}

fn settings() -> Settings {
    // A coalesced write may still be in memory.
    kagi::ui::settings::flush();
    let text = match std::fs::read_to_string(log_dir().join("settings.json")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Settings::Unreadable {
                text: String::new(),
                why: error.to_string(),
            }
        }
    };
    if text.trim().is_empty() {
        return Settings::Keys(BTreeMap::new());
    }
    match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text) {
        Ok(map) => Settings::Keys(
            map.into_iter()
                .filter(|(key, _)| !SESSION_KEYS.contains(&key.as_str()))
                .collect(),
        ),
        Err(error) => Settings::Unreadable {
            text,
            why: error.to_string(),
        },
    }
}

fn tmp_entries() -> BTreeSet<String> {
    std::fs::read_dir(std::env::temp_dir())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn snapshot() -> Before {
    Before {
        settings: settings(),
        ports: std::fs::read(port_store_path()).ok(),
        oplog: std::fs::read(log_dir().join("operations.jsonl")).unwrap_or_default(),
        tmp: tmp_entries(),
    }
}

fn shown(value: Option<&serde_json::Value>) -> String {
    value.map_or_else(|| "absent".to_string(), |value| value.to_string())
}

fn settings_changes(before: &Settings, report: &mut String) {
    let after = settings();
    match (before, &after) {
        (Settings::Keys(before), Settings::Keys(after)) => {
            let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
            for key in keys {
                let (old, new) = (before.get(key), after.get(key));
                if old != new {
                    let _ = writeln!(
                        report,
                        "- settings.json key `{key}`: {} → {}",
                        shown(old),
                        shown(new)
                    );
                }
            }
        }
        (before, after) if before != after => {
            let describe = |state: &Settings| match state {
                Settings::Keys(keys) => format!("{} key(s)", keys.len()),
                Settings::Unreadable { why, .. } => format!("unparsable ({why})"),
            };
            let _ = writeln!(
                report,
                "- settings.json: {} → {}",
                describe(before),
                describe(after)
            );
        }
        _ => {}
    }
}

fn store_text(bytes: &Option<Vec<u8>>) -> String {
    match bytes {
        Some(bytes) => String::from_utf8_lossy(bytes)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        None => "absent".to_string(),
    }
}

fn oplog_changes(before: &[u8], report: &mut String) {
    let after = std::fs::read(log_dir().join("operations.jsonl")).unwrap_or_default();
    if after.len() < before.len() || after[..before.len()] != *before {
        let _ = writeln!(
            report,
            "- operations.jsonl: its first {} bytes (entries already there) changed; now {} bytes",
            before.len(),
            after.len()
        );
        return;
    }
    // The fixture directories are gone by now, so a recorded path cannot be
    // canonicalized: accept the root as `TMPDIR` spells it and as resolved
    // (`/var/…` and `/private/var/…` on macOS).
    let raw = std::env::temp_dir();
    let root = std::fs::canonicalize(&raw).unwrap_or_else(|_| raw.clone());
    for line in String::from_utf8_lossy(&after[before.len()..]).lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            // An entry nobody can read hides whatever it was (#899 review).
            let shown: String = line.chars().take(120).collect();
            let _ = writeln!(
                report,
                "- operations.jsonl: an appended line is not a JSON entry: `{shown}`"
            );
            continue;
        };
        let repo = entry
            .get("repo")
            .and_then(|repo| repo.as_str())
            .unwrap_or("");
        let local = Path::new(repo);
        // A remote repository is named by host, not by a local path.
        if !local.is_absolute() {
            continue;
        }
        let under_root = local.starts_with(&root) || local.starts_with(&raw);
        if !under_root {
            let op = entry.get("op").and_then(|op| op.as_str()).unwrap_or("?");
            let _ = writeln!(
                report,
                "- operations.jsonl: `{op}` was recorded for `{repo}`, outside the runner's temporary directory `{}`",
                root.display()
            );
        }
    }
}

/// Fail the scenario if it left shared state different from `before`.
pub(crate) fn check(scenario: &str, before: &Before) {
    let mut report = String::new();
    settings_changes(&before.settings, &mut report);
    let ports = std::fs::read(port_store_path()).ok();
    if ports != before.ports {
        let _ = writeln!(
            report,
            "- worktree_ports.json: {} → {}",
            store_text(&before.ports),
            store_text(&ports)
        );
    }
    oplog_changes(&before.oplog, &mut report);
    for added in tmp_entries().difference(&before.tmp) {
        let _ = writeln!(
            report,
            "- temporary directory `{}`: new entry `{added}` remains",
            std::env::temp_dir().display()
        );
    }
    assert!(
        report.is_empty(),
        "[gui-e2e] scenario {scenario} left shared state changed (#516):\n{report}"
    );
}
