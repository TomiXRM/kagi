//! Durable non-run operation regressions: Fable T1–T7 (T3 is part of T1).
//! Real Git mutations must be recorded before the Backend returns, without UI.
//! Each integration-test binary has its own environment; serialize this one's
//! tests and restore KAGI_LOG_DIR even when an assertion unwinds.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use kagi_domain::history::{HistoryEntry, OperationKind};
use kagi_domain::plan_note::HistoryMoveDir;
use kagi_git::cli::GitCliOutput;
use kagi_git::oplog::{
    read_oplog_tail, read_oplog_tail_for_repo, recovery, Actor, OpLogEntry, OpOutcome,
};
use kagi_git::ops::{CleanupDeleteTarget, MergedBranchStatus, StateSummary};
use kagi_git::trust::RepoTrust;
use kagi_git::{Backend, CommitId, Operation, OperationOutcome};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());
const NOW: i64 = 1_768_003_200;

struct LogEnv(Option<OsString>);

impl LogEnv {
    fn set(path: &Path) -> Self {
        let previous = Self(std::env::var_os("KAGI_LOG_DIR"));
        std::env::set_var("KAGI_LOG_DIR", path);
        previous
    }
}

impl Drop for LogEnv {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.templateDir=",
        ])
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .expect("git failed to start");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("fixture output is UTF-8")
}

struct Fixture {
    // Restore the environment before removing the directory it points to.
    _log_env: LogEnv,
    _root: TempDir,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let path = root.path().join("repo");
        let logs = root.path().join("logs");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(&logs).unwrap();
        let path = path.canonicalize().unwrap();
        let log_env = LogEnv::set(&logs);
        git(
            &path,
            &["init", "-q", "-b", "main", "--object-format=sha1", "."],
        );
        for (key, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.com"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.hooksPath", "/dev/null"),
        ] {
            git(&path, &["config", key, value]);
        }
        let fixture = Self {
            _log_env: log_env,
            _root: root,
            path,
        };
        fixture.commit_file("base.txt", "base\n", "base");
        fixture
    }

    fn commit_file(&self, name: &str, contents: &str, message: &str) {
        std::fs::write(self.path.join(name), contents).unwrap();
        git(&self.path, &["add", name]);
        git(&self.path, &["commit", "-qm", message]);
    }

    fn stash(&self, contents: &str) {
        std::fs::write(self.path.join("base.txt"), contents).unwrap();
        git(&self.path, &["stash", "push", "-qm", contents]);
    }

    fn history_entry(&self) -> HistoryEntry {
        let before = git(&self.path, &["rev-parse", "HEAD"]).trim().to_string();
        self.commit_file("feature.txt", "committed feature\n", "feature");
        let after = git(&self.path, &["rev-parse", "HEAD"]).trim().to_string();
        HistoryEntry {
            kind: OperationKind::Commit,
            branch: "main".to_string(),
            before: CommitId(before),
            after: CommitId(after),
            summary: "feature".to_string(),
        }
    }

    fn merged_target(&self) -> CleanupDeleteTarget {
        git(&self.path, &["branch", "merged"]);
        let tip = git(&self.path, &["rev-parse", "merged"]).trim().to_string();
        self.commit_file("later.txt", "later\n", "advance main");
        CleanupDeleteTarget {
            name: "merged".to_string(),
            local_tip: Some(CommitId(tip)),
            remote_tip: None,
            status: MergedBranchStatus::FullyMerged,
        }
    }

    fn records(&self, count: usize, actor: Actor) -> Vec<OpLogEntry> {
        // A one-entry tail would conceal double recording. Also check the
        // global file so entries with the wrong repository cannot hide.
        assert_eq!(read_oplog_tail(100).len(), count);
        let entries = read_oplog_tail_for_repo(&self.path, 100);
        assert_eq!(entries.len(), count);
        for entry in &entries {
            assert_eq!(entry.actor, actor);
            assert_eq!(Path::new(&entry.repo).canonicalize().unwrap(), self.path);
            let worktree = entry
                .worktree
                .as_deref()
                .expect("worktree must be recorded");
            assert_eq!(Path::new(worktree).canonicalize().unwrap(), self.path);
        }
        entries
    }
}

