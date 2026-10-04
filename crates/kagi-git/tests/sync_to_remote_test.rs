//! #536 slice 1 (ADR-0215): `Operation::SyncToRemote` makes the branch, index
//! and working tree match the fetched upstream while every local commit and
//! change is retained under `refs/kagi/backups/` — and those backups restore
//! the original state exactly.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kagi_domain::plan_note::{PlanNote, SyncNote};
use kagi_git::backend::ExecutionPolicy;
use kagi_git::oplog::OpOutcome;
use kagi_git::{Backend, Operation, OperationOutcome};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct LogDir {
    _dir: TempDir,
    prev: Option<String>,
}

impl LogDir {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let prev = std::env::var("KAGI_LOG_DIR").ok();
        std::env::set_var("KAGI_LOG_DIR", dir.path());
        Self { _dir: dir, prev }
    }
}

impl Drop for LogDir {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

/// `origin` bare with main = base → up1 → up2. Local clone whose main is
/// base → up1 → local1 (one commit ahead, one behind). Returns (tmp, local).
fn fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let seed = tmp.path().join("seed");
    std::fs::create_dir(&seed).unwrap();
    init_repo(&seed, "main");
    write_file(&seed, "a.txt", "base\n");
    write_file(&seed, "b.txt", "base\n");
    commit_all(&seed, "base");
    write_file(&seed, "a.txt", "up1\n");
    commit_all(&seed, "up1");
    let origin = tmp.path().join("origin.git");
    git(
        &seed,
        &["clone", "-q", "--bare", ".", origin.to_str().unwrap()],
    );
    let local = tmp.path().join("local");
    git(
        &seed,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            local.to_str().unwrap(),
        ],
    );
    // Upstream advances.
    write_file(&seed, "a.txt", "up2\n");
    write_file(&seed, "c.txt", "up2\n");
    commit_all(&seed, "up2");
    git(
        &seed,
        &["push", "-q", origin.to_str().unwrap(), "main:main"],
    );
    // Local diverges.
    write_file(&local, "b.txt", "local1\n");
    commit_all(&local, "local1");
    git(&local, &["fetch", "-q", "origin"]);
    (tmp, local)
}

fn backend(path: &Path) -> Backend {
    let mut b = Backend::open_with_policy(path, ExecutionPolicy::human(false)).unwrap();
    b.set_auto_snapshot(false);
    b
}

fn op() -> Operation {
    Operation::SyncToRemote {
        branch: "main".into(),
    }
}

fn tip(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
}

fn status(dir: &Path) -> String {
    git_output(dir, &["status", "--porcelain", "--untracked-files=all"])
}

/// Dirty every classification: staged edit + unstaged edit on the same file,
/// an unstaged edit, a staged deletion, a staged new file, an untracked file
/// in a subdirectory, and an ignored file (which must be left alone).
fn make_dirty(local: &Path) {
    write_file(local, ".gitignore", "ignored.log\n");
    git(local, &["add", ".gitignore"]);
    git(local, &["commit", "-qm", "ignore rules"]);
    write_file(local, "ignored.log", "keep me\n");
    write_file(local, "a.txt", "staged\n");
    git(local, &["add", "a.txt"]);
    write_file(local, "a.txt", "staged-then-edited\n");
    write_file(local, "b.txt", "unstaged\n");
    git(local, &["rm", "-q", "--cached", "b.txt"]);
    write_file(local, "new.txt", "staged-new\n");
    git(local, &["add", "new.txt"]);
    std::fs::create_dir_all(local.join("sub")).unwrap();
    write_file(local, "sub/untracked.txt", "untracked\n");
}

