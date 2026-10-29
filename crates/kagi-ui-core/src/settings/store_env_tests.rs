//! Settings-store tests that need the **process-global** store and the real
//! `KAGI_LOG_DIR` (#491, PR #617 review). They cannot run in-process with the
//! rest of the suite: the store is global, `KAGI_LOG_DIR` is a process-wide
//! env var, and sibling tests in this crate (`avatar`, `theme`) set and remove
//! it concurrently.
//!
//! So each case runs as an `#[ignore]`d child test re-invoked in a fresh
//! process with `KAGI_LOG_DIR` pointed at a tempdir. The visible `#[test]` is
//! the parent that spawns it and checks the outcome — the same pattern as
//! `src/ui/env_tests.rs`.

use std::path::Path;
use std::process::{Command, Output};

/// Spawn this same test binary for the single `#[ignore]`d test `name`, with
/// `KAGI_LOG_DIR` set to `dir`.
fn spawn_child(name: &str, dir: &Path) -> Output {
    // `module_path!()` is crate-qualified; libtest names are not.
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, m)| m);
    Command::new(std::env::current_exe().expect("current_exe"))
        .args([
            &format!("{module}::{name}"),
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KAGI_LOG_DIR", dir)
        .output()
        .expect("spawn self")
}

/// Spawn the child and require that it passed.
fn run_child(name: &str, dir: &Path) {
    let out = spawn_child(name, dir);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "child {name} failed:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // A typo'd name would filter everything out and still exit 0.
    assert!(
        stdout.contains("1 passed"),
        "child {name} did not run:\n{stdout}"
    );
}

fn settings_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("settings.json")).expect("settings.json")
}

#[test]
fn public_scale_migration_uses_the_live_settings_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("settings.json"),
        r#"{"ui_zoom":"1000","future_only_key":"keepme"}"#,
    )
    .expect("seed");
    run_child("public_scale_migration_child", tmp.path());
    let saved: serde_json::Value = serde_json::from_str(&settings_text(tmp.path())).expect("json");
    assert_eq!(saved["ui_zoom"], "1111");
    assert_eq!(saved["ui_scale_base"], "v2");
    assert_eq!(saved["future_only_key"], "keepme");
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn public_scale_migration_child() {
    use crate::settings::{migrate_ui_scale_base, read_setting, settings_path};
    let path = settings_path().expect("settings path");
    assert_eq!(migrate_ui_scale_base(), Some(1111));
    assert_eq!(read_setting("ui_zoom").as_deref(), Some("1111"));
    let first_bytes = std::fs::read(&path).expect("persisted");
    assert_eq!(migrate_ui_scale_base(), Some(1111));
    assert_eq!(std::fs::read(&path).expect("persisted"), first_bytes);
}

#[test]
fn compact_threshold_uses_the_live_settings_store_and_restores_after_restart() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("settings.json"),
        r#"{"compact_threshold_rem":"80","session_repos":"/a","future":{"preserve":true}}"#,
    )
    .expect("seed");
    run_child("compact_threshold_persistence_child", tmp.path());
    let saved: serde_json::Value = serde_json::from_str(&settings_text(tmp.path())).expect("json");
    assert_eq!(saved["compact_threshold_rem"], "160");
    assert_eq!(saved["session_repos"], "/a");
    assert_eq!(saved["future"]["preserve"], true);
    run_child("compact_threshold_restart_child", tmp.path());
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn compact_threshold_persistence_child() {
    use crate::settings::{flush, settings_path, Settings};
    use crate::theme::{
        compact_threshold_rem, init_compact_threshold_rem, set_compact_threshold_rem,
    };
    use kagi_domain::compact::{MAX_THRESHOLD_REM, MIN_THRESHOLD_REM};

    let path = settings_path().expect("settings path");
    let original = std::fs::read(&path).expect("seeded settings");
    init_compact_threshold_rem();
    assert_eq!(compact_threshold_rem(), 80);
    assert_eq!(std::fs::read(&path).expect("settings"), original);

    for (requested, expected) in [
        (0, MIN_THRESHOLD_REM),
        (MIN_THRESHOLD_REM - 1, MIN_THRESHOLD_REM),
        (MIN_THRESHOLD_REM, MIN_THRESHOLD_REM),
        (80, 80),
        (MAX_THRESHOLD_REM, MAX_THRESHOLD_REM),
        (MAX_THRESHOLD_REM + 1, MAX_THRESHOLD_REM),
        (u32::MAX, MAX_THRESHOLD_REM),
    ] {
        assert_eq!(set_compact_threshold_rem(requested), expected);
        assert_eq!(compact_threshold_rem(), expected);
        assert_eq!(Settings::load().compact_threshold_rem(), expected);
        flush();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("settings")).expect("json");
        assert_eq!(
            saved["compact_threshold_rem"].as_str(),
            Some(expected.to_string().as_str())
        );
        assert_eq!(saved["session_repos"], "/a");
        assert_eq!(saved["future"]["preserve"], true);
    }
}