/// The run-family outcome branch cleanup always produces.
fn cleanup(outcome: OperationOutcome) -> kagi_git::ops::CleanupOutcome {
    match outcome {
        OperationOutcome::BranchCleanup(cleanup) => cleanup,
        other => panic!("expected a branch-cleanup outcome, got {other:?}"),
    }
}

fn success(entry: &OpLogEntry) -> &StateSummary {
    match &entry.outcome {
        OpOutcome::Success { after } => after,
        other => panic!("expected successful durable record, got {other:?}"),
    }
}

fn logged_oid<'a>(summary: &'a str, expected: &str) -> &'a str {
    assert_eq!(expected.len(), 40);
    summary
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|token| *token == expected)
        .expect("durable summary must contain the full recoverable commit OID")
}

// HEAD, index entries, status, and tracked content: refusal must preserve all.
fn repo_state(path: &Path) -> (String, String, String, String) {
    (
        git(path, &["rev-parse", "HEAD"]),
        git(path, &["ls-files", "--stage"]),
        git(path, &["status", "--porcelain=v1"]),
        git(path, &["diff", "HEAD", "--", "."]),
    )
}

#[test]
fn stash_drop_records_recoverable_oid_without_worktree_snapshot() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    fixture.stash("stashed content\n");
    let expected = git(dir, &["rev-parse", "refs/stash"]).trim().to_string();
    // A dirty tree makes an accidental auto-snapshot a real extra savepoint.
    std::fs::write(dir.join("base.txt"), "unrelated live edits\n").unwrap();
    let before = repo_state(dir);
    let mut backend = Backend::open(dir).unwrap();
    backend.set_auto_snapshot(true);
    let plan = backend.plan_stash_drop(0).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let result = backend
        .run(&Operation::StashDrop { index: 0 }, &plan)
        .unwrap();
    assert!(matches!(result, OperationOutcome::StashDrop { oid } if oid == expected));
    assert!(git(dir, &["stash", "list"]).is_empty());
    assert_eq!(repo_state(dir), before);
    assert!(git(dir, &["for-each-ref", "refs/kagi/snapshots/"]).is_empty());

    let records = fixture.records(1, Actor::Human);
    assert_eq!(records[0].op, "stash-drop");
    let recovered = logged_oid(&success(&records[0]).dirty, &expected);
    assert_eq!(git(dir, &["cat-file", "-t", recovered]).trim(), "commit");
    git(dir, &["stash", "store", "-m", "recovered", recovered]);
    assert_eq!(git(dir, &["stash", "list", "--format=%H"]).trim(), expected);
    assert_eq!(
        git(dir, &["show", "stash@{0}:base.txt"]),
        "stashed content\n"
    );
    assert_eq!(repo_state(dir), before);
}

#[test]
fn stash_drop_refuses_shifted_stash_list_and_records_refusal() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    fixture.stash("first stash\n");
    fixture.stash("second stash\n");
    let mut backend = Backend::open(dir).unwrap();
    let plan = backend.plan_stash_drop(0).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    fixture.stash("third stash\n");
    let stashes = git(dir, &["stash", "list", "--format=%H"]);
    assert_eq!(stashes.lines().count(), 3);
    let before = repo_state(dir);

    let error = backend
        .run(&Operation::StashDrop { index: 0 }, &plan)
        .unwrap_err();
    assert!(
        error.is_preflight(),
        "caller must retain the preflight failure phase"
    );
    assert_eq!(git(dir, &["stash", "list", "--format=%H"]), stashes);
    assert_eq!(repo_state(dir), before);
    let records = fixture.records(1, Actor::Human);
    assert_eq!(records[0].op, "stash-drop");
    assert!(matches!(records[0].outcome, OpOutcome::Refused { .. }));
}

