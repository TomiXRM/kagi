//! #501 — a transport mutation records at its own execution boundary.
//!
//! The regression these guard: the PR merge used to append the oplog only from
//! the UI completion, and `finish_op_on_main` drops that whole callback when the
//! tab switched mid-op (`OpDisposition::DropStale`). A merge that really
//! happened on GitHub then left no record at all. These tests never run a UI
//! callback — reaching the assertions IS the stale-completion case.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

use kagi_git::github::{merge_pr, plan_pr_merge, MergeMethod};
use kagi_git::oplog::{read_oplog_tail, Actor, OpOutcome};

/// PATH and KAGI_LOG_DIR are process-global; the tests in this binary share them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const HEAD_SHA: &str = "1111111111111111111111111111111111111111";

const PR_JSON: &str = r#"[{"number":501,"title":"transport recording",
  "headRefName":"feat/x","headRefOid":"1111111111111111111111111111111111111111",
  "baseRefName":"main","isDraft":false,"reviewDecision":"APPROVED",
  "mergeable":"MERGEABLE","statusCheckRollup":[],
  "url":"https://example.invalid/pull/501","author":{"login":"a"},
  "reviewRequests":[],"body":""}]"#;

struct Environment {
    path: Option<OsString>,
    log: Option<OsString>,
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in [("PATH", &self.path), ("KAGI_LOG_DIR", &self.log)] {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

impl Environment {
    /// Put `bin` first on PATH and point the oplog at `logs`.
    fn install(bin: &Path, logs: &Path) -> Self {
        let restore = Environment {
            path: std::env::var_os("PATH"),
            log: std::env::var_os("KAGI_LOG_DIR"),
        };
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::split_paths(
            &restore.path.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        std::env::set_var("KAGI_LOG_DIR", logs);
        restore
    }
}

/// A stand-in `gh` so the transport runs for real without touching GitHub.
fn fake_gh(bin: &Path, body: &str) {
    let gh = bin.join("gh");
    std::fs::write(&gh, body).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

const GH_MERGES: &str = "#!/bin/sh\necho '✓ Merged pull request #501'\n";
const GH_REFUSES: &str = "#!/bin/sh\necho 'Head branch was modified' >&2\nexit 1\n";

fn merge_plan() -> kagi_git::OperationPlan {
    let pr = kagi_git::github::parse_pr_list(PR_JSON).unwrap().remove(0);
    plan_pr_merge(&pr, MergeMethod::Squash, false, "branch 'main'".into())
}

#[test]
fn pr_merge_records_before_returning_when_the_ui_completion_is_dropped() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, logs, workdir) = (
        root.path().join("bin"),
        root.path().join("logs"),
        root.path().join("repo"),
    );
    for dir in [&bin, &logs, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let _restore = Environment::install(&bin, &logs);
    let plan = merge_plan();

    fake_gh(&bin, GH_MERGES);
    let report = merge_pr(&workdir, 501, MergeMethod::Squash, false, HEAD_SHA, &plan);
    assert!(report.result.is_ok(), "fake gh should merge");
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));

    // No UI callback ran: this is the dropped-completion path.
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "the merge must be recorded exactly once");
    let entry = &entries[0];
    assert_eq!(entry.op, "pr-merge");
    assert_eq!(entry.actor, Actor::Human);
    assert_eq!(
        entry.worktree.as_deref(),
        Some(workdir.display().to_string().as_str()),
        "the recording must name the worktree it ran in"
    );
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected a recorded success, got {:?}", entry.outcome);
    };
    assert!(
        after.dirty.contains(HEAD_SHA),
        "the head the merge was bound to is the recovery handle: {}",
        after.dirty
    );

    // A refused merge is an accepted attempt too — one more entry, not zero.
    fake_gh(&bin, GH_REFUSES);
    let report = merge_pr(&workdir, 501, MergeMethod::Squash, false, HEAD_SHA, &plan);
    assert!(report.result.is_err());
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 2);
    let OpOutcome::Failed { error } = &entries[0].outcome else {
        panic!("expected a recorded failure, got {:?}", entries[0].outcome);
    };
    assert!(error.contains("Head branch was modified"), "{error}");
}

#[test]
fn pr_merge_reports_recording_failure_without_hiding_the_merge() {
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = tempfile::tempdir().unwrap();
    let (bin, logs, workdir) = (
        root.path().join("bin"),
        root.path().join("logs"),
        root.path().join("repo"),
    );
    for dir in [&bin, &logs, &workdir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // Deterministic append failure: the log file path is a directory (EISDIR).
    std::fs::create_dir_all(logs.join("operations.jsonl")).unwrap();
    let _restore = Environment::install(&bin, &logs);
    fake_gh(&bin, GH_MERGES);

    let report = merge_pr(
        &workdir,
        501,
        MergeMethod::Squash,
        false,
        HEAD_SHA,
        &merge_plan(),
    );
    assert!(
        report.result.is_ok(),
        "a failed append must not turn a completed merge into a failure"
    );
    let kagi_git::backend::recording::Recording::Failed { attempted, .. } = &report.recording
    else {
        panic!("expected a failed recording, got an appended one");
    };
    assert_eq!(attempted.op, "pr-merge");
    assert!(matches!(attempted.outcome, OpOutcome::Success { .. }));
}
