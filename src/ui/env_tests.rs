//! Tests for helpers that read `settings.json`, i.e. the process-global
//! `KAGI_LOG_DIR`.
//!
//! They cannot run in-process with the rest of the suite:
//!
//! - `commands::effective_keystroke` caches the overrides in a `OnceLock`, so
//!   only the *first* caller in a process ever sees the settings file;
//! - other tests (`graph_view`) set and **remove** `KAGI_LOG_DIR` concurrently,
//!   so an in-process `write_setting` here could land in the developer's real
//!   `~/.kagi/settings.json`.
//!
//! So each case runs as an `#[ignore]`d child test re-invoked in a fresh
//! process with `KAGI_LOG_DIR` pointed at a tempdir. The visible `#[test]` is
//! the parent that spawns it and asserts the child passed.

use std::path::Path;
use std::process::Command;

/// Spawn this same test binary for the single `#[ignore]`d test `name`, with
/// `KAGI_LOG_DIR` set to a fresh tempdir seeded with `settings`.
fn run_child(name: &str, settings: &str) {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("settings.json"), settings).expect("seed settings.json");
    // `module_path!()` is crate-qualified (`kagi::ui::env_tests`); libtest
    // names are not — drop the crate segment.
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, m)| m);
    let full = format!("{module}::{name}");
    let out = Command::new(std::env::current_exe().expect("current_exe"))
        .args([
            &full,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KAGI_LOG_DIR", tmp.path())
        .output()
        .expect("spawn self");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "child {full} failed:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // A typo'd name would filter everything out and still exit 0.
    assert!(
        stdout.contains("1 passed"),
        "child {full} did not run:\n{stdout}"
    );
}

// ── commands::effective_keystroke ─────────────────────────────

#[test]
fn effective_keystroke_honours_settings_overrides() {
    run_child(
        "effective_keystroke_child",
        r#"{
  "keybinding.view.toggleSidebar": "",
  "keybinding.repo.fetch": "ctrl-alt-x"
}"#,
    );
}

#[test]
#[ignore = "child process of effective_keystroke_honours_settings_overrides"]
fn effective_keystroke_child() {
    use crate::ui::commands::effective_keystroke;
    // Empty override means **unbound**, not "fall back to the default".
    assert_eq!(effective_keystroke("view.toggleSidebar"), None);
    // Non-empty override replaces the default.
    assert_eq!(
        effective_keystroke("repo.fetch").as_deref(),
        Some("ctrl-alt-x")
    );
    // No override -> registry default.
    assert_eq!(
        effective_keystroke("repo.push").as_deref(),
        Some("secondary-shift-k")
    );
    // Unknown command id -> nothing to bind.
    assert_eq!(effective_keystroke("no.such.command"), None);
}

// ── tabs::record_recent_repo ──────────────────────────────────

#[test]
fn record_recent_repo_dedupes_and_caps() {
    run_child("record_recent_repo_child", "{}");
}

#[test]
#[ignore = "child process of record_recent_repo_dedupes_and_caps"]
fn record_recent_repo_child() {
    use crate::ui::tabs::{recent_repos, record_recent_repo};

    // `recent_repos` filters to existing directories, so use real ones.
    let root = std::env::var("KAGI_LOG_DIR").expect("KAGI_LOG_DIR");
    let dir = |n: usize| {
        let p = Path::new(&root).join(format!("repo{n}"));
        std::fs::create_dir_all(&p).expect("mkdir");
        p
    };

    // New entries go to the front.
    record_recent_repo(&dir(1));
    record_recent_repo(&dir(2));
    assert_eq!(recent_repos(), vec![dir(2), dir(1)]);

    // Re-recording an existing entry moves it to the front without duplicating
    // (the `retain` has to run before the `insert`).
    record_recent_repo(&dir(1));
    assert_eq!(recent_repos(), vec![dir(1), dir(2)]);

    // The cap holds: RECENT_REPOS_MAX is 12.
    for n in 3..=20 {
        record_recent_repo(&dir(n));
    }
    let list = recent_repos();
    assert_eq!(list.len(), 12);
    assert_eq!(list[0], dir(20));
    assert_eq!(list[11], dir(9));
}