#[test]
fn history_undo_redo_preserves_edits_and_records_full_oid_chain() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let entry = fixture.history_entry();
    std::fs::write(dir.join("base.txt"), "staged local edits\n").unwrap();
    git(dir, &["add", "base.txt"]);
    std::fs::write(dir.join("base.txt"), "unstaged local edits\n").unwrap();
    let index_before = git(dir, &["ls-files", "--stage"]);
    let mut backend = Backend::open(dir).unwrap();
    backend.set_actor(Actor::Mcp);
    let undo_plan = backend.plan_undo(&entry).unwrap();
    assert!(undo_plan.blockers.is_empty(), "{:?}", undo_plan.blockers);
    backend
        .run_history_move(HistoryMoveDir::Undo, &undo_plan, &entry)
        .unwrap();

    assert_eq!(git(dir, &["rev-parse", "main"]).trim(), entry.before.0);
    // execute_history_move is ref-only (soft), unlike its stale mixed-reset
    // overview comment: committed changes become staged; existing index stays.
    assert_eq!(git(dir, &["ls-files", "--stage"]), index_before);
    assert_eq!(
        git(
            dir,
            &["diff", "--cached", "--name-only", "--", "feature.txt"]
        )
        .trim(),
        "feature.txt"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("base.txt")).unwrap(),
        "unstaged local edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("feature.txt")).unwrap(),
        "committed feature\n"
    );
    let undo_records = fixture.records(1, Actor::Mcp);
    let undo = &undo_records[0];
    assert_eq!(undo.op, "undo-commit");
    assert_eq!(undo.parent, None);
    logged_oid(&success(undo).dirty, &entry.after.0);
    logged_oid(&success(undo).dirty, &entry.before.0);
    // #500: the same two ends, typed, so a recovery consumer never has to read
    // "moved from <oid> to <oid>" out of the sentence above.
    assert_eq!(
        undo.recovery
            .iter()
            .map(|h| (h.kind.as_str(), h.oid.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (recovery::HISTORY_FROM, entry.after.0.as_str()),
            (recovery::HISTORY_TO, entry.before.0.as_str()),
        ]
    );

    let redo_plan = backend.plan_redo(&entry).unwrap();
    assert!(redo_plan.blockers.is_empty(), "{:?}", redo_plan.blockers);
    backend
        .run_history_move(HistoryMoveDir::Redo, &redo_plan, &entry)
        .unwrap();
    assert_eq!(git(dir, &["rev-parse", "main"]).trim(), entry.after.0);
    assert_eq!(git(dir, &["ls-files", "--stage"]), index_before);
    assert!(git(
        dir,
        &["diff", "--cached", "--name-only", "--", "feature.txt"]
    )
    .is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.join("base.txt")).unwrap(),
        "unstaged local edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("feature.txt")).unwrap(),
        "committed feature\n"
    );
    let records = fixture.records(2, Actor::Mcp);
    let redo = &records[0];
    assert_eq!(redo.op, "redo-commit");
    assert_eq!(records[1].id, undo.id);
    assert!(redo.id > undo.id);
    assert_eq!(redo.parent, Some(undo.id));
    logged_oid(&success(redo).dirty, &entry.before.0);
    logged_oid(&success(redo).dirty, &entry.after.0);
}

#[test]
fn history_undo_refuses_moved_head_and_records_failure() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let entry = fixture.history_entry();
    let backend = Backend::open(dir).unwrap();
    let plan = backend.plan_undo(&entry).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    git(dir, &["commit", "--allow-empty", "-qm", "external commit"]);
    std::fs::write(dir.join("base.txt"), "retain staged edits\n").unwrap();
    git(dir, &["add", "base.txt"]);
    std::fs::write(dir.join("base.txt"), "retain unstaged edits\n").unwrap();
    let before = repo_state(dir);

    let error = backend
        .run_history_move(HistoryMoveDir::Undo, &plan, &entry)
        .unwrap_err();
    assert!(
        error.is_preflight(),
        "caller must retain the preflight failure phase"
    );
    assert_eq!(repo_state(dir), before);
    let records = fixture.records(1, Actor::Human);
    assert_eq!(records[0].op, "undo-commit");
    assert!(matches!(records[0].outcome, OpOutcome::Failed { .. }));
}

