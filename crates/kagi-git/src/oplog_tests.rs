use super::*;

#[test]
fn test_runtime_without_log_dir_refuses_home_fallback() {
    let error = log_file_path_from_env(None, Some(Path::new("/home/tester")), true)
        .expect_err("test runtime must not fall back to HOME");
    assert!(matches!(error, GitError::Other(message) if message == "tests must set KAGI_LOG_DIR"));
}

#[test]
fn log_dir_override_selects_that_directory() {
    let path = log_file_path_from_env(
        Some(std::ffi::OsStr::new("/tmp/kagi-logs")),
        Some(Path::new("/home/tester")),
        true,
    )
    .expect("KAGI_LOG_DIR must be accepted")
    .expect("KAGI_LOG_DIR produces a path");
    assert_eq!(path, Path::new("/tmp/kagi-logs/operations.jsonl"));
}

// ── append_oplog (integration-style, uses tempdir) ────────
//
// These tests manipulate the KAGI_LOG_DIR environment variable, which is
// process-global.  We serialise them with a mutex so parallel test threads
// do not interfere with each other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ── #421: repo confinement for read_oplog_tail_for_repo ─────

#[test]
fn oplog_filter_scopes_to_bound_repo() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // The global oplog holds entries for many repos. A server/CLI bound to repo
    // A must only ever see A's entries, never repo B's — and the filter must
    // survive path-shape differences (trailing slash, `.`, symlinked $TMPDIR).
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", log_dir.to_str().unwrap());

    // Two real repositories so path normalization (discover→workdir→canonicalize)
    // actually resolves.
    let repo_a = dir.path().join("A");
    let repo_b = dir.path().join("B");
    git2::Repository::init(&repo_a).unwrap();
    git2::Repository::init(&repo_b).unwrap();

    let mk = |repo: &std::path::Path, op: &str| OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        failure_code: None,
        ref_moves: None,
        repo_identity: None,
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 1,
        op: op.to_string(),
        repo: repo.display().to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "clean".to_string(),
        },
        outcome: OpOutcome::Success {
            after: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
        },
    };
    append_oplog(&mk(&repo_a, "checkout-a")).expect("write a1");
    append_oplog(&mk(&repo_b, "checkout-b")).expect("write b1");
    append_oplog(&mk(&repo_a, "branch-a")).expect("write a2");

    // Query A via a messy `A/.` path to exercise normalization.
    let messy_a = repo_a.join(".");
    let tail = read_oplog_tail_for_repo(&messy_a, 50);

    // A's own entries ARE returned (guards against a filter that drops all).
    assert!(
        tail.iter().any(|e| e.op == "checkout-a") && tail.iter().any(|e| e.op == "branch-a"),
        "bound repo A's entries must be returned: {:?}",
        tail.iter().map(|e| &e.op).collect::<Vec<_>>()
    );
    // B never leaks — by op label and by normalized repo path.
    let want_a = normalize_repo_path(&repo_a);
    assert!(
        tail.iter()
            .all(|e| normalize_repo_path(std::path::Path::new(&e.repo)) == want_a),
        "every returned entry must belong to repo A"
    );
    assert!(
        !tail.iter().any(|e| e.op == "checkout-b"),
        "repo B's entry must never appear in A's oplog"
    );

    // The unfiltered global read still sees both (GUI behaviour unchanged).
    let global = read_oplog_tail(50);
    assert!(global.iter().any(|e| e.op == "checkout-b"));

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

