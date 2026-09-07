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

// ── escape_json_string ────────────────────────────────────

#[test]
fn escape_plain_string() {
    assert_eq!(escape_json_string("hello"), "\"hello\"");
}

#[test]
fn escape_double_quote() {
    assert_eq!(escape_json_string("say \"hi\""), "\"say \\\"hi\\\"\"");
}

#[test]
fn escape_backslash() {
    assert_eq!(escape_json_string("a\\b"), "\"a\\\\b\"");
}

#[test]
fn escape_newline() {
    assert_eq!(escape_json_string("a\nb"), "\"a\\nb\"");
}

#[test]
fn escape_carriage_return() {
    assert_eq!(escape_json_string("a\rb"), "\"a\\rb\"");
}

#[test]
fn escape_tab() {
    assert_eq!(escape_json_string("a\tb"), "\"a\\tb\"");
}

#[test]
fn escape_null_byte() {
    assert_eq!(escape_json_string("a\x00b"), "\"a\\u0000b\"");
}

#[test]
fn escape_all_specials_together() {
    // "a\b"<newline>  →  "\"a\\\\b\\\"\\n\""
    let input = "a\\b\"\n";
    let result = escape_json_string(input);
    assert_eq!(result, "\"a\\\\b\\\"\\n\"");
}

// ── entry_to_json ─────────────────────────────────────────

#[test]
fn json_success_entry_contains_required_fields() {
    let entry = OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 1_000_000,
        op: "checkout".to_string(),
        repo: "/tmp/repo".to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "clean".to_string(),
        },
        outcome: OpOutcome::Success {
            after: StateSummary {
                head: "branch: feature".to_string(),
                dirty: "clean".to_string(),
            },
        },
    };
    let json = entry_to_json(&entry);
    assert!(json.contains("\"timestamp\":1000000"), "timestamp missing");
    assert!(json.contains("\"op\":\"checkout\""), "op missing");
    assert!(json.contains("\"repo\":\"/tmp/repo\""), "repo missing");
    assert!(json.contains("\"kind\":\"Success\""), "kind missing");
    assert!(
        json.contains("\"head\":\"branch: main\""),
        "before.head missing"
    );
    assert!(
        json.contains("\"head\":\"branch: feature\""),
        "after.head missing"
    );
}

#[test]
fn json_refused_entry_contains_blockers() {
    let entry = OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 2_000_000,
        op: "checkout".to_string(),
        repo: "/tmp/repo".to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "1 modified".to_string(),
        },
        outcome: OpOutcome::Refused {
            blockers: vec![
                "Working tree has changes".to_string(),
                "Branch 'x' does not exist".to_string(),
            ],
        },
    };
    let json = entry_to_json(&entry);
    assert!(json.contains("\"kind\":\"Refused\""), "kind missing");
    assert!(
        json.contains("Working tree has changes"),
        "blocker 1 missing"
    );
    assert!(json.contains("Branch"), "blocker 2 missing");
}

#[test]
fn json_failed_entry_contains_error() {
    let entry = OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 3_000_000,
        op: "stash-push".to_string(),
        repo: "/tmp/repo".to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "clean".to_string(),
        },
        outcome: OpOutcome::Failed {
            error: "stash push failed: some error".to_string(),
        },
    };
    let json = entry_to_json(&entry);
    assert!(json.contains("\"kind\":\"Failed\""), "kind missing");
    assert!(json.contains("stash push failed"), "error text missing");
}

#[test]
fn json_escapes_special_chars_in_repo_path() {
    let entry = OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 0,
        op: "checkout".to_string(),
        repo: "/path/with \"quotes\" and \\backslash".to_string(),
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
    let json = entry_to_json(&entry);
    // repo path with special chars must be properly escaped.
    assert!(
        json.contains("\\\"quotes\\\""),
        "double-quote escaping failed"
    );
    assert!(json.contains("\\\\backslash"), "backslash escaping failed");
}

// ── append_oplog (integration-style, uses tempdir) ────────
//
// These tests manipulate the KAGI_LOG_DIR environment variable, which is
// process-global.  We serialise them with a mutex so parallel test threads
// do not interfere with each other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn append_two_entries_creates_two_jsonl_lines() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().to_str().unwrap().to_string();

    // Temporarily override the env var for this test.
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let make_entry = |op: &str, ts: i64| OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: ts,
        op: op.to_string(),
        repo: "/tmp/testrepo".to_string(),
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

    let path1 = append_oplog(&make_entry("checkout", 1)).expect("first write");
    let path2 = append_oplog(&make_entry("create-branch", 2)).expect("second write");
    assert_eq!(path1, path2, "both writes should go to the same file");

    let content = std::fs::read_to_string(&path1).expect("read log");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 JSON lines, got: {:?}", lines);

    // Each line must contain the op name.
    assert!(
        lines[0].contains("checkout"),
        "first line should mention checkout"
    );
    assert!(
        lines[1].contains("create-branch"),
        "second line should mention create-branch"
    );

    // Restore env.
    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

#[test]
fn append_includes_expected_json_fields() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().to_str().unwrap().to_string();

    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = OpLogEntry {
        backup_refs: Vec::new(),
        recovery: Vec::new(),
        id: 0,
        parent: None,
        actor: Actor::Human,
        worktree: None,
        timestamp: 9_999,
        op: "stash-apply".to_string(),
        repo: "/my/repo".to_string(),
        before: StateSummary {
            head: "branch: feat".to_string(),
            dirty: "2 modified".to_string(),
        },
        outcome: OpOutcome::Refused {
            blockers: vec!["Working tree is dirty".to_string()],
        },
    };

    let path = append_oplog(&entry).expect("write");
    let line = std::fs::read_to_string(&path).expect("read");
    let line = line.trim_end();

    assert!(line.contains("\"timestamp\":9999"), "timestamp field");
    assert!(line.contains("\"op\":\"stash-apply\""), "op field");
    assert!(line.contains("\"repo\":\"/my/repo\""), "repo field");
    assert!(line.contains("\"kind\":\"Refused\""), "outcome kind");
    assert!(line.contains("Working tree is dirty"), "blocker text");
    assert!(line.contains("\"head\":\"branch: feat\""), "before.head");
    assert!(line.contains("\"dirty\":\"2 modified\""), "before.dirty");

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

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
