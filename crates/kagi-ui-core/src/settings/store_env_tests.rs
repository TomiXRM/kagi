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