#[test]
fn append_waiting_for_retirement_cannot_publish_a_deleted_root() {
    // Worker/absorb unit tests also append while ENV_LOCK is held. Isolate the
    // process-wide log environment so only this deliberate writer race exists.
    if std::env::var_os("KAGI_RETENTION_RACE_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "oplog::tests::append_waiting_for_retirement_cannot_publish_a_deleted_root",
                "--nocapture",
            ])
            .env("KAGI_RETENTION_RACE_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let repo_path = root.path().join("repo");
    let repo = git2::Repository::init_bare(&repo_path).unwrap();
    let log_dir = root.path().join("log");
    let previous = std::env::var_os("KAGI_LOG_DIR");
    std::env::set_var("KAGI_LOG_DIR", &log_dir);
    let oid = repo.blob(b"shared recovery").unwrap();
    let name = "refs/kagi/backups/race/0";
    repo.reference(name, oid, false, "fixture backup").unwrap();
    let state = StateSummary {
        head: "fixture".into(),
        dirty: "clean".into(),
    };
    let mut entry = OpLogEntry::new(
        "discard",
        repo_path.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    );
    entry.backup_refs.push(name.into());
    let (_, entry) = append_oplog_receipt(&entry).unwrap();
    let plan = retention::plan(&repo, &entry).unwrap();
    let mut lock = retention::lock(&log_file_path().unwrap().unwrap()).unwrap();
    let (started, ready) = std::sync::mpsc::channel();
    let (finished, completion) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            started.send(()).unwrap();
            finished.send(append_oplog_receipt(&entry)).unwrap();
        });
        ready.recv().unwrap();
        // The root exists while append is queued, then retirement removes it
        // before append acquires the lock. Validation before locking is unsafe.
        assert!(completion
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        let mut retired = false;
        retention::execute_locked(&repo, &plan, &mut retired, &mut lock).unwrap();
        assert!(retired);
        drop(lock);
        let result = completion.recv().unwrap();
        assert!(
            result.is_err(),
            "queued append must not advertise a retired root"
        );
    });
    assert!(read_oplog_tail(10).is_empty());
    assert!(repo.find_reference(name).is_err());
    match previous {
        Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #499: bounded tail reads ────────────────────────────────

fn synthetic_entry(id: u64, repo: &str) -> OpLogEntry {
    let state = StateSummary {
        head: "branch: main".to_string(),
        dirty: "clean".to_string(),
    };
    OpLogEntry {
        id,
        parent: id.checked_sub(1),
        timestamp: 1_700_000_000 + id as i64,
        op: format!("op-{id}"),
        repo: repo.to_string(),
        actor: Actor::Human,
        worktree: None,
        before: state.clone(),
        outcome: OpOutcome::Success { after: state },
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        failure_code: None,
        ref_moves: None,
        repo_identity: None,
    }
}

/// A log of `count` explicit-id lines, chained exactly as `append_oplog`
/// writes them. `repos` is cycled so a filtered read has non-matching lines
/// to skip.
fn synthetic_log(dir: &Path, count: u64, repos: &[&str]) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("operations.jsonl");
    let mut content = String::new();
    for id in 0..count {
        content.push_str(&entry_to_json(&synthetic_entry(
            id,
            repos[id as usize % repos.len()],
        )));
        content.push('\n');
    }
    std::fs::write(&path, &content).unwrap();
    path
}

#[test]
fn tail_read_bytes_do_not_grow_with_history() {
    let dir = tempfile::tempdir().unwrap();
    let small = synthetic_log(&dir.path().join("s"), 500, &["/tmp/r"]);
    let large = synthetic_log(&dir.path().join("l"), 20_000, &["/tmp/r"]);

    let small_tail = super::tail::read_path(&small, 3, &|_| true);
    let large_tail = super::tail::read_path(&large, 3, &|_| true);

    assert_eq!(
        small_tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![499, 498, 497],
        "newest first"
    );
    assert_eq!(
        large_tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![19_999, 19_998, 19_997]
    );
    assert_eq!(large_tail.entries[0].parent, Some(19_998));
    // The measurement #499 asks for: a 40x longer history costs the same read.
    assert_eq!(small_tail.bytes, large_tail.bytes);
    let length = std::fs::metadata(&large).unwrap().len();
    assert!(
        large_tail.bytes * 10 < length,
        "read {} of {} bytes",
        large_tail.bytes,
        length
    );
}

