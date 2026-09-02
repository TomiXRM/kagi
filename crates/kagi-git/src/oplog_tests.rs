use super::*;

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
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().to_str().unwrap().to_string();

    // Temporarily override the env var for this test.
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let make_entry = |op: &str, ts: i64| OpLogEntry {
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
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let log_dir = dir.path().to_str().unwrap().to_string();

    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = OpLogEntry {
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