#[test]
fn cleanup_records_full_tips_and_allows_local_and_remote_recovery() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let mut target = fixture.merged_target();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare", "."]);
    git(
        dir,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir, &["push", "origin", "merged"]);
    target.remote_tip = target.local_tip.clone();
    let expected = &target.local_tip.as_ref().unwrap().0;
    let before = repo_state(dir);
    let mut backend = Backend::open(dir).unwrap();
    backend.set_actor(Actor::Cli);
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(&target))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let outcome = backend
        .execute_delete_merged_branches(&plan, std::slice::from_ref(&target))
        .result
        .map(cleanup)
        .unwrap();
    assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
    assert_eq!(outcome.deleted.len(), 1);
    assert_eq!(outcome.deleted[0].local_tip, target.local_tip);
    assert_eq!(outcome.deleted[0].remote_tip, target.remote_tip);
    assert!(git(remote.path(), &["for-each-ref", "refs/heads/merged"]).is_empty());
    assert!(git(dir, &["for-each-ref", "refs/heads/merged"]).is_empty());
    assert_eq!(repo_state(dir), before);

    let records = fixture.records(1, Actor::Cli);
    assert_eq!(records[0].op, "branch-cleanup");
    let summary = &success(&records[0]).dirty;
    assert!(summary.contains(&target.name));
    let recovered = logged_oid(summary, expected);
    git(dir, &["branch", &target.name, recovered]);
    assert_eq!(git(dir, &["rev-parse", &target.name]).trim(), expected);
    git(
        dir,
        &["push", "origin", &format!("{recovered}:refs/heads/merged")],
    );
    assert_eq!(
        git(remote.path(), &["rev-parse", "merged"]).trim(),
        expected
    );
    assert_eq!(repo_state(dir), before);
}

#[test]
fn untrusted_cleanup_preserves_branch_and_records_failure() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let target = fixture.merged_target();
    let expected = git(dir, &["rev-parse", &target.name]);
    let mut backend = Backend::open(dir).unwrap();
    backend.set_actor(Actor::Cli);
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(&target))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    std::fs::write(dir.join("base.txt"), "keep local edits\n").unwrap();
    let before = repo_state(dir);
    backend.set_trust_for_test(RepoTrust::Untrusted);

    let error = backend
        .execute_delete_merged_branches(&plan, &[target])
        .result
        .unwrap_err();
    assert!(error.is_untrusted());
    assert_eq!(git(dir, &["rev-parse", "merged"]), expected);
    assert_eq!(repo_state(dir), before);
    let records = fixture.records(1, Actor::Cli);
    assert_eq!(records[0].op, "branch-cleanup");
    assert!(matches!(records[0].outcome, OpOutcome::Failed { .. }));
}

