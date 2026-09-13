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
        self.merged_target_named("merged")
    }

    fn merged_target_named(&self, name: &str) -> CleanupDeleteTarget {
        git(&self.path, &["branch", name]);
        let tip = git(&self.path, &["rev-parse", name]).trim().to_string();
        self.commit_file(
            &format!("later-{name}.txt"),
            "later\n",
            &format!("advance main past {name}"),
        );
        CleanupDeleteTarget {
            name: name.to_string(),
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

/// A process group that has already gone: the handle an `Unaccounted`
/// termination carries, in the state a later reconcile read must be able to
/// prove. A reaped child leads its own group here — nothing is left to signal.
fn dead_group() -> u32 {
    let mut child = Command::new("true")
        .spawn()
        .expect("spawn a short-lived child");
    let group = child.id();
    child.wait().expect("reap it");
    group
}

/// The runner answers the batch delete with a termination the executor *saw*
/// end — what a transport reports when the process is accounted for but the
/// repository state is not.
fn push_stopped(dir: &Path, args: &[&str]) -> Result<GitCliOutput, kagi_git::GitError> {
    RUNNER_CALLS.lock().unwrap().push(args.join(" "));
    if args.first() == Some(&"push") {
        return Err(kagi_git::GitError::TerminationUnknown(
            kagi_git::Termination::stopped("push --delete: the remote closed the connection"),
        ));
    }
    kagi_git::cli::run_git(dir, args)
}

/// The runner answers with an *unaccounted* termination: the deadline expired,
/// the kill could not account for the child, and the process group it was
/// spawned into is the only handle left on it.
fn push_unaccounted(dir: &Path, args: &[&str]) -> Result<GitCliOutput, kagi_git::GitError> {
    RUNNER_CALLS.lock().unwrap().push(args.join(" "));
    if args.first() == Some(&"push") {
        return Err(kagi_git::GitError::TerminationUnknown(
            kagi_git::Termination::Unaccounted {
                reason: "push --delete: deadline expired".to_string(),
                group: dead_group(),
            },
        ));
    }
    kagi_git::cli::run_git(dir, args)
}

/// Give one of the fixture's branches a remote half on `remote`, so the
/// cleanup reaches the `push --delete` the runner answers.
fn push_target_to(fixture: &Fixture, remote: &Path, target: &mut CleanupDeleteTarget) {
    let dir = &fixture.path;
    git(dir, &["push", "origin", &target.name]);
    git(dir, &["fetch", "-q", "origin"]);
    let _ = remote;
    target.remote_tip = target.local_tip.clone();
}

/// Run one cleanup through the production admission path with `runner`, and
/// hand back the sessions it settled into plus the operation id.
fn cleanup_through_the_app(
    fixture: &Fixture,
    targets: &[CleanupDeleteTarget],
    runner: kagi_git::ops::GitRunner,
) -> (kagi::app::Sessions, kagi::app::OperationId, OpOutcome) {
    let dir = &fixture.path;
    let backend = Backend::open(dir).unwrap();
    let plan = backend.plan_delete_merged_branches(NOW, targets).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let repo_id = backend.write_repo_id().unwrap();
    // Frozen at approval: one `Absent` promise per remote half this batch will
    // delete — no more (a local-only target promises nothing) and no fewer.
    let remote = backend.remote_expectation("branch-cleanup", &plan);
    let expected: Vec<kagi_git::backend::remote_ref::RemoteExpectation> = targets
        .iter()
        .filter(|t| t.remote_tip.is_some())
        .map(|t| kagi_git::backend::remote_ref::RemoteExpectation::Ref {
            remote: "origin".to_string(),
            refname: format!("refs/heads/{}", t.name),
            expect: kagi_git::backend::remote_ref::RemoteExpect::Absent,
        })
        .collect();
    assert_eq!(remote, expected, "the batch is frozen at plan time");
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
            remote,
        },
    )
    .expect("a fresh session admits the write");
    let (job_path, job_plan, job_targets) = (dir.clone(), plan, targets.to_vec());
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
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

/// Two merged branches, both pushed to a bare remote: the batch this cleanup
/// promises to delete. Returns the targets and the remote.
fn two_remote_targets(fixture: &Fixture) -> (Vec<CleanupDeleteTarget>, TempDir) {
    let remote = TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare", "."]);
    git(
        &fixture.path,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let mut targets = vec![
        fixture.merged_target_named("merged"),
        fixture.merged_target_named("merged-two"),
    ];
    for target in &mut targets {
        push_target_to(fixture, remote.path(), target);
    }
    (targets, remote)
}

/// ADR-0177 / ADR-0196 決定 2.4: a `push --delete` whose termination is
/// unconfirmed stops the cleanup — retrying it per branch would re-run a
/// deletion that may already have happened — and the `Termination` reaches the
/// settlement *typed*, because `child_stopped` / `group` is the whole
/// difference between a lease with an exit and a lease without one.
///
/// The exit itself is the frozen batch: the read confirms the delete only when
/// **every** promised ref is gone from the remote (#701).
#[test]
fn cleanup_stops_at_an_unconfirmed_delete_and_keeps_the_lease() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // ── The child was accounted for: the scope is free, the receipt is not.
    RUNNER_CALLS.lock().unwrap().clear();
    let fixture = Fixture::new();
    let (targets, remote) = two_remote_targets(&fixture);
    let local_tip = git(&fixture.path, &["rev-parse", "merged"]);

    let (mut sessions, id, recorded) = cleanup_through_the_app(&fixture, &targets, push_stopped);
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

    // Nothing was deleted on the remote, so nothing is confirmed.
    let read = kagi::app::read_reconcile(&sessions, id).expect("the entry is readable");
    assert!(read.stop_proven(), "the executor already proved the stop");
    assert!(
        !read.resolved(),
        "both promised refs are still on the remote: {}",
        read.observation
    );
    assert!(kagi::app::acknowledge(&mut sessions, read).is_err());

    // One of the two is gone. A batch that removed half of itself is not a
    // confirmed batch — this is the assertion "any one absent" would break.
    git(remote.path(), &["update-ref", "-d", "refs/heads/merged"]);
    let read = kagi::app::read_reconcile(&sessions, id).unwrap();
    assert!(
        !read.resolved(),
        "one of two refs is not the batch: {}",
        read.observation
    );
    assert!(kagi::app::acknowledge(&mut sessions, read).is_err());

    // Both gone: the promise the plan froze is kept, and only now can the
    // requirement be closed.
    git(
        remote.path(),
        &["update-ref", "-d", "refs/heads/merged-two"],
    );
    let read = kagi::app::read_reconcile(&sessions, id).unwrap();
    assert!(
        read.resolved(),
        "every promised ref is absent: {}",
        read.observation
    );
    kagi::app::acknowledge(&mut sessions, read).expect("a kept promise can be acknowledged");
    assert!(sessions.reconcile_ids().is_empty());
    let records = fixture.records(1, Actor::Cli);
    assert!(matches!(records[0].outcome, OpOutcome::Unknown { .. }));
    assert_eq!(
        records[0].failure_code,
        Some(kagi_git::oplog::FailureCode::TerminationUnknown)
    );

    // ── The group could not be accounted for: the scope stays reserved until
    // a read proves that group is gone — and even then, only a kept promise
    // closes it.
    RUNNER_CALLS.lock().unwrap().clear();
    let fixture = Fixture::new();
    let (targets, remote) = two_remote_targets(&fixture);

    let (mut sessions, id, recorded) =
        cleanup_through_the_app(&fixture, &targets, push_unaccounted);
    assert!(matches!(recorded, OpOutcome::Unknown { .. }));
    assert!(
        sessions.has_leases(),
        "an unaccounted process group retains the scope (ADR-0175)"
    );
    assert_eq!(sessions.reconcile_ids(), vec![id]);
    for name in ["merged", "merged-two"] {
        git(
            remote.path(),
            &["update-ref", "-d", &format!("refs/heads/{name}")],
        );
    }
    let read = kagi::app::read_reconcile(&sessions, id)
        .expect("the group it carries is what makes the entry readable");
    assert!(
        read.stop_proven(),
        "the read asked the OS: that group is gone"
    );
    assert!(read.resolved(), "and the remote kept the promise");
    kagi::app::acknowledge(&mut sessions, read).expect("the proven stop releases the scope");
    assert!(
        !sessions.has_leases(),
        "acknowledging a proven-gone group is the exit from the retained lease"
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
    // A local-only target promises nothing on a remote.
    let remote = backend.remote_expectation("branch-cleanup", &plan);
    assert!(remote.is_empty(), "no remote half, no promise");
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
            remote,
        },
    )
    .unwrap();
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
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
        "and the operation keeps its id rather than being dropped on the floor"
    );
    // `Abandoned` is deliberately the one termination with no automatic exit:
    // kagi lost its own executor, so there is no group to probe and the read
    // says so instead of offering an acknowledgement that proves nothing.
    assert_eq!(
        kagi::app::prepare_reconcile(&sessions, id).err().as_deref(),
        Some("execution termination is unconfirmed and nothing is left to probe"),
    );
    let records = fixture.records(1, Actor::Human);
    assert_eq!(records[0].op, "branch-cleanup");
    assert!(
        matches!(records[0].outcome, OpOutcome::Unknown { .. }),
        "an abandoned write is Unknown, never a failure: {:?}",
        records[0].outcome
    );
}

