// Real-file settings store tests: rescue, atomic writes, and scale migration.
use super::*;
use crate::settings::Settings;

fn store(path: &Path) -> Store {
    load_doc(path)
}

fn set(s: &mut Store, key: &str, value: &str) {
    s.doc.insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
    s.dirty = true;
    s.pending.insert(key.to_string());
}

fn on_disk(path: &Path, key: &str) -> Option<String> {
    load_doc(path).doc.get(key).and_then(scalar_to_string)
}

/// Every value on disk must still be a JSON **string** (ADR-0091/0092).
fn assert_flat_strings(text: &str) {
    let v: serde_json::Value = serde_json::from_str(text).expect("on-disk settings must parse");
    for (k, v) in v.as_object().expect("flat object") {
        assert!(v.is_string(), "{k} must be stored as a string, got {v}");
    }
}

#[test]
fn first_launch_records_only_the_scale_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let mut s = store(&path);
    assert!(!path.exists());

    assert_eq!(migrate_ui_scale_base_store(&mut s), None);
    let saved = load_doc(&path).doc;
    assert_eq!(saved.len(), 1);
    assert_eq!(on_disk(&path, "ui_scale_base").as_deref(), Some("v2"));
    assert!(!saved.contains_key("ui_zoom"));

    let first_flush = s.last_flush;
    let first_bytes = std::fs::read(&path).expect("persisted");
    assert_eq!(migrate_ui_scale_base_store(&mut s), None);
    assert_eq!(s.last_flush, first_flush, "repeat must not flush");
    assert_eq!(std::fs::read(&path).expect("persisted"), first_bytes);
}

#[test]
fn scale_migration_preserves_keys_and_runs_only_once() {
    for (old, expected) in [
        ("900", "1000"),
        ("1000", "1111"),
        ("1001", "1112"),
        ("4294967295", "4294967295"),
    ] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        std::fs::write(
                &path,
                format!(
                    r#"{{"ui_zoom":"{old}","session_repos":"/a","future_only_key":"keepme","ui_scale_base":"v1"}}"#
                ),
            )
            .expect("seed");
        let mut s = store(&path);

        let zoom = expected.parse().expect("expected u32");
        assert_eq!(migrate_ui_scale_base_store(&mut s), Some(zoom));
        let saved = load_doc(&path).doc;
        assert_eq!(saved.len(), 4, "all existing keys survive");
        assert_eq!(on_disk(&path, "ui_scale_base").as_deref(), Some("v2"));
        assert_eq!(on_disk(&path, "ui_zoom").as_deref(), Some(expected));
        assert_eq!(on_disk(&path, "session_repos").as_deref(), Some("/a"));
        assert_eq!(on_disk(&path, "future_only_key").as_deref(), Some("keepme"));
        assert_flat_strings(&std::fs::read_to_string(&path).expect("persisted"));

        let first_flush = s.last_flush;
        let first_bytes = std::fs::read(&path).expect("persisted");
        assert_eq!(migrate_ui_scale_base_store(&mut s), Some(zoom));
        assert_eq!(s.last_flush, first_flush, "repeat must not flush");
        assert_eq!(std::fs::read(&path).expect("persisted"), first_bytes);
    }
}

#[test]
fn invalid_zoom_is_not_rewritten_during_scale_migration() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"ui_zoom":"not a zoom","future_only_key":"keepme"}"#,
    )
    .expect("seed");
    let mut s = store(&path);
    assert_eq!(migrate_ui_scale_base_store(&mut s), None);
    assert_eq!(on_disk(&path, "ui_zoom").as_deref(), Some("not a zoom"));
    assert_eq!(on_disk(&path, "future_only_key").as_deref(), Some("keepme"));
    assert_eq!(on_disk(&path, "ui_scale_base").as_deref(), Some("v2"));
}

#[test]
fn corrupt_settings_are_rescued_before_scale_version_is_saved() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let original = r#"{"ui_zoom":"900","session_repos":"/a""#;
    std::fs::write(&path, original).expect("seed");
    let mut s = store(&path);
    assert!(s.corrupt);

    assert_eq!(migrate_ui_scale_base_store(&mut s), None);
    assert_eq!(
        std::fs::read_to_string(path.with_extension("json.corrupt")).expect("rescue"),
        original
    );
    let saved = load_doc(&path).doc;
    assert_eq!(saved.len(), 1);
    assert_eq!(on_disk(&path, "ui_scale_base").as_deref(), Some("v2"));
    assert!(!saved.contains_key("ui_zoom"));
}