#[cfg(unix)]
#[test]
fn cleanup_remote_success_local_failure_preserves_recovery_and_records_partial() {
    if !crate::test_support::run_isolated() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;

    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let mut target = fixture.merged_target();
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare", "."]);
    git(
        dir,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir, &["push", "origin", "merged"]);
    target.remote_tip = target.local_tip.clone();
    let expected = &target.remote_tip.as_ref().unwrap().0;
    let moved = git(dir, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(&moved, expected);
    let before = repo_state(dir);
    let mut backend = Backend::open(dir).unwrap();
    backend.set_actor(Actor::Cli);
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(&target))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);

    // post-receive runs after the remote ref transaction commits, but before
    // push returns to cleanup's local verification. Move only the local ref:
    // HEAD/index/worktree stay unchanged and there is no timing-dependent race.
    let hook = remote.path().join("hooks/post-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    git(
        remote.path(),
        &[
            "config",
            "core.hooksPath",
            hook.parent().unwrap().to_str().unwrap(),
        ],
    );
    let local_git_dir = dir
        .join(".git")
        .display()
        .to_string()
        .replace('\'', "'\\''");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec git --git-dir='{local_git_dir}' update-ref refs/heads/merged {moved}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    let outcome = backend
        .execute_delete_merged_branches(&plan, std::slice::from_ref(&target))
        .result
        .map(cleanup)
        .unwrap();
    assert!(git(remote.path(), &["for-each-ref", "refs/heads/merged"]).is_empty());
    assert!(git(dir, &["for-each-ref", "refs/remotes/origin/merged"]).is_empty());
    assert_eq!(git(dir, &["rev-parse", "merged"]).trim(), moved);
    assert_eq!(repo_state(dir), before);
    assert_eq!(outcome.deleted.len(), 1);
    assert_eq!(outcome.deleted[0].name, target.name);
    assert_eq!(outcome.deleted[0].remote_tip, target.remote_tip);
    assert!(outcome.deleted[0].local_tip.is_none());
    assert_eq!(outcome.failed.len(), 1);
    assert_eq!(outcome.failed[0].0, target.name);

    let records = fixture.records(1, Actor::Cli);
    assert_eq!(records[0].op, "branch-cleanup");
    let (after, error) = match &records[0].outcome {
        OpOutcome::Partial { after, error } => (after, error),
        other => panic!("expected durable Partial after remote deletion, got {other:?}"),
    };
    assert!(error.contains(&target.name));
    let recovered = logged_oid(&after.dirty, expected);
    std::fs::remove_file(hook).unwrap();
    git(
        dir,
        &["push", "origin", &format!("{recovered}:refs/heads/merged")],
    );
    assert_eq!(
        git(remote.path(), &["rev-parse", "merged"]).trim(),
        expected
    );
    assert_eq!(git(dir, &["rev-parse", "merged"]).trim(), moved);
    assert_eq!(repo_state(dir), before);
}

#[test]
fn cleanup_moved_local_tip_without_deletions_records_failed() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let target = fixture.merged_target();
    let mut backend = Backend::open(dir).unwrap();
    backend.set_actor(Actor::Cli);
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(&target))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    git(dir, &["update-ref", "refs/heads/merged", "HEAD"]);
    let moved = git(dir, &["rev-parse", "merged"]);
    let before = repo_state(dir);

    let outcome = backend
        .execute_delete_merged_branches(&plan, &[target])
        .result
        .map(cleanup)
        .unwrap();
    assert!(outcome.deleted.is_empty());
    assert_eq!(outcome.failed.len(), 1);
    assert_eq!(outcome.failed[0].0, "merged");
    assert!(outcome.failed[0]
        .1
        .contains("local branch moved since plan"));
    assert_eq!(git(dir, &["rev-parse", "merged"]), moved);
    assert_eq!(repo_state(dir), before);
    let records = fixture.records(1, Actor::Cli);
    match &records[0].outcome {
        OpOutcome::Failed { error } => assert!(error.contains(&outcome.failed[0].1)),
        other => panic!("expected Failed with no deleted refs, got {other:?}"),
    }
}

/// Commands the injected runner saw, so "no fallback push ran" is an
/// observation and not an inference. One test uses it; `ENV_LOCK` serializes.
static RUNNER_CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Give the fixture's `merged` branch a remote half, so the cleanup reaches
/// the `push --delete` the runner answers.
fn push_target_to(fixture: &Fixture, remote: &TempDir, target: &mut CleanupDeleteTarget) {
    let dir = &fixture.path;
    git(remote.path(), &["init", "--bare", "."]);
    git(
        dir,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir, &["push", "origin", "merged"]);
    git(dir, &["fetch", "-q", "origin"]);
    target.remote_tip = target.local_tip.clone();
}

/// A pid that has already been reaped: the handle a `Termination` carries when
/// the executor could not account for its child, in the state a later
/// reconcile read must be able to prove.
fn dead_pid() -> u32 {
    let mut child = Command::new("true")
        .spawn()
        .expect("spawn a short-lived child");
    let pid = child.id();
    child.wait().expect("reap it");
    pid
}