/// A stand-in `gh` first on PATH, answering `gh pr view … mergedAt` with
/// `body`. The guard restores PATH on drop, so one test can ask the same
/// question of three different servers.
struct FakeGh(Option<OsString>);
impl FakeGh {
    fn answering(bin: &Path, body: &str) -> Self {
        std::fs::create_dir_all(bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let restore = Self(std::env::var_os("PATH"));
        let mut paths = vec![bin.to_path_buf()];
        paths.extend(std::env::split_paths(
            &restore.0.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        restore
    }

    /// A `gh` that answers *per repository and per exact ref*, logging every
    /// argv to `log`.
    ///
    /// [`BASE_REPO`] — the one the plan froze — reports the PR merged, and the
    /// GraphQL ref query answers for [`HEAD_BRANCH`] alone: present, or the
    /// structured `"ref": null`. **Any other** repository, ref, or host gets
    /// the false-confirm shape — merged, and a 404 — so a read that is not
    /// pinned to the frozen identity, or that mangles the branch into a URL,
    /// resolves; a pinned structured one cannot (#701 reviews 4 and 5).
    fn serving(bin: &Path, log: &Path, branch_present: bool) -> Self {
        let answer = if branch_present {
            r#"{"data":{"repository":{"ref":{"target":{"oid":"1111111111111111111111111111111111111111"}}}}}"#
        } else {
            r#"{"data":{"repository":{"ref":null}}}"#
        };
        Self::serving_ref(bin, log, &format!("echo '{answer}'"))
    }

    /// As [`Self::serving`], but the ref query answers with `body` — for the
    /// shapes that are *not* an answer: a GraphQL error, a null repository.
    fn serving_ref(bin: &Path, log: &Path, body: &str) -> Self {
        Self::answering(
            bin,
            &format!(
                r#"echo "$*" >> {log}
case "$*" in
  *"-R {BASE_REPO}"*)
      echo '{{"mergedAt":"2026-09-07T00:00:00Z"}}' ;;
  *graphql*"--hostname ghe.example"*"owner=acme"*"name=widgets"*"ref=refs/heads/{HEAD_BRANCH}"*)
      {body} ;;
  "pr view"*)
      echo '{{"mergedAt":"2026-09-07T00:00:00Z"}}' ;;
  *)
      echo 'gh: Not Found (HTTP 404)' >&2; exit 1 ;;
esac"#,
                log = log.display(),
            ),
        )
    }
}
impl Drop for FakeGh {
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// A same-repository PR. `{fork}` is substituted so the fork-shaped variant
/// differs from it in exactly the one field that decides the promise.
const PR_JSON_T: &str = r#"[{"number":501,"title":"reconcile","headRefName":"feature#x",
  "headRefOid":"1111111111111111111111111111111111111111","baseRefName":"main",
  "isDraft":false,"reviewDecision":"APPROVED","mergeable":"MERGEABLE",
  "statusCheckRollup":[],"url":"https://ghe.example/acme/widgets/pull/501",
  "author":{"login":"a"},"reviewRequests":[],"body":"",
  "isCrossRepository":{fork}}]"#;

