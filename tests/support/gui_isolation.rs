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
//!   The check is lexical (a `..` fails it); it catches a scenario that leaks
//!   by mistake, not one that routes a receipt through a symlink on purpose —
//!   the fixtures are gone by then, so a link cannot be resolved (#899 review).
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
        Self(read_shared(&port_store_path()))
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
    oplog: Option<Vec<u8>>,
    tmp: BTreeSet<String>,
    quarantined: BTreeSet<String>,
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

/// A shared file's bytes, `None` when absent. Any other read failure fails the
/// scenario: an unreadable file must not compare equal to an empty one (#899
/// review).
fn read_shared(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!(
            "[gui-e2e] shared state `{}` cannot be read: {error}",
            path.display()
        ),
    }
}

fn tmp_entries() -> BTreeSet<String> {
    let dir = std::env::temp_dir();
    std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("[gui-e2e] `{}` cannot be listed: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!("[gui-e2e] `{}` cannot be listed: {error}", dir.display())
                })
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// `settings.json.corrupt[.N]` files: the store moves a file that does not
/// parse aside instead of overwriting it, so a scenario that broke and then
/// rewrote the settings leaves one behind even when every key is back
/// (#899 review).
fn quarantined() -> BTreeSet<String> {
    let dir = log_dir();
    std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("[gui-e2e] `{}` cannot be listed: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!("[gui-e2e] `{}` cannot be listed: {error}", dir.display())
                })
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("settings.json.corrupt"))
        .collect()
}

pub(crate) fn snapshot() -> Before {
    Before {
        settings: settings(),
        ports: read_shared(&port_store_path()),
        oplog: read_shared(&log_dir().join("operations.jsonl")),
        tmp: tmp_entries(),
        quarantined: quarantined(),
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

fn oplog_changes(before: &Option<Vec<u8>>, report: &mut String) {
    let after = read_shared(&log_dir().join("operations.jsonl"));
    // The writer creates the file with its first entry: absent and empty are
    // different states, and neither transition between them is an append
    // (#899 review).
    match (before, &after) {
        (Some(_), None) => {
            let _ = writeln!(report, "- operations.jsonl: removed");
            return;
        }
        (None, Some(bytes)) if bytes.is_empty() => {
            let _ = writeln!(report, "- operations.jsonl: created empty");
            return;
        }
        _ => {}
    }
    let before = before.as_deref().unwrap_or_default();
    let after = after.unwrap_or_default();
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
    // Valid JSON is not yet an entry (`{}`, `null`): the product's reader
    // skips what it cannot decode, so every line must survive it (#899 review).
    let lines = String::from_utf8_lossy(&after)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let decoded = kagi_git::oplog::read_oplog_tail(lines + 1).len();
    if decoded != lines {
        let _ = writeln!(
            report,
            "- operations.jsonl: {lines} line(s) but the oplog reader decodes {decoded}"
        );
    }
    let raw = std::env::temp_dir();
    let root = std::fs::canonicalize(&raw).unwrap_or_else(|_| raw.clone());
    for line in String::from_utf8_lossy(&after[before.len()..]).lines() {
        if line.trim().is_empty() {
            // The writer only appends JSON entries (#899 review).
            let _ = writeln!(report, "- operations.jsonl: a blank line was appended");
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
        let Some(repo) = entry
            .get("repo")
            .and_then(|repo| repo.as_str())
            .filter(|repo| !repo.is_empty())
        else {
            let shown: String = line.chars().take(120).collect();
            let _ = writeln!(
                report,
                "- operations.jsonl: an appended entry names no repository: `{shown}`"
            );
            continue;
        };
        let local = Path::new(repo);
        // A remote receipt is scoped `<host label>:<path>` (src/remote/mod.rs);
        // any other non-absolute path is a malformed local scope (#899 review).
        if !local.is_absolute() {
            let remote = repo.split_once(':').is_some_and(|(host, path)| {
                !host.is_empty() && !host.contains('/') && !path.is_empty()
            });
            if !remote {
                let _ = writeln!(
                    report,
                    "- operations.jsonl: `{repo}` is neither an absolute local path nor `host:path`"
                );
            }
            continue;
        }
        // `starts_with` is lexical: a `..` could climb out of the root.
        let climbs = local
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir));
        let under_root = !climbs && (local.starts_with(&root) || local.starts_with(&raw));
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
    let ports = read_shared(&port_store_path());
    if ports != before.ports {
        let _ = writeln!(
            report,
            "- worktree_ports.json: {} → {}",
            store_text(&before.ports),
            store_text(&ports)
        );
    }
    oplog_changes(&before.oplog, &mut report);
    let quarantined = quarantined();
    for name in quarantined.symmetric_difference(&before.quarantined) {
        let change = if quarantined.contains(name) {
            "left behind"
        } else {
            "removed"
        };
        let _ = writeln!(report, "- settings quarantine `{name}`: {change}");
    }
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