#[test]
fn compact_threshold_initialization_leaves_missing_or_invalid_preferences_untouched() {
    for document in [
        r#"{"ui_zoom":"1250","future":"preserve"}"#,
        r#"{"compact_threshold_rem":"invalid","future":"preserve"}"#,
        r#"{"compact_threshold_rem":"70.5","future":"preserve"}"#,
        r#"{"compact_threshold_rem":"161","future":"preserve"}"#,
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("settings.json"), document).expect("seed");
        run_child("compact_threshold_default_child", tmp.path());
        assert_eq!(settings_text(tmp.path()), document);
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let document = r#"{"compact_threshold_rem":80,"future":"preserve"}"#;
    std::fs::write(tmp.path().join("settings.json"), document).expect("seed");
    run_child("compact_threshold_legacy_integer_child", tmp.path());
    assert_eq!(settings_text(tmp.path()), document);
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn compact_threshold_default_child() {
    assert_compact_threshold_initialization(70);
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn compact_threshold_legacy_integer_child() {
    assert_compact_threshold_initialization(80);
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn compact_threshold_restart_child() {
    assert_compact_threshold_initialization(160);
}

fn assert_compact_threshold_initialization(expected: u32) {
    use crate::settings::settings_path;
    use crate::theme::{compact_threshold_rem, init_compact_threshold_rem};

    let path = settings_path().expect("settings path");
    let original = std::fs::read(&path).expect("seeded settings");
    init_compact_threshold_rem();
    assert_eq!(compact_threshold_rem(), expected);
    assert_eq!(std::fs::read(&path).expect("settings"), original);
}

#[test]
fn accent_source_migration_uses_the_live_settings_store() {
    for (document, expected) in [
        (
            r#"{"accent":"blue","future_only_key":"keepme"}"#,
            Some("theme"),
        ),
        (
            r#"{"accent":"system","future_only_key":"keepme"}"#,
            Some("system"),
        ),
        (
            r#"{"accent_source":"blue","future_only_key":"keepme"}"#,
            Some("theme"),
        ),
        (
            r#"{"accent_source":"theme","accent":"system","future_only_key":"keepme"}"#,
            Some("theme"),
        ),
        (r#"{"future_only_key":"keepme"}"#, None),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("settings.json"), document).expect("seed");
        run_child("accent_source_migration_child", tmp.path());
        let saved: serde_json::Value =
            serde_json::from_str(&settings_text(tmp.path())).expect("json");
        assert_eq!(saved["accent_source"].as_str(), expected);
        assert!(saved.get("accent").is_none(), "legacy key removed");
        assert_eq!(saved["future_only_key"], "keepme");
    }
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn accent_source_migration_child() {
    use crate::{settings::read_setting, system_accent};
    system_accent::init();
    match read_setting("accent_source").as_deref() {
        Some("system") => {
            assert!(system_accent::follows_system_accent_for(false));
            assert!(system_accent::follows_system_accent_for(true));
        }
        Some("theme") => {
            assert!(!system_accent::follows_system_accent_for(false));
            assert!(!system_accent::follows_system_accent_for(true));
        }
        None => {
            assert!(system_accent::follows_system_accent_for(false));
            assert!(!system_accent::follows_system_accent_for(true));
        }
        other => panic!("uncanonical accent source: {other:?}"),
    }
    assert!(read_setting("accent").is_none());
}

// ── #617 review 3: the public read path sees a stat-invisible edit ─────────

#[test]
fn read_path_sees_an_edit_that_keeps_mtime_and_length() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("settings.json"),
        "{ \"theme\": \"aaaaaaaa\" }\n",
    )
    .expect("seed");
    run_child("read_path_sees_stat_invisible_edit_child", tmp.path());
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn read_path_sees_stat_invisible_edit_child() {
    use crate::settings::{read_setting, settings_path};
    let path = settings_path().expect("settings path");

    assert_eq!(read_setting("theme").as_deref(), Some("aaaaaaaa"));

    // Replace the contents in place with the same length, then restore the
    // timestamps: `stat` now reports exactly what it reported before.
    let md = std::fs::metadata(&path).expect("metadata");
    let times = std::fs::FileTimes::new()
        .set_accessed(md.accessed().unwrap_or(std::time::SystemTime::UNIX_EPOCH))
        .set_modified(md.modified().expect("mtime"));
    std::fs::write(&path, "{ \"theme\": \"bbbbbbbb\" }\n").expect("in-place edit");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_times(times))
        .expect("pin mtime");
    let after = std::fs::metadata(&path).expect("metadata");
    assert_eq!(after.len(), md.len(), "same length");
    assert_eq!(
        after.modified().expect("mtime"),
        md.modified().expect("mtime"),
        "same mtime — the edit must be invisible to stat"
    );

    // The content re-check is rate-limited, not skipped.
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(
        read_setting("theme").as_deref(),
        Some("bbbbbbbb"),
        "a stat-invisible edit must still be picked up by the read path"
    );
}

// ── #617 review 4: what a burst actually defers, and what a quit saves ─────

#[test]
fn burst_uses_a_real_trailing_thread_and_quit_flushes_it() {
    let tmp = tempfile::tempdir().expect("tempdir");
    run_child("burst_and_graceful_quit_child", tmp.path());
    let text = settings_text(tmp.path());
    assert!(
        text.contains("\"graph_col_w\": \"199\""),
        "the final drag value must be on disk after flush: {text}"
    );
    assert!(text.contains("\"session_repos\""), "{text}");
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn burst_and_graceful_quit_child() {
    use crate::settings::{flush, read_setting, settings_path, write_setting};
    let path = settings_path().expect("settings path");

    // A committed save of one key is never deferred.
    write_setting("session_repos", Some("/a\u{1f}/b"));
    assert!(
        std::fs::read_to_string(&path)
            .expect("written")
            .contains("session_repos"),
        "a session save must reach disk immediately"
    );

    // A drag: the same key over and over, through the real schedule + thread.
    for w in 0..200 {
        write_setting("graph_col_w", Some(&w.to_string()));
    }
    // Reads during the burst see the latest value even before it is written.
    assert_eq!(read_setting("graph_col_w").as_deref(), Some("199"));

    flush();
    let text = std::fs::read_to_string(&path).expect("written");
    assert!(text.contains("\"graph_col_w\": \"199\""), "{text}");
    assert!(text.contains("session_repos"), "{text}");
}

#[test]
fn settings_window_bounds_burst_coalesces_frame() {
    let tmp = tempfile::tempdir().expect("tempdir");
    run_child("settings_window_bounds_burst_child", tmp.path());
    let saved: serde_json::Value = serde_json::from_str(&settings_text(tmp.path())).expect("json");
    assert_eq!(
        saved["settings_window_frame"],
        "99,198,760,000000000000000000000000000000ab"
    );
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn settings_window_bounds_burst_child() {
    use crate::settings::{flush, write_settings_window_frame, SettingsWindowFrame};
    let before = super::store::committed_flushes();
    for i in 0..100 {
        write_settings_window_frame(SettingsWindowFrame {
            x: i as f32,
            y: (i * 2) as f32,
            height: 760.,
            display_uuid: Some(0xab),
        });
    }
    flush();
    let commits = super::store::committed_flushes() - before;
    assert!(
        commits <= 2,
        "100 bounds changes must coalesce into <=2 commits, got {commits}"
    );
}

#[test]
fn settings_window_frame_migrates_legacy_geometry_once() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("settings.json"),
        r#"{"settings_window_position":"-120,240","settings_window_height":"640","future_only_key":"keepme"}"#)
        .expect("seed");
    run_child("settings_window_frame_migration_child", tmp.path());
    let saved: serde_json::Value = serde_json::from_str(&settings_text(tmp.path())).expect("json");
    assert_eq!(saved["settings_window_frame"], "-120,240,640");
    assert_eq!(saved["future_only_key"], "keepme");
    assert!(saved.get("settings_window_position").is_none());
    assert!(saved.get("settings_window_height").is_none());
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn settings_window_frame_migration_child() {
    use crate::settings::{read_setting, settings_window_frame, SettingsWindowFrame};
    let expected = Some(SettingsWindowFrame {
        x: -120.,
        y: 240.,
        height: 640.,
        display_uuid: None,
    });
    assert_eq!(settings_window_frame(), expected);
    assert!(read_setting("settings_window_position").is_none());
    assert!(read_setting("settings_window_height").is_none());
    let committed = super::store::committed_flushes();
    assert_eq!(settings_window_frame(), expected);
    assert_eq!(
        super::store::committed_flushes(),
        committed,
        "migration never repeats"
    );
}

#[test]
fn a_hard_kill_loses_only_the_repeated_key() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = spawn_child("hard_exit_mid_burst_child", tmp.path());
    assert_eq!(
        out.status.code(),
        Some(7),
        "child must have exited abruptly:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = settings_text(tmp.path());
    // The committed saves survived a quit with no flush at all…
    assert!(text.contains("\"session_repos\""), "{text}");
    assert!(text.contains("\"theme\": \"one-dark\""), "{text}");
    // …and the drag key is on disk too, just not necessarily at its last value.
    assert!(text.contains("\"graph_col_w\""), "{text}");
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn hard_exit_mid_burst_child() {
    use crate::settings::write_setting;
    write_setting("session_repos", Some("/a\u{1f}/b"));
    write_setting("theme", Some("one-dark"));
    for w in 0..200 {
        write_setting("graph_col_w", Some(&w.to_string()));
    }
    // No flush, no unwinding, no destructors — the SIGKILL case.
    std::process::exit(7);
}

// ── #617 review 5: pending state is owned per destination ─────────────────

#[test]
fn a_failed_flush_is_not_lost_when_the_settings_path_moves() {
    let a = tempfile::tempdir().expect("tempdir a");
    let b = tempfile::tempdir().expect("tempdir b");
    // The child needs both paths; KAGI_LOG_DIR starts at A.
    let out = Command::new(std::env::current_exe().expect("current_exe"))
        .args([
            &format!(
                "{}::path_switch_child",
                module_path!()
                    .split_once("::")
                    .map_or(module_path!(), |(_, m)| m)
            ),
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KAGI_LOG_DIR", a.path())
        .env("KAGI_TEST_DIR_B", b.path())
        .output()
        .expect("spawn self");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "child failed:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("1 passed"), "child did not run:\n{stdout}");

    // A's pending value survived the detour through B…
    let a_text = settings_text(a.path());
    assert!(a_text.contains("\"stranded\": \"keepme\""), "{a_text}");
    assert!(a_text.contains("\"recovered\": \"yes\""), "{a_text}");
    // …and never leaked into B.
    let b_text = settings_text(b.path());
    assert!(!b_text.contains("stranded"), "{b_text}");
    assert!(b_text.contains("\"only_b\": \"1\""), "{b_text}");
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn path_switch_child() {
    use crate::settings::write_setting;
    let a = std::env::var("KAGI_LOG_DIR").expect("A");
    let b = std::env::var("KAGI_TEST_DIR_B").expect("B");
    let a_file = Path::new(&a).join("settings.json");

    // Make every write to A fail: `settings.json` is a directory, so it reads
    // as corrupt and cannot be rescued (a directory will not rename over the
    // reserved file), which is exactly the "flush failed, keep it pending" path.
    std::fs::create_dir(&a_file).expect("block A");
    write_setting("stranded", Some("keepme"));
    assert!(a_file.is_dir(), "A must still be blocked");

    // Switch to B and write there. A's pending value must not follow it.
    std::env::set_var("KAGI_LOG_DIR", &b);
    write_setting("only_b", Some("1"));

    // Come back to A, unblock it, and write again: the value stranded before
    // the detour is still pending and lands with this flush.
    std::env::set_var("KAGI_LOG_DIR", &a);
    std::fs::remove_dir(&a_file).expect("unblock A");
    write_setting("recovered", Some("yes"));
}

#[test]
fn main_window_frame_preserves_unknown_preferences_and_retires_size() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("settings.json"),
        r#"{"window_size":"640x480","future_only_key":"keepme"}"#,
    )
    .expect("seed");
    run_child("main_window_frame_child", tmp.path());
    let saved: serde_json::Value = serde_json::from_str(&settings_text(tmp.path())).expect("json");
    assert_eq!(saved["window_frame"], "-120,240,640,480,display-id");
    assert_eq!(saved["future_only_key"], "keepme");
    assert!(saved.get("window_size").is_none());
}

#[test]
#[ignore = "child process: needs a private KAGI_LOG_DIR"]
fn main_window_frame_child() {
    use crate::settings::{flush, main_window_frame, write_main_window_frame, MainWindowFrame};
    let frame = MainWindowFrame {
        x: -120.,
        y: 240.,
        width: 640.,
        height: 480.,
        display: "display-id".into(),
    };
    write_main_window_frame(&frame);
    flush();
    assert_eq!(main_window_frame(), Some(frame));
}