/// The base repository the fixture's `url` names, host included.
const BASE_URL: &str = "https://ghe.example/acme/widgets";
/// The same repository as `<host>/<owner>/<repo>` — what the plan freezes.
const BASE_REPO: &str = "ghe.example/acme/widgets";
/// The PR's head branch. `#` is a valid branch character and a URL fragment
/// delimiter, so a read that puts it in a path asks about `feature` instead
/// and reads that ref's absence as this one's (#701 review 5).
const HEAD_BRANCH: &str = "feature#x";
/// Same `acme/widgets`, different host: the remote that must never answer.
const DECOY_URL: &str = "https://github.com/acme/widgets";

fn pr_json(fork: bool) -> String {
    PR_JSON_T.replace("{fork}", if fork { "true" } else { "false" })
}

fn one_pr(fork: bool) -> kagi_domain::github::PullRequest {
    kagi_git::github::parse_pr_list(&pr_json(fork))
        .unwrap()
        .remove(0)
}

/// #701: a pr-merge whose server state could not be re-read settles `Unknown`
/// with the child accounted for — the lease goes, the requirement stays. Its
/// exit is the promise frozen at approval: the PR itself must read *merged*
/// when asked again. "Open" is a readable answer that confirms nothing, and
/// "could not ask" must never pass for either.
#[test]
fn pr_merge_unknown_resolves_only_on_a_merged_re_read() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let bin = dir.join("fake-bin");

    let pr = one_pr(false);
    let plan = kagi_git::github::plan_pr_merge(
        &pr,
        kagi_git::github::MergeMethod::Squash,
        false,
        "branch 'main'".into(),
    );
    let backend = Backend::open(dir).unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    let remote = backend.remote_expectation("pr-merge", &plan);
    assert_eq!(
        remote,
        vec![
            kagi_git::backend::remote_ref::RemoteExpectation::PullRequest {
                base_repo: BASE_REPO.to_string(),
                number: 501,
                expect: kagi_git::backend::remote_ref::PrExpect::Merged,
            }
        ],
        "the promise a merge freezes is the PR's own state, not a ref"
    );
    drop(backend);

    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).unwrap();
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "pr-merge",
            path: dir.clone(),
            repo: repo_id,
            plan: std::sync::Arc::new(plan.clone()),
            remote,
        },
    )
    .unwrap();
    // What `merge_pr` returns when `gh` failed and the re-read could not say:
    // the children exited, the repository state did not.
    let (entry_repo, entry_before) = (dir.display().to_string(), plan.current.clone());
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        Box::new(move || {
            const EVIDENCE: &str = "gh failed and the state could not be re-read";
            let entry = OpLogEntry::new(
                "pr-merge",
                entry_repo.clone(),
                entry_before,
                OpOutcome::Unknown {
                    after: StateSummary {
                        head: "unknown".into(),
                        dirty: "unknown".into(),
                    },
                    evidence: EVIDENCE.to_string(),
                },
            )
            .with_worktree(Some(entry_repo));
            Ok(kagi_git::backend::recording::RunReport {
                result: Err(kagi_git::GitError::TerminationUnknown(
                    kagi_git::Termination::stopped(EVIDENCE),
                )),
                recording: kagi_git::backend::recording::finalize(entry),
                stash: None,
            })
        }),
    )
    .unwrap();
    let id = job.id();
    sessions.apply(job.run());
    assert!(
        !sessions.has_leases(),
        "both gh processes exited, so the scope is released at settlement"
    );
    assert_eq!(sessions.reconcile_ids(), vec![id]);

    // The server says the PR is still open: readable, and not a merge.
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":null}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            !read.resolved(),
            "an open PR is not a merged one: {}",
            read.observation
        );
        assert!(kagi::app::acknowledge(&mut sessions, read).is_err());
    }
    // The server cannot be asked at all. This is the answer that must never
    // pass for "merged" — nor for "not merged" (ADR-0177).
    {
        let _gh = FakeGh::answering(&bin, "echo 'could not connect' >&2; exit 1");
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            !read.resolved(),
            "an unreadable server confirms nothing: {}",
            read.observation
        );
        assert!(
            read.observation.contains("unreadable"),
            "and it must not be reported as an answer: {}",
            read.observation
        );
        assert!(kagi::app::acknowledge(&mut sessions, read).is_err());
    }
    // Merged: the promise is kept, and only now can the scope be closed.
    {
        let _gh = FakeGh::answering(&bin, r#"echo '{"mergedAt":"2026-09-07T00:00:00Z"}'"#);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            read.resolved(),
            "the server confirms the merge: {}",
            read.observation
        );
        kagi::app::acknowledge(&mut sessions, read).expect("a kept promise closes it");
    }
    assert!(sessions.reconcile_ids().is_empty());
}

