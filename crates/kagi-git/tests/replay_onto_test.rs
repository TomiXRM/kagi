//! #344 slice 1 (ADR-0211): `Operation::ReplayOnto` moves a branch that is
//! checked out in another worktree by ref update only, through the plan →
//! confirm → preflight → execute → verify → oplog pipeline, and refuses every
//! way the picture could have changed since planning.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kagi_domain::plan_note::{PlanNote, RebaseNote};
use kagi_git::backend::ExecutionPolicy;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_git::{Backend, Operation, OperationOutcome};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// `main`: base → main1 → main2. `feat` (from base): feat1 → feat2, checked
/// out in `<tmp>/wt-feat`. Returns (tmp, main dir, feat worktree dir).
fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let main = tmp.path().join("main");
    std::fs::create_dir(&main).unwrap();
    init_repo(&main, "main");
    write_file(&main, "a.txt", "base\n");
    commit_all(&main, "base");
    write_file(&main, "a.txt", "main1\n");
    commit_all(&main, "main1");
    git(&main, &["branch", "feat", "HEAD~1"]);
    write_file(&main, "c.txt", "main2\n");
    commit_all(&main, "main2");
    let wt = tmp.path().join("wt-feat");
    git(&main, &["worktree", "add", wt.to_str().unwrap(), "feat"]);
    write_file(&wt, "b.txt", "feat1\n");
    commit_all(&wt, "feat1");
    write_file(&wt, "b.txt", "feat2\n");
    commit_all(&wt, "feat2");
    (tmp, main, wt)
}

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

/// `git replay` exists from 2.44; the feature gate hides it below that, so
/// the suite is a no-op there rather than a failure.
fn replay_available() -> bool {
    let ok = kagi_git::cli::GitFeatures::detected().replay_onto;
    if !ok {
        eprintln!(
            "skipping: git replay needs git 2.44+ (found {:?})",
            kagi_git::cli::git_version()
        );
    }
    ok
}

fn backend(path: &Path) -> Backend {
    let mut b = Backend::open_with_policy(path, ExecutionPolicy::human(false)).unwrap();
    b.set_auto_snapshot(false);
    b
}

fn op() -> Operation {
    Operation::ReplayOnto {
        branch: "feat".into(),
        onto: "main".into(),
    }
}

fn tip(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
}

fn index_hash(dir: &Path) -> String {
    git_output(dir, &["ls-files", "-s"])
}

#[test]
fn replays_a_branch_checked_out_elsewhere_without_touching_any_worktree() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, wt) = fixture();
    // Dirty the *current* worktree: replay must not care.
    write_file(&main, "a.txt", "dirty\n");
    let main_status = git_output(&main, &["status", "--porcelain"]);
    let main_index = index_hash(&main);
    let wt_index = index_hash(&wt);
    let feat_before = tip(&main, "feat");
    let main_tip = tip(&main, "main");

    let mut b = backend(&main);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(plan.destructive, "history rewrite is two-stage");
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Rebase(RebaseNote::ReplayUpdates { count: 1, .. })
        )),
        "{:?}",
        plan.warnings
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| matches!(w, PlanNote::Rebase(RebaseNote::ReplayWorktreeStale { .. }))),
        "the checked-out worktree is named"
    );
    let updates: Vec<&String> = plan
        .preview_commits
        .iter()
        .filter(|l| l.starts_with("update "))
        .collect();
    assert_eq!(updates.len(), 1, "{:?}", plan.preview_commits);
    assert!(updates[0].starts_with("update refs/heads/feat "));
    assert!(updates[0].ends_with(&feat_before), "old = plan-time tip");
    // Planning wrote nothing.
    assert_eq!(tip(&main, "feat"), feat_before);
    assert_eq!(git_output(&main, &["status", "--porcelain"]), main_status);

    let report = b.run_recorded(&op(), &plan);
    let outcome = report.result.expect("run");
    let OperationOutcome::ReplayOnto {
        branch,
        from,
        to,
        reference,
        backups,
    } = outcome
    else {
        panic!("unexpected outcome");
    };
    assert_eq!(
        (branch.as_str(), from.as_str()),
        ("feat", feat_before.as_str())
    );
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].reference, "refs/heads/feat");
    assert_eq!(backups[0].backup, reference);
    assert!(
        plan.preview_commits
            .iter()
            .any(|l| l.starts_with(&format!("verify refs/heads/main {main_tip}"))),
        "onto is pinned in the script: {:?}",
        plan.preview_commits
    );
    assert_eq!(tip(&main, "feat"), to, "feat moved to the replayed tip");
    assert_eq!(tip(&main, "feat~2"), main_tip, "…which sits on main");
    assert_eq!(
        git_output(&main, &["log", "--format=%s", "feat"]),
        "feat2\nfeat1\nmain2\nmain1\nbase"
    );
    // Neither worktree was touched: the dirty main stays dirty, the feat
    // worktree's index is exactly what it was (its HEAD follows the ref).
    assert_eq!(git_output(&main, &["status", "--porcelain"]), main_status);
    assert_eq!(index_hash(&main), main_index);
    assert_eq!(index_hash(&wt), wt_index);
    assert_eq!(tip(&wt, "HEAD"), to);
    // Backup ref retains the old tip; the oplog carries it.
    assert_eq!(tip(&main, &reference), feat_before);
    let entry = report.recording.entry().clone();
    assert_eq!(entry.op, "replay-onto");
    assert_eq!(entry.backup_refs, vec![reference.clone()]);
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    let tail = read_oplog_tail_for_repo(&main, 5);
    assert_eq!(tail.last().map(|e| e.op.as_str()), Some("replay-onto"));
    // Restore from the backup ref, exactly as the recovery text says.
    git(&main, &["update-ref", "refs/heads/feat", &reference]);
    assert_eq!(tip(&main, "feat"), feat_before);
}