/// The runner answers the batch delete with a termination the executor *saw*
/// end — what a `gh`-style transport reports when the process is accounted for
/// but the repository state is not.
fn push_stopped(dir: &Path, args: &[&str]) -> Result<GitCliOutput, kagi_git::GitError> {
    RUNNER_CALLS.lock().unwrap().push(args.join(" "));
    if args.first() == Some(&"push") {
        return Err(kagi_git::GitError::TerminationUnknown(
            kagi_git::Termination::stopped("push --delete: the remote closed the connection"),
        ));
    }
    kagi_git::cli::run_git(dir, args)
}

/// The runner answers with an *unproven* termination: the deadline expired, the
/// child could not be reaped, and its pid is the only handle left on it.
fn push_unproven(dir: &Path, args: &[&str]) -> Result<GitCliOutput, kagi_git::GitError> {
    RUNNER_CALLS.lock().unwrap().push(args.join(" "));
    if args.first() == Some(&"push") {
        return Err(kagi_git::GitError::TerminationUnknown(
            kagi_git::Termination {
                reason: "push --delete: deadline expired".to_string(),
                child_stopped: false,
                pid: Some(dead_pid()),
            },
        ));
    }
    kagi_git::cli::run_git(dir, args)
}

/// Run one cleanup through the production admission path with `runner`, and
/// hand back the sessions it settled into plus the operation id.
fn cleanup_through_the_app(
    fixture: &Fixture,
    target: &CleanupDeleteTarget,
    runner: kagi_git::ops::GitRunner,
) -> (kagi::app::Sessions, kagi::app::OperationId, OpOutcome) {
    let dir = &fixture.path;
    let backend = Backend::open(dir).unwrap();
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(target))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let repo_id = backend.write_repo_id().unwrap();
    drop(backend);

    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).expect("the tab just attached");
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "branch-cleanup",
            path: dir.clone(),
            repo: repo_id,
            plan: std::sync::Arc::new(plan.clone()),
        },
    )
    .expect("a fresh session admits the write");
    let (job_path, job_plan, job_targets) = (dir.clone(), plan, vec![target.clone()]);
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        kagi::app::LegacyBusy(false),
        Box::new(move || {
            let mut backend = Backend::open(&job_path).map_err(|e| e.to_string())?;
            backend.set_actor(Actor::Cli);
            Ok(backend.execute_delete_merged_branches_with(&job_plan, &job_targets, runner))
        }),
    )
    .expect("nothing else holds the lease");
    let id = job.id();
    let completion = job.run();
    let recorded = completion.report.recording.entry().outcome.clone();
    sessions.apply(completion);
    (sessions, id, recorded)
}