/// #701 final review 4: both halves of the merge promise are read from the
/// repository the plan **named**, not from whatever a remote name resolves to
/// at reconcile time.
///
/// A remote name is not an address. After the promise is frozen, an external
/// `remote.<name>.url` change or a new `url.*.insteadOf` rewrite can point it
/// at a repository where the head ref never existed — and `Absent` succeeds
/// without observing the base repository at all. So this test moves the remote
/// under the frozen promise and still requires the frozen identity to be what
/// GitHub is asked about; a decoy repository answers with exactly the
/// false-confirm shape (merged, branch 404) so an unpinned read resolves.
#[test]
fn pr_merge_reads_the_repository_it_froze_not_a_remote_name() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    let bin = dir.join("fake-bin");
    let log = dir.join("gh-argv");
    git(dir, &["remote", "add", "upstream", BASE_URL]);

    let plan = kagi_git::github::plan_pr_merge(
        &one_pr(false),
        kagi_git::github::MergeMethod::Squash,
        true,
        "branch 'main'".into(),
    );
    let backend = Backend::open(dir).unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    let remote = backend.remote_expectation("pr-merge", &plan);
    assert_eq!(
        remote,
        vec![
            kagi_git::backend::remote_ref::RemoteExpectation::PullRequest {
                base_repo: BASE_REPO.to_string(),
                number: 501,
                expect: kagi_git::backend::remote_ref::PrExpect::Merged,
            },
            kagi_git::backend::remote_ref::RemoteExpectation::GithubRef {
                base_repo: BASE_REPO.to_string(),
                branch: HEAD_BRANCH.to_string(),
                expect: kagi_git::backend::remote_ref::RemoteExpect::Absent,
            },
        ],
        "both halves carry the repository, and neither carries a remote name"
    );
    drop(backend);

    let mut sessions = kagi::app::Sessions::new();
    let session = sessions.attach(dir.clone());
    let owner = sessions.attachment(session).unwrap();
    let approved = kagi::app::approve_run(
        &mut sessions,
        kagi::app::RunRequest {
            owner,
            name: "pr-merge",
            path: dir.clone(),
            repo: repo_id,
            plan: std::sync::Arc::new(plan.clone()),
            remote,
        },
    )
    .unwrap();
    let (entry_repo, entry_before) = (dir.display().to_string(), plan.current.clone());
    let job = kagi::app::prepare_run(
        &mut sessions,
        approved,
        Box::new(move || {
            const EVIDENCE: &str = "gh failed and the state could not be re-read";
            let entry = OpLogEntry::new(
                "pr-merge",
                entry_repo.clone(),
                entry_before,
                OpOutcome::Unknown {
                    after: StateSummary {
                        head: "unknown".into(),
                        dirty: "unknown".into(),
                    },
                    evidence: EVIDENCE.to_string(),
                },
            )
            .with_worktree(Some(entry_repo));
            Ok(kagi_git::backend::recording::RunReport {
                result: Err(kagi_git::GitError::TerminationUnknown(
                    kagi_git::Termination::stopped(EVIDENCE),
                )),
                recording: kagi_git::backend::recording::finalize(entry),
                stash: None,
            })
        }),
    )
    .unwrap();
    let id = job.id();
    sessions.apply(job.run());
    assert_eq!(sessions.reconcile_ids(), vec![id]);

    // The promise is frozen. Now move everything a remote name could be
    // resolved through — the configured URL, and an `insteadOf` rewrite of the
    // frozen URL itself. Neither may reach the read.
    git(dir, &["remote", "set-url", "upstream", DECOY_URL]);
    git(
        dir,
        &["config", &format!("url.{DECOY_URL}.insteadOf"), BASE_URL],
    );

    // Merged in the base repository, but its head branch — whose name a URL
    // path would truncate at the `#` — is still there.
    {
        let _gh = FakeGh::serving(&bin, &log, true);
        let read = kagi::app::read_reconcile(&sessions, id);

        // What was *asked* comes first: a question sent to the decoy, or one
        // that truncated `feature#x` into a URL path, is answered "merged,
        // branch gone" — so checking only the verdict would pass by luck.
        let asked = std::fs::read_to_string(&log).unwrap();
        assert!(
            asked.contains(&format!("-R {BASE_REPO}")),
            "the PR must be read in the frozen repository: {asked}"
        );
        assert!(
            asked.contains("graphql --hostname ghe.example")
                && asked.contains("owner=acme")
                && asked.contains("name=widgets")
                && asked.contains(&format!("ref=refs/heads/{HEAD_BRANCH}")),
            "and the branch must travel as a GraphQL variable, whole: {asked}"
        );
        assert!(
            !asked.contains("github.com"),
            "nothing may be asked of the decoy: {asked}"
        );

        let read = read.expect("the frozen repository answered");
        assert!(
            !read.resolved(),
            "the head branch is still in the base repository: {}",
            read.observation
        );
        assert!(kagi::app::acknowledge(&mut sessions, read).is_err());
    }

    // "Could not ask" is not "gone" (ADR-0177). A repository the token cannot
    // see answers with GraphQL errors and a null repository — the shape a REST
    // 404 would have flattened into "the ref is absent".
    // The last of these is the one a structural check alone cannot catch: a
    // perfectly shaped "the ref is not there" that the command *failed* to
    // establish. A failed read is not an observation (#701 review 6).
    for body in [
        r#"echo '{"data":{"repository":null},"errors":[{"type":"NOT_FOUND"}]}'; exit 1"#,
        r#"echo '{"data":{"repository":null}}'"#,
        "echo 'gh: HTTP 500' >&2; exit 1",
        r#"echo '{"data":{"repository":{"ref":null}}}'; exit 1"#,
    ] {
        let _gh = FakeGh::serving_ref(&bin, &log, body);
        assert!(
            kagi::app::read_reconcile(&sessions, id).is_err(),
            "a repository that cannot be read confirms nothing: {body}"
        );
    }
    // Both halves kept: only a structured `"ref": null` from a readable
    // repository is absence.
    {
        let _gh = FakeGh::serving(&bin, &log, false);
        let read = kagi::app::read_reconcile(&sessions, id).unwrap();
        assert!(
            read.resolved(),
            "merged and the branch is gone: {}",
            read.observation
        );
        kagi::app::acknowledge(&mut sessions, read).expect("a whole kept promise closes it");
    }
    assert!(sessions.reconcile_ids().is_empty());
}