#[test]
fn tail_read_spans_several_chunks_without_losing_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = synthetic_log(dir.path(), 400, &["/tmp/r"]);
    assert!(
        std::fs::metadata(&path).unwrap().len() > 3 * 8 * 1024,
        "fixture must cross the chunk boundary"
    );

    let tail = super::tail::read_path(&path, 300, &|_| true);

    assert_eq!(
        tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        (100..400).rev().collect::<Vec<_>>()
    );
}

#[test]
fn filtered_tail_read_returns_the_newest_matching_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = synthetic_log(dir.path(), 60, &["/tmp/a", "/tmp/b"]);

    let tail = super::tail::read_path(&path, 2, &|entry| entry.repo == "/tmp/b");

    assert_eq!(
        tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![59, 57]
    );
}

#[test]
fn a_legacy_line_in_the_window_falls_back_to_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("operations.jsonl");
    let legacy: Vec<String> = (0..3)
        .map(|index| {
            let mut value: serde_json::Value =
                serde_json::from_str(&entry_to_json(&synthetic_entry(index, "/tmp/r"))).unwrap();
            let object = value.as_object_mut().unwrap();
            object.remove("id");
            object.remove("parent");
            value.to_string()
        })
        .collect();
    std::fs::write(&path, format!("{}\n", legacy.join("\n"))).unwrap();

    let tail = super::tail::read_path(&path, 1, &|_| true);

    // Identity still comes from the position in the file (ADR-0149 back-compat),
    // which only the whole-file read knows — so it read the whole file.
    assert_eq!(tail.entries.len(), 1);
    assert_eq!(tail.entries[0].id, 2);
    assert_eq!(tail.entries[0].parent, Some(1));
    assert_eq!(tail.bytes, std::fs::metadata(&path).unwrap().len());
}

#[test]
fn an_unterminated_final_line_leaves_earlier_entries_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = synthetic_log(dir.path(), 3, &["/tmp/r"]);
    let mut content = std::fs::read_to_string(&path).unwrap();
    // A writer killed mid-append: a partial record with no terminator.
    content.push_str("{\"id\":3,\"parent\":2,\"timestamp\":17000");
    std::fs::write(&path, &content).unwrap();

    let tail = super::tail::read_path(&path, 3, &|_| true);

    assert_eq!(
        tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![2, 1, 0],
        "the fragment is skipped, complete entries survive"
    );
}

/// Deliberate narrowing (#499): `read_to_string` rejects a file for one stray
/// byte, and a blank read made the locked append restart the chain at id 0.
/// A bounded read never touches older history, so corruption behind the window
/// no longer hides readable recent entries.
#[test]
fn invalid_bytes_older_than_the_window_no_longer_blank_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = synthetic_log(dir.path(), 4, &["/tmp/r"]);
    let mut raw = std::fs::read(&path).unwrap();
    let first = raw.iter().position(|byte| *byte == b'\n').unwrap();
    raw[first / 2] = 0xff;
    std::fs::write(&path, &raw).unwrap();
    assert!(std::fs::read_to_string(&path).is_err(), "file is not UTF-8");

    let tail = super::tail::read_path(&path, 2, &|_| true);

    assert_eq!(
        tail.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![3, 2]
    );
}

#[test]
fn invalid_bytes_inside_the_window_keep_the_all_or_nothing_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = synthetic_log(dir.path(), 4, &["/tmp/r"]);
    let mut raw = std::fs::read(&path).unwrap();
    let last = raw.len() - 2;
    raw[last] = 0xff;
    std::fs::write(&path, &raw).unwrap();

    let tail = super::tail::read_path(&path, 2, &|_| true);

    // The window itself cannot be interpreted, so the whole-file read decides —
    // and it rejects the file, exactly as every reader did before.
    assert!(tail.entries.is_empty());
    assert_eq!(tail.bytes, 0);
}