#[test]
fn save_and_reload_round_trips_session_and_unknown_keys() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");

    let mut s = store(&path);
    assert!(!s.corrupt, "a missing file is not corrupt");
    set(&mut s, "session_repos", "/a\u{1f}/b");
    set(&mut s, "session_active", "1");
    set(&mut s, "theme", "one-dark");
    set(&mut s, "future_only_key", "keepme");
    assert!(flush_store(&mut s));

    assert_flat_strings(&std::fs::read_to_string(&path).expect("written"));

    // A fresh read of the real file sees every key, known and unknown.
    let back = Settings {
        raw: load_doc(&path).doc,
    };
    assert_eq!(back.get_str("session_repos").as_deref(), Some("/a\u{1f}/b"));
    assert_eq!(back.get_str("session_active").as_deref(), Some("1"));
    assert_eq!(back.theme().as_deref(), Some("one-dark"));
    assert_eq!(back.get_str("future_only_key").as_deref(), Some("keepme"));
}

#[test]
fn corrupt_file_is_kept_aside_not_overwritten() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    // Truncated JSON — the exact shape a half-written file used to take.
    let original = "{\n  \"session_repos\": \"/a\u{1f}/b\",\n  \"theme\": \"one-dark\"";
    std::fs::write(&path, original).expect("seed");

    let mut s = store(&path);
    assert!(s.corrupt, "unparsable settings must be flagged");
    assert!(s.doc.is_empty(), "a corrupt file reads as empty");

    // One ordinary setting change used to overwrite the file with the empty
    // default, taking session_repos (the restored tab set) with it.
    set(&mut s, "theme", "gruvbox");
    assert!(flush_store(&mut s));

    assert_eq!(
        std::fs::read_to_string(path.with_extension("json.corrupt")).expect("rescued"),
        original,
        "the corrupt original must be kept byte-for-byte"
    );
    assert!(!load_doc(&path).corrupt);
    assert_eq!(on_disk(&path, "theme").as_deref(), Some("gruvbox"));
}

/// #617 review 1: a fixed `.corrupt` name would let a second corruption
/// replace the only surviving copy of the first one's session data.
#[test]
fn second_corruption_does_not_overwrite_the_first_rescue() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");

    let first = "{ \"session_repos\": \"/first\"";
    std::fs::write(&path, first).expect("seed");
    let mut s = store(&path);
    set(&mut s, "theme", "a");
    assert!(flush_store(&mut s));

    // The freshly written file is corrupted again by something else.
    let second = "{ \"session_repos\": \"/second\"";
    std::fs::write(&path, second).expect("re-corrupt");
    let mut s = store(&path);
    set(&mut s, "theme", "b");
    assert!(flush_store(&mut s));

    assert_eq!(
        std::fs::read_to_string(path.with_extension("json.corrupt")).expect("first rescue"),
        first,
        "the first rescue must survive a second corruption"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("settings.json.corrupt.1")).expect("second rescue"),
        second
    );
    assert_eq!(on_disk(&path, "theme").as_deref(), Some("b"));
}

/// Two rescues racing for a name must not collide: the reservation is
/// exclusive, so each caller gets its own destination.
#[test]
fn rescue_reserves_a_name_exclusively() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut taken = Vec::new();
    for i in 0..3 {
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, format!("garbage {i}")).expect("seed");
        let aside = rescue(&path, "settings").expect("rescue");
        assert!(!taken.contains(&aside), "{aside:?} handed out twice");
        assert_eq!(
            std::fs::read_to_string(&aside).expect("rescued"),
            format!("garbage {i}")
        );
        taken.push(aside);
    }
}