#[test]
fn refuses_when_the_branch_moved_after_planning() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, wt) = fixture();
    let mut b = backend(&main);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty());
    // feat advances in its worktree between plan and confirm.
    write_file(&wt, "b.txt", "feat3\n");
    commit_all(&wt, "feat3");
    let moved = tip(&main, "feat");
    let report = b.run_recorded(&op(), &plan);
    let err = report.result.unwrap_err().to_string();
    // Whichever layer fires first — the re-plan's changed title, the
    // preflight's old-value check, or git's own update-ref CAS — the result
    // is a refusal with nothing written.
    assert!(
        err.contains("re-plan")
            || err.contains("moved after planning")
            || err.contains("cannot lock ref"),
        "{err}"
    );
    assert_eq!(tip(&main, "feat"), moved, "nothing was written");
    assert!(
        report.recording.entry().backup_refs.is_empty(),
        "no backup is taken for a refused run"
    );
}

#[test]
fn refuses_a_dirty_checked_out_worktree_at_plan_and_at_preflight() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, wt) = fixture();
    let mut b = backend(&main);
    // Plan-time: dirty → blocker.
    write_file(&wt, "b.txt", "uncommitted\n");
    let plan = b.plan(&op()).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Rebase(RebaseNote::ReplayWorktreeDirty { .. }))),
        "{:?}",
        plan.blockers
    );
    assert!(b.run(&op(), &plan).is_err());
    // Preflight-time: clean at plan, dirtied before confirm → refused.
    git(&wt, &["checkout", "--", "b.txt"]);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty());
    let before = tip(&main, "feat");
    write_file(&wt, "b.txt", "uncommitted\n");
    let err = b.run(&op(), &plan).unwrap_err().to_string();
    // `Backend::run` re-plans before dispatch, so the dirty worktree is caught
    // as a fresh blocker (the preflight's own check is the second line of
    // defence); either way nothing is written.
    assert!(
        err.contains("uncommitted changes") || err.contains("plan has blockers"),
        "{err}"
    );
    assert_eq!(tip(&main, "feat"), before);
}