/// #499 x #500: the bounded read must not be a second, poorer interpretation of
/// a line. Typed recovery handles come back exactly as written, and an unknown
/// sibling field on the same line does not make the entry — or its handles —
/// disappear. (`OpLogEntry` has no storage for an unknown field, so nothing
/// here claims one round-trips.)
#[test]
fn tail_read_keeps_typed_recovery_beside_an_unknown_field() {
    let dir = tempfile::tempdir().unwrap();
    // Past one chunk, so the read really is a window and not the whole file.
    let path = synthetic_log(dir.path(), 200, &["/tmp/r"]);
    let mut newest = synthetic_entry(200, "/tmp/r");
    newest.recovery = vec![
        RecoveryHandle::oid(recovery::SAVEPOINT, "a".repeat(40)),
        RecoveryHandle::file("dir=x, y/ファイル.txt", "b".repeat(40), None)
            .with_reference("refs/kagi/backups/attempt/1"),
    ];
    let mut line: serde_json::Value = serde_json::from_str(&entry_to_json(&newest)).unwrap();
    line["future_field"] = serde_json::json!({ "keep": true });
    let mut content = std::fs::read_to_string(&path).unwrap();
    content.push_str(&format!("{line}\n"));
    std::fs::write(&path, &content).unwrap();

    let tail = super::tail::read_path(&path, 1, &|_| true);

    assert_eq!(tail.entries.len(), 1);
    assert_eq!(tail.entries[0].recovery, newest.recovery);
    assert!(tail.bytes < content.len() as u64, "still a bounded read");
}

// ── recovery handles (#500) ───────────────────────────────

/// A path containing `,`, `=` and non-ASCII is exactly what the comma-joined
/// `path=blob` summary could not express unambiguously. As JSON string values
/// it round-trips byte-for-byte, and the display summary is untouched.
#[test]
fn recovery_handles_round_trip_with_awkward_paths() {
    let handles = vec![
        RecoveryHandle::oid(recovery::SAVEPOINT, "a".repeat(40)),
        RecoveryHandle::file("dir=x, y/ファイル.txt", "b".repeat(40), None),
        RecoveryHandle::file("plain.txt", "c".repeat(40), None)
            .with_reference("refs/kagi/backups/attempt/1"),
    ];
    let summary = "discarded 1 file(s); backup: dir=x, y/ファイル.txt=bbb";
    let mut entry = OpLogEntry::new(
        "discard",
        "/tmp/repo",
        StateSummary {
            head: "branch: main".to_string(),
            dirty: "1 modified".to_string(),
        },
        OpOutcome::Success {
            after: StateSummary {
                head: "branch: main".to_string(),
                // The human-readable summary the UI shows stays as it was.
                dirty: summary.to_string(),
            },
        },
    );
    entry.recovery = handles.clone();

    let json = entry_to_json(&entry);
    let parsed = parse_oplog_line(&json).expect("a written line must parse");
    assert_eq!(parsed.recovery, handles);
    let OpOutcome::Success { after } = &parsed.outcome else {
        panic!("outcome kind changed")
    };
    assert_eq!(
        after.dirty, summary,
        "the displayed summary must survive unchanged"
    );
}

/// A pre-#500 line carries the prose only. It must still read — and must NOT be
/// mined for handles: no typed data means "not known to be recoverable".
#[test]
fn legacy_line_without_recovery_reads_as_no_typed_data() {
    let legacy = concat!(
        r#"{"timestamp":1000,"op":"restore-snapshot","repo":"/tmp/repo","#,
        r#""before":{"head":"branch: main","dirty":"clean"},"#,
        r#""outcome":{"kind":"Success","after":{"head":"branch: main","#,
        r#""dirty":"savepoint 1111111111111111111111111111111111111111"}}}"#,
    );
    let entry = parse_oplog_line(legacy).expect("legacy lines must still parse");
    assert_eq!(entry.op, "restore-snapshot");
    assert!(entry.backup_refs.is_empty());
    assert!(
        entry.recovery.is_empty(),
        "prose must never be promoted to a typed recovery claim"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("legacy outcome must still read")
    };
    assert!(after.dirty.starts_with("savepoint "), "prose is preserved");
}