/// When the original cannot be moved, the write is refused outright rather
/// than replacing it. Here the rescue name is occupied by a non-empty
/// *directory*, which `create_new` rejects and `rename` cannot replace.
#[test]
fn rescue_failure_refuses_the_write_and_keeps_the_original() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let original = "{ \"session_repos\": \"/keepme\"";
    std::fs::write(&path, original).expect("seed");
    // Block every candidate name with directories.
    std::fs::create_dir(tmp.path().join("settings.json.corrupt")).expect("mkdir");
    for n in 1..100 {
        std::fs::create_dir(tmp.path().join(format!("settings.json.corrupt.{n}"))).expect("mkdir");
    }

    let mut s = store(&path);
    set(&mut s, "theme", "gruvbox");
    assert!(!flush_store(&mut s), "a failed rescue must report failure");
    assert!(s.dirty, "the change stays pending");
    assert_eq!(
        std::fs::read_to_string(&path).expect("original"),
        original,
        "the original must be untouched when it cannot be rescued"
    );
}

/// #617 review 2: the store is dirty (a burst is pending) when something
/// else corrupts the file. The flush must still rescue it — "the original
/// is always preserved" has no dirty-path exception.
#[test]
fn external_corruption_during_a_burst_is_still_rescued() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, "{ \"theme\": \"one-dark\" }\n").expect("seed");

    let mut s = store(&path);
    assert!(!s.corrupt);
    set(&mut s, "graph_col_w", "120"); // pending, not yet flushed

    let garbage = "{ \"session_repos\": \"/gone\"";
    std::fs::write(&path, garbage).expect("corrupt behind our back");

    assert!(flush_store(&mut s));
    assert_eq!(
        std::fs::read_to_string(path.with_extension("json.corrupt")).expect("rescued"),
        garbage,
        "a file corrupted after load must still be rescued"
    );
    assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
}

/// …and a *valid* external edit during a burst is merged, not clobbered:
/// only the keys this process wrote are replayed on top of theirs.
#[test]
fn external_edit_during_a_burst_is_merged_not_clobbered() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, "{ \"theme\": \"one-dark\" }\n").expect("seed");

    let mut s = store(&path);
    set(&mut s, "graph_col_w", "120"); // ours, pending

    // Another writer replaces the file, changing a key we never touched and
    // adding one this build does not know.
    std::fs::write(
        &path,
        "{ \"theme\": \"gruvbox\", \"future_only_key\": \"keepme\" }\n",
    )
    .expect("external write");

    assert!(flush_store(&mut s));
    assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
    assert_eq!(
        on_disk(&path, "theme").as_deref(),
        Some("gruvbox"),
        "a key we did not write must keep the other writer's value"
    );
    assert_eq!(on_disk(&path, "future_only_key").as_deref(), Some("keepme"));
}

/// #617 review 3: `(mtime, len)` cannot see a same-size in-place edit with
/// the timestamp pinned. The write path therefore compares bytes, so such
/// an edit is still merged instead of overwritten.
#[test]
fn same_stamp_external_edit_is_detected_by_content() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, "{ \"theme\": \"aaaaaaaa\" }\n").expect("seed");

    let s_before = store(&path);
    let stamp_before = s_before.stamp;
    let mut s = s_before;
    set(&mut s, "graph_col_w", "120");

    // Same length, and the timestamps restored to exactly what we read.
    let times = std::fs::File::open(&path)
        .and_then(|f| f.metadata())
        .map(|m| {
            std::fs::FileTimes::new()
                .set_accessed(m.accessed().unwrap_or_else(|_| SystemTime::now()))
                .set_modified(m.modified().expect("mtime"))
        })
        .expect("times");
    std::fs::write(&path, "{ \"theme\": \"bbbbbbbb\" }\n").expect("in-place edit");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|f| f.set_times(times))
        .expect("pin mtime");
    assert_eq!(
        file_stamp(&path),
        stamp_before,
        "the edit must be invisible to stat for this test to mean anything"
    );

    assert!(flush_store(&mut s));
    assert_eq!(
        on_disk(&path, "theme").as_deref(),
        Some("bbbbbbbb"),
        "a stat-invisible edit must still be merged, not overwritten"
    );
    assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
}