#[test]
fn syncs_and_restores_everything_from_the_two_backups() {
    if !test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, local) = fixture();
    make_dirty(&local);
    let before_status = status(&local);
    let before_tip = tip(&local, "main");
    let upstream = tip(&local, "origin/main");
    let before_index = git_output(&local, &["ls-files", "-s"]);
    let before_a_wt = std::fs::read_to_string(local.join("a.txt")).unwrap();

    let mut b = backend(&local);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(plan.destructive);
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Sync(SyncNote::AbandonsCommits { count: 2, .. })
        )),
        "{:?}",
        plan.warnings
    );
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Sync(SyncNote::PreservesWork {
                staged: 3,
                unstaged: 1,
                untracked: 2
            })
        )),
        "{:?}",
        plan.warnings
    );
    let recovery = plan.recovery.as_ref().expect("recovery");
    assert_eq!(recovery.commands.len(), 2, "{:?}", recovery.commands);
    // Planning wrote nothing.
    assert_eq!(status(&local), before_status);
    assert!(!git_succeeds(
        &local,
        &["rev-parse", "--verify", "refs/kagi/backups"]
    ));

    let report = b.run_recorded(&op(), &plan);
    let OperationOutcome::SyncToRemote {
        from,
        to,
        tip_backup,
        work_backup,
        removed_untracked,
        ..
    } = report.result.expect("run")
    else {
        panic!("outcome");
    };
    assert_eq!(
        (from.as_str(), to.as_str()),
        (before_tip.as_str(), upstream.as_str())
    );
    let work = work_backup.expect("dirty tree is retained");
    // sub/untracked.txt only: b.txt was untracked (deleted from the index)
    // but the target tracks it, so the checkout overwrote it in place and the
    // retained blob no longer matched — never deleted.
    assert_eq!(removed_untracked, 1);
    // Synced: branch at upstream, HEAD still main, tree clean except ignored.
    assert_eq!(tip(&local, "main"), upstream);
    assert_eq!(
        git_output(&local, &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    // The ignore rule lived in a local-only commit, so after the sync
    // `ignored.log` is plain untracked — but it was never retained, removed
    // or rewritten: ignored files are not touched (ADR-0215).
    assert_eq!(
        status(&local),
        "?? ignored.log",
        "nothing staged/unstaged; only the ex-ignored file"
    );
    assert!(!git_succeeds(
        &local,
        &["cat-file", "-e", &format!("{}:ignored.log", work.reference)]
    ));
    assert_eq!(
        std::fs::read_to_string(local.join("ignored.log")).unwrap(),
        "keep me\n"
    );
    assert_eq!(
        std::fs::read_to_string(local.join("a.txt")).unwrap(),
        "up2\n"
    );
    assert!(local.join("c.txt").exists());
    assert!(!local.join("sub/untracked.txt").exists());
    assert!(!local.join("new.txt").exists());
    // Receipt.
    let entry = report.recording.entry().clone();
    assert_eq!(entry.op, "sync-to-remote");
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    assert_eq!(
        entry.backup_refs,
        vec![tip_backup.clone(), work.reference.clone()]
    );
    assert_eq!(entry.recovery.len(), 2);
    // The recovery commands from the plan are the real ones.
    assert_eq!(
        recovery.commands[0],
        format!("git update-ref 'refs/heads/main' '{tip_backup}'")
    );
    assert_eq!(
        recovery.commands[1],
        format!("git stash apply --index '{}'", work.reference)
    );

    // Restore, exactly as the receipt says, with plain git.
    git(&local, &["update-ref", "refs/heads/main", &tip_backup]);
    git(&local, &["read-tree", "-m", "-u", "HEAD"]);
    assert_eq!(tip(&local, "main"), before_tip);
    git(&local, &["stash", "apply", "--index", &work.reference]);
    assert_eq!(
        status(&local),
        before_status,
        "every classification restored"
    );
    assert_eq!(
        git_output(&local, &["ls-files", "-s"]),
        before_index,
        "index entries restored"
    );
    assert_eq!(
        std::fs::read_to_string(local.join("a.txt")).unwrap(),
        before_a_wt
    );
    assert_eq!(
        std::fs::read_to_string(local.join("sub/untracked.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(
        std::fs::read_to_string(local.join("ignored.log")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn clean_tree_keeps_only_the_tip_backup() {
    if !test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, local) = fixture();
    let before_tip = tip(&local, "main");
    let mut b = backend(&local);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(!plan
        .warnings
        .iter()
        .any(|w| matches!(w, PlanNote::Sync(SyncNote::PreservesWork { .. }))));
    let report = b.run_recorded(&op(), &plan);
    let OperationOutcome::SyncToRemote {
        work_backup,
        tip_backup,
        ..
    } = report.result.expect("run")
    else {
        panic!("outcome");
    };
    assert!(work_backup.is_none());
    assert_eq!(
        report.recording.entry().backup_refs,
        vec![tip_backup.clone()]
    );
    assert_eq!(tip(&local, &tip_backup), before_tip);
    assert_eq!(tip(&local, "main"), tip(&local, "origin/main"));
    assert_eq!(status(&local), "");
}

#[test]
fn refuses_when_upstream_moved_after_planning() {
    if !test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (tmp, local) = fixture();
    make_dirty(&local);
    let before_status = status(&local);
    let before_tip = tip(&local, "main");
    let mut b = backend(&local);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty());
    // Upstream advances and is fetched between plan and confirm.
    let seed = tmp.path().join("seed");
    write_file(&seed, "a.txt", "up3\n");
    commit_all(&seed, "up3");
    git(
        &seed,
        &[
            "push",
            "-q",
            tmp.path().join("origin.git").to_str().unwrap(),
            "main:main",
        ],
    );
    git(&local, &["fetch", "-q", "origin"]);
    let report = b.run_recorded(&op(), &plan);
    let err = report.result.unwrap_err().to_string();
    assert!(err.contains("re-plan"), "{err}");
    assert_eq!(tip(&local, "main"), before_tip);
    assert_eq!(status(&local), before_status, "nothing touched");
    assert!(
        report.recording.entry().backup_refs.is_empty(),
        "no backup for a refusal"
    );
}

#[test]
fn blockers_no_upstream_not_fetched_conflict_elsewhere_in_sync() {
    if !test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (tmp, local) = fixture();
    let mut b = backend(&local);
    // No upstream.
    git(&local, &["branch", "lonely"]);
    let plan = b
        .plan(&Operation::SyncToRemote {
            branch: "lonely".into(),
        })
        .unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Sync(SyncNote::NoUpstream { .. }))),
        "{:?}",
        plan.blockers
    );
    // Upstream configured, tracking ref missing.
    git(
        &local,
        &["branch", "--set-upstream-to=origin/main", "lonely"],
    );
    git(&local, &["update-ref", "-d", "refs/remotes/origin/main"]);
    let plan = b
        .plan(&Operation::SyncToRemote {
            branch: "lonely".into(),
        })
        .unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Sync(SyncNote::UpstreamNotFetched { .. }))),
        "{:?}",
        plan.blockers
    );
    git(&local, &["fetch", "-q", "origin"]);
    // Checked out in another worktree.
    let wt = tmp.path().join("wt");
    git(
        &local,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "lonely"],
    );
    let plan = b
        .plan(&Operation::SyncToRemote {
            branch: "lonely".into(),
        })
        .unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Sync(SyncNote::CheckedOutElsewhere { .. }))),
        "{:?}",
        plan.blockers
    );
    assert!(b
        .run(
            &Operation::SyncToRemote {
                branch: "lonely".into()
            },
            &plan
        )
        .is_err());
    // Conflict in progress (merge of a conflicting branch).
    git(&local, &["checkout", "-q", "-b", "other", "main~1"]);
    write_file(&local, "b.txt", "other\n");
    commit_all(&local, "other-b");
    git(&local, &["checkout", "-q", "main"]);
    assert!(
        !git_succeeds(&local, &["merge", "-q", "other"]),
        "merge must conflict"
    );
    let plan = b.plan(&op()).unwrap();
    assert!(
        plan.blockers.iter().any(|x| matches!(
            x,
            PlanNote::Sync(SyncNote::OperationInProgress { .. })
                | PlanNote::Sync(SyncNote::ConflictedFiles { .. })
        )),
        "{:?}",
        plan.blockers
    );
    assert!(b.run(&op(), &plan).is_err());
    git(&local, &["merge", "--abort"]);
    // Already in sync and clean → no-op blocker, no backups.
    git(
        &local,
        &["update-ref", "refs/heads/main", &tip(&local, "origin/main")],
    );
    git(&local, &["read-tree", "-m", "-u", "HEAD"]);
    let plan = b.plan(&op()).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Sync(SyncNote::AlreadyInSync { .. }))),
        "{:?}",
        plan.blockers
    );
}

#[test]
fn refuses_when_the_tree_got_dirty_after_a_clean_plan() {
    if !test_support::run_isolated() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, local) = fixture();
    let mut b = backend(&local);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty());
    write_file(&local, "a.txt", "late edit\n");
    let err = b.run(&op(), &plan).unwrap_err().to_string();
    assert!(err.contains("re-plan"), "{err}");
    assert_eq!(
        std::fs::read_to_string(local.join("a.txt")).unwrap(),
        "late edit\n"
    );
}