/// A `recovery` value written by some other tool (wrong type, missing `oid`)
/// reads as "no typed data" rather than dropping the whole entry.
#[test]
fn malformed_recovery_degrades_to_empty_without_losing_the_entry() {
    for value in [r#""not-an-array""#, r#"[{"kind":"savepoint"}]"#, "[42]"] {
        let line = format!(
            concat!(
                r#"{{"timestamp":1,"op":"discard","repo":"/tmp/repo","#,
                r#""before":{{"head":"h","dirty":"d"}},"#,
                r#""outcome":{{"kind":"Failed","error":"e"}},"recovery":{}}}"#,
            ),
            value
        );
        let entry = parse_oplog_line(&line).unwrap_or_else(|| panic!("must parse: {line}"));
        assert!(entry.recovery.is_empty(), "{value}");
    }
}

/// #894: the recorded repository round-trips; a line written before the
/// field, or a malformed value, reads as "unknown" (`None`) and the entry
/// itself is kept.
#[test]
fn repo_identity_is_additive_and_lenient() {
    let base = concat!(
        r#"{"timestamp":1000,"op":"checkout","repo":"/tmp/repo","#,
        r#""before":{"head":"branch: main","dirty":"clean"},"#,
        r#""outcome":{"kind":"Success","after":{"head":"branch: f","dirty":"clean"}}"#,
    );
    let legacy = parse_oplog_line(&format!("{base}}}")).expect("legacy line");
    assert_eq!(legacy.repo_identity, None);

    let mut entry = legacy.clone();
    entry.repo_identity = Some(RepoIdentity {
        common_dir: "/tmp/repo/.git".into(),
        file_id: Some((16_777_232, 4_242)),
        created: Some((1_700_000_000, 123_456_789)),
    });
    let back = parse_oplog_line(&entry_to_json(&entry)).unwrap();
    assert_eq!(back.repo_identity, entry.repo_identity);

    for bad in [r#""a string""#, r#"{"dev":1}"#, "42"] {
        let line = format!(r#"{base},"repo_identity":{bad}}}"#);
        let read = parse_oplog_line(&line).unwrap_or_else(|| panic!("row dropped: {bad}"));
        assert_eq!(read.repo_identity, None, "{bad}");
    }
}

/// #900 review: deciding "same repository". The case that matters most: a
/// repository deleted and re-cloned at the same place from the same remote,
/// whose new `.git` got the old inode — same path, same `(dev, ino)`, and its
/// `main` may even sit at the same OID. Only the creation time tells them
/// apart, so it must be required, and its absence must not read as "same".
#[test]
fn same_repository_needs_the_file_and_its_birth_to_match() {
    use SameRepository::{Ambiguous, Different, Same};
    let id = |dir: &str, file: Option<(u64, u64)>, born: Option<(u64, u32)>| RepoIdentity {
        common_dir: dir.into(),
        file_id: file,
        created: born,
    };
    let original = id("/r/.git", Some((1, 42)), Some((100, 5)));

    let recloned = id("/r/.git", Some((1, 42)), Some((200, 7)));
    assert_eq!(
        original.same_repository(&recloned),
        Different,
        "reused inode"
    );
    assert_eq!(original.same_repository(&original.clone()), Same);
    let moved = id("/elsewhere/.git", Some((1, 42)), Some((100, 5)));
    assert_eq!(original.same_repository(&moved), Same, "renamed, same file");
    assert_eq!(
        original.same_repository(&id("/r/.git", Some((1, 43)), Some((100, 5)))),
        Different
    );
    assert_eq!(
        original.same_repository(&id("/r/.git", Some((1, 42)), None)),
        Ambiguous,
        "a file id match alone could be a reused inode"
    );
    // A whole-second creation time (HFS+, many network filesystems) cannot
    // tell a delete and re-create within that second from the original.
    let coarse = id("/r/.git", Some((1, 42)), Some((100, 0)));
    assert_eq!(
        coarse.same_repository(&coarse.clone()),
        Ambiguous,
        "coarse birth"
    );
    assert_eq!(
        coarse.same_repository(&id("/r/.git", Some((1, 42)), Some((101, 0)))),
        Different,
        "a coarse time still proves a difference"
    );
    // No file id (non-unix): path + birth.
    let windows = id("C:/r/.git", None, Some((100, 5)));
    assert_eq!(windows.same_repository(&windows.clone()), Same);
    assert_eq!(
        windows.same_repository(&id("C:/r/.git", None, Some((9, 9)))),
        Different
    );
    assert_eq!(
        windows.same_repository(&id("C:/r/.git", None, None)),
        Ambiguous
    );
    assert_eq!(
        windows.same_repository(&id("C:/x/.git", None, None)),
        Different
    );
}

/// #891 review: an operation whose termination is unconfirmed may still be
/// moving refs after the observation, so its moves are not a record.
#[test]
fn an_unconfirmed_termination_records_no_ref_moves() {
    let state = || crate::ops::StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let observed = Some(Vec::new());
    let unknown = OpLogEntry::new(
        "cherry-pick-continue",
        "/r",
        state(),
        OpOutcome::Unknown {
            after: state(),
            evidence: "process termination is unconfirmed".into(),
        },
    )
    .with_ref_moves(observed.clone());
    assert_eq!(unknown.ref_moves, None, "fail closed: not recorded");

    let refused = OpLogEntry::new(
        "cherry-pick-continue",
        "/r",
        state(),
        OpOutcome::Refused {
            blockers: vec!["unresolved".into()],
        },
    )
    .with_ref_moves(observed.clone());
    assert_eq!(
        refused.ref_moves, observed,
        "a settled outcome keeps its record"
    );
}

/// #334 slice 2a: `ref_moves` is additive. A line written before the field is
/// "not recorded" (`None`), never "nothing moved"; "nothing moved" survives a
/// round trip as itself; a malformed list is "not recorded" and the entry
/// still reads.
#[test]
fn ref_moves_distinguish_not_recorded_from_nothing_moved() {
    let base = concat!(
        r#"{"timestamp":1000,"op":"checkout","repo":"/tmp/repo","#,
        r#""before":{"head":"branch: main","dirty":"clean"},"#,
        r#""outcome":{"kind":"Success","after":{"head":"branch: f","dirty":"clean"}}"#,
    );
    let legacy = parse_oplog_line(&format!("{base}}}")).expect("legacy line");
    assert_eq!(legacy.ref_moves, None, "absent = not recorded");

    let mut entry = legacy.clone();
    entry.ref_moves = Some(Vec::new());
    let empty = parse_oplog_line(&entry_to_json(&entry)).unwrap();
    assert_eq!(empty.ref_moves, Some(Vec::new()), "recorded, nothing moved");

    let head = kagi_domain::ref_moves::RefMove {
        refname: "HEAD".into(),
        old: Some("a".repeat(40)),
        new: Some("a".repeat(40)),
        old_symbolic: Some("refs/heads/main".into()),
        new_symbolic: Some("refs/heads/f".into()),
    };
    let created = kagi_domain::ref_moves::RefMove {
        refname: "refs/heads/f".into(),
        old: None,
        new: Some("b".repeat(40)),
        old_symbolic: None,
        new_symbolic: None,
    };
    entry.ref_moves = Some(vec![head, created]);
    let full = parse_oplog_line(&entry_to_json(&entry)).unwrap();
    assert_eq!(full.ref_moves, entry.ref_moves);

    for value in ["null", r#""HEAD""#, r#"[{"old":"a"}]"#] {
        let line = format!(r#"{base},"ref_moves":{value}}}"#);
        let entry = parse_oplog_line(&line).unwrap_or_else(|| panic!("must parse: {line}"));
        assert_eq!(entry.ref_moves, None, "{value}");
    }
}