/// ADR-0177 / ADR-0196 決定 2.4: a `push --delete` whose termination is
/// unconfirmed stops the cleanup — retrying it per branch would re-run a
/// deletion that may already have happened — and the `Termination` reaches the
/// settlement *typed*, because `child_stopped` / `pid` is the whole difference
/// between a lease with an exit and a lease without one.
#[test]
fn cleanup_stops_at_an_unconfirmed_delete_and_keeps_the_lease() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // ── The child was accounted for: the scope is free, the receipt is not.
    RUNNER_CALLS.lock().unwrap().clear();
    let fixture = Fixture::new();
    let mut target = fixture.merged_target();
    let remote = TempDir::new().unwrap();
    push_target_to(&fixture, &remote, &mut target);
    let local_tip = git(&fixture.path, &["rev-parse", "merged"]);

    let (mut sessions, id, recorded) = cleanup_through_the_app(&fixture, &target, push_stopped);
    let pushes: Vec<String> = RUNNER_CALLS
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("push "))
        .cloned()
        .collect();
    assert_eq!(
        pushes.len(),
        1,
        "an unconfirmed delete must not be re-run per branch: {pushes:?}"
    );
    assert!(
        matches!(recorded, OpOutcome::Unknown { .. }),
        "the receipt must be Unknown, not a retryable failure: {recorded:?}"
    );
    // The workflow stopped: the local half of the same branch is untouched.
    assert_eq!(git(&fixture.path, &["rev-parse", "merged"]), local_tip);
    assert!(
        !sessions.has_leases(),
        "a child the executor saw stop releases the scope at settlement"
    );
    assert_eq!(sessions.reconcile_ids(), vec![id]);
    let read = kagi::app::read_reconcile(&sessions, id).expect("the entry is readable");
    assert!(read.stop_proven(), "the executor already proved the stop");
    kagi::app::acknowledge(&mut sessions, read).expect("and it can be acknowledged");
    assert!(sessions.reconcile_ids().is_empty());
    let records = fixture.records(1, Actor::Cli);
    assert!(matches!(records[0].outcome, OpOutcome::Unknown { .. }));
    assert_eq!(
        records[0].failure_code,
        Some(kagi_git::oplog::FailureCode::TerminationUnknown)
    );

    // ── The child could not be accounted for: the scope stays reserved until
    // a read proves the pid is gone, and only then can it be acknowledged.
    RUNNER_CALLS.lock().unwrap().clear();
    let fixture = Fixture::new();
    let mut target = fixture.merged_target();
    let remote = TempDir::new().unwrap();
    push_target_to(&fixture, &remote, &mut target);

    let (mut sessions, id, recorded) = cleanup_through_the_app(&fixture, &target, push_unproven);
    assert!(matches!(recorded, OpOutcome::Unknown { .. }));
    assert!(
        sessions.has_leases(),
        "an unproven termination retains the scope (ADR-0175)"
    );
    assert_eq!(sessions.reconcile_ids(), vec![id]);
    let read = kagi::app::read_reconcile(&sessions, id)
        .expect("the pid it carries is what makes the entry readable");
    assert!(
        read.stop_proven(),
        "the read asked the OS: that child is gone"
    );
    kagi::app::acknowledge(&mut sessions, read).expect("the proven stop releases the scope");
    assert!(
        !sessions.has_leases(),
        "acknowledging an unproven termination is the exit from the retained lease"
    );
    assert!(sessions.reconcile_ids().is_empty());
}

/// #289 / ADR-0196 決定 2.4: a run job whose task unwinds returns no
/// completion. Dropping it there would strand the operation — the lease stays
/// reserved with nothing to acknowledge it against, and the UI's busy mirror
/// would be cleared under it. The abandonment settles it as `Unknown` through
/// the same `apply` instead.
#[test]
fn a_panicked_run_job_settles_as_unknown_and_keeps_its_reconcile_entry() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let target = fixture.merged_target();
    let backend = Backend::open(dir).unwrap();
    let plan = backend
        .plan_delete_merged_branches(NOW, std::slice::from_ref(&target))
        .unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    drop(backend);

    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).unwrap();
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "branch-cleanup",
            path: dir.clone(),
            repo: repo_id,
            plan: std::sync::Arc::new(plan),
        },
    )
    .unwrap();
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        kagi::app::LegacyBusy(false),
        Box::new(|| panic!("the transport unwound mid-write")),
    )
    .unwrap();
    let id = job.id();
    // What the UI holds before the task can end: the completion to settle with
    // if the task never returns one.
    let abandonment = job.abandonment();
    let hushed = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())).is_err();
    std::panic::set_hook(hushed);
    assert!(
        panicked,
        "the job must actually unwind for this to be the case"
    );

    sessions.apply(abandonment.into_completion());

    assert!(
        sessions.has_leases(),
        "a panic proves nothing: the writer may still be running (ADR-0175)"
    );
    assert_eq!(
        sessions.reconcile_ids(),
        vec![id],
        "and the operation keeps its id, so the retained lease has an exit"
    );
    let records = fixture.records(1, Actor::Human);
    assert_eq!(records[0].op, "branch-cleanup");
    assert!(
        matches!(records[0].outcome, OpOutcome::Unknown { .. }),
        "an abandoned write is Unknown, never a failure: {:?}",
        records[0].outcome
    );
}

#[path = "support/isolated.rs"]
mod test_support;