/// #701 final review 2: a fork PR's head branch is not in the base repository,
/// so `refs/heads/<head>` there is absent from the start — and
/// `RemoteExpect::Absent` would happily call that "deleted". Upstream `gh`
/// makes it worse: it skips the remote head deletion for a cross-repository PR
/// but still deletes the *local* branch, which nothing here can account for
/// yet (#705). So `--delete-branch` is refused at plan time and no deletion
/// promise is frozen: fail closed rather than confirm a deletion nobody did.
#[test]
fn pr_merge_from_a_fork_refuses_to_promise_a_branch_deletion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let dir = &fixture.path;
    // A fork's head branch is in the fork, so `heads/<head>` in the base
    // repository is 404 from the start — the shape that makes an `Absent`
    // expectation confirm without observing anything.
    let plan = kagi_git::github::plan_pr_merge(
        &one_pr(true),
        kagi_git::github::MergeMethod::Squash,
        true,
        "branch 'main'".into(),
    );
    assert_eq!(
        plan.disposition,
        kagi_domain::plan_note::PlanDisposition::Blocked,
        "a fork merge that promises a branch deletion must not be confirmable"
    );
    assert!(
        plan.blockers.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Github(
                kagi_domain::plan_note::GithubNote::ForkDeletesBranch { branch }
            ) if branch == HEAD_BRANCH
        )),
        "and it must say why: {:?}",
        plan.blockers
    );
    assert!(
        matches!(
            plan.recovery.as_ref().map(|r| &r.kind),
            Some(kagi_domain::plan_note::RecoveryKind::Github(
                kagi_domain::plan_note::GithubRecovery::MergePr {
                    delete_branch: None,
                    ..
                }
            ))
        ),
        "a refused option freezes no promise: {:?}",
        plan.recovery
    );
    assert_eq!(
        Backend::open(dir)
            .unwrap()
            .remote_expectation("pr-merge", &plan),
        vec![
            kagi_git::backend::remote_ref::RemoteExpectation::PullRequest {
                base_repo: BASE_REPO.to_string(),
                number: 501,
                expect: kagi_git::backend::remote_ref::PrExpect::Merged,
            }
        ],
        "no ref that was never there may stand in for a deletion"
    );

    // Same PR without the option: an ordinary merge is still allowed.
    let plain = kagi_git::github::plan_pr_merge(
        &one_pr(true),
        kagi_git::github::MergeMethod::Squash,
        false,
        "branch 'main'".into(),
    );
    assert_eq!(
        plain.disposition,
        kagi_domain::plan_note::PlanDisposition::Ready
    );
}

#[path = "support/isolated.rs"]
mod test_support;