#[test]
fn write_atomic_replaces_via_temp_and_leaves_none_behind() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, "{\"theme\":\"old\"}\n").expect("seed");

    write_atomic(&path, b"{\"theme\":\"new\"}\n").expect("atomic write");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "{\"theme\":\"new\"}\n"
    );
    assert_eq!(
        std::fs::read_dir(tmp.path()).expect("read_dir").count(),
        1,
        "the temp file must be renamed away, not left next to settings.json"
    );

    // A failing replace (the target is a directory) leaves the previous
    // content readable and no partial temp file behind.
    let blocked = tmp.path().join("blocked");
    std::fs::create_dir(&blocked).expect("mkdir");
    let occupied = blocked.join("settings.json");
    std::fs::create_dir(&occupied).expect("mkdir target");
    assert!(write_atomic(&occupied, b"nope").is_err());
    assert_eq!(
        std::fs::read_dir(&blocked).expect("read_dir").count(),
        1,
        "a failed atomic write must clean up its temp file"
    );
}

/// #617 Codex review: the atomic replace must carry the existing file's
/// mode over, or a settings file the user tightened silently comes back at
/// the default umask after any setting change.
#[cfg(unix)]
#[test]
fn a_tightened_file_keeps_its_mode_across_writes() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let mut s = store(&path);

    // A fresh file keeps whatever the platform default is — the same mode
    // an ordinary create in this directory would produce.
    set(&mut s, "theme", "one-dark");
    assert!(flush_store(&mut s));
    let reference = tmp.path().join("reference");
    std::fs::write(&reference, "x").expect("reference file");
    let mode_of = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(
        mode_of(&path),
        mode_of(&reference),
        "a new settings file must not invent a mode of its own"
    );

    // Now the user tightens it. Every later write must preserve that.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    set(&mut s, "theme", "gruvbox");
    assert!(flush_store(&mut s));
    assert_eq!(
        mode_of(&path),
        0o600,
        "chmod 600 must survive a settings change"
    );
    assert_eq!(on_disk(&path, "theme").as_deref(), Some("gruvbox"));

    // …and the rescue of a corrupt file keeps the original's mode too: the
    // reserved file is replaced by renaming the original onto it.
    std::fs::write(&path, "{ \"session_repos\": \"/keepme\"").expect("corrupt");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let mut s = store(&path);
    set(&mut s, "theme", "nord");
    assert!(flush_store(&mut s));
    assert_eq!(
        mode_of(&path.with_extension("json.corrupt")),
        0o600,
        "the rescued original must keep its own mode"
    );
}

/// #617 review 4: only a *repeated* key coalesces. Two different keys in a
/// row are two user actions and must both be on disk immediately.
#[test]
fn distinct_keys_are_never_deferred() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let mut s = store(&path);

    s.doc.insert("session_repos".into(), "/a".into());
    schedule_flush(&mut s, "session_repos");
    s.doc.insert("session_active".into(), "1".into());
    schedule_flush(&mut s, "session_active");

    assert!(!s.dirty, "a session save must not sit in memory");
    assert_eq!(on_disk(&path, "session_repos").as_deref(), Some("/a"));
    assert_eq!(on_disk(&path, "session_active").as_deref(), Some("1"));
}

#[test]
fn burst_of_writes_collapses_to_one_file_write() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("settings.json");
    let mut s = store(&path);

    // The first write of a burst lands synchronously (durability unchanged).
    s.doc.insert("graph_col_w".into(), "0".into());
    schedule_flush(&mut s, "graph_col_w");
    assert!(path.exists(), "the first write is not deferred");

    // The rest of the drag only updates memory. `trailing` is pre-armed so
    // this asserts the coalescing policy without spawning a thread.
    s.trailing = true;
    for w in 1..=200 {
        s.doc.insert("graph_col_w".into(), w.to_string().into());
        schedule_flush(&mut s, "graph_col_w");
    }
    assert!(s.dirty, "the burst is still pending in memory");
    assert_eq!(
        on_disk(&path, "graph_col_w").as_deref(),
        Some("0"),
        "200 drag events must not each hit the disk"
    );

    // …and the trailing flush writes the final value, once.
    assert!(flush_store(&mut s));
    assert_flat_strings(&std::fs::read_to_string(&path).expect("read"));
    assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("200"));
}