#[test]
fn refuses_a_range_with_merges_and_an_up_to_date_branch() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, wt) = fixture();
    let mut b = backend(&main);
    // Merge main into feat: the range main..feat now contains a merge.
    git(&wt, &["merge", "-q", "--no-ff", "-m", "merge-main", "main"]);
    let plan = b.plan(&op()).expect("plan");
    assert!(
        plan.blockers.iter().any(|x| matches!(
            x,
            PlanNote::Rebase(RebaseNote::ReplayRangeHasMerges { count: 1, .. })
        )),
        "{:?}",
        plan.blockers
    );
    assert!(b.run(&op(), &plan).is_err());
    // Already on main: nothing to do is a blocker, not a success.
    let main_tip = tip(&main, "main");
    git(&wt, &["checkout", "-q", "--detach"]);
    git(&main, &["branch", "-f", "feat", &main_tip]);
    let plan = b.plan(&op()).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Rebase(RebaseNote::ReplayNothingToDo { .. }))),
        "{:?}",
        plan.blockers
    );
}

#[test]
fn conflicts_are_a_blocker_and_leave_no_state() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, _wt) = fixture();
    // main edits b.txt differently from feat.
    write_file(&main, "b.txt", "main-b\n");
    commit_all(&main, "main-b");
    let mut b = backend(&main);
    let plan = b.plan(&op()).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|x| matches!(x, PlanNote::Rebase(RebaseNote::ReplayConflicts { .. }))),
        "{:?}",
        plan.blockers
    );
    assert!(b.run(&op(), &plan).is_err());
    for state in ["rebase-merge", "rebase-apply", "sequencer", "MERGE_HEAD"] {
        assert!(
            !main.join(".git").join(state).exists(),
            "{state} was created"
        );
    }
}

#[test]
fn refuses_when_onto_moved_after_planning() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, _wt) = fixture();
    let feat_before = tip(&main, "feat");
    // The reviewer's scenario: HEAD is detached elsewhere (so head_at_plan is
    // satisfied), `main` moves after planning, and the moved refs themselves
    // are unchanged — only the onto pin can refuse this.
    git(&main, &["checkout", "-q", "--detach", "main~1"]);
    let mut b = backend(&main);
    let plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let main_before = tip(&main, "main");
    git(&main, &["branch", "-f", "main", "main~1"]);
    assert_ne!(tip(&main, "main"), main_before);
    let err = b.run(&op(), &plan).unwrap_err().to_string();
    assert!(err.contains("moved after planning"), "{err}");
    assert_eq!(tip(&main, "feat"), feat_before, "nothing written");
}

#[test]
fn backs_up_every_ref_a_multi_ref_script_moves() {
    if !test_support::run_isolated() || !replay_available() {
        return;
    }
    let _guard = ENV_LOCK.lock().unwrap();
    let _log = LogDir::new();
    let (_tmp, main, _wt) = fixture();
    // Plain `--onto` moves only the named tip (git 2.50.1: a descendant
    // branch inside the range is left alone unless `--contained`). The
    // execute contract still has to back up every line a plan carries, so
    // feed it the two-line script `--contained` prints.
    git(&main, &["branch", "feat-child", "feat~1"]);
    let child_before = tip(&main, "feat-child");
    let feat_before = tip(&main, "feat");
    let mut b = backend(&main);
    let mut plan = b.plan(&op()).expect("plan");
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let contained = git_output(
        &main,
        &[
            "-c",
            "replay.refAction=print",
            "replay",
            "--contained",
            "--onto",
            "main",
            "main..feat",
        ],
    );
    let child_line = contained
        .lines()
        .find(|l| l.contains("refs/heads/feat-child"))
        .expect("--contained prints the child");
    plan.preview_commits.push(child_line.to_string());
    let report = b.run_recorded(&op(), &plan);
    let OperationOutcome::ReplayOnto { backups, .. } = report.result.expect("run") else {
        panic!("outcome");
    };
    assert_eq!(backups.len(), 2, "{backups:?}");
    let entry = report.recording.entry().clone();
    assert_eq!(entry.backup_refs.len(), 2);
    assert_eq!(entry.recovery.len(), 2, "one recovery handle per moved ref");
    for b in &backups {
        let expected = if b.reference == "refs/heads/feat" {
            &feat_before
        } else {
            &child_before
        };
        assert_eq!(&b.old, expected);
        assert_eq!(
            &tip(&main, &b.backup),
            expected,
            "{} backup holds its old tip",
            b.reference
        );
        assert_ne!(tip(&main, &b.reference), *expected, "{} moved", b.reference);
    }
}
