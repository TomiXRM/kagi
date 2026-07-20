//! T-DNDMERGE-001 / ADR-0079: drag-and-drop branch merge — integration tests.
//!
//! These exercise the drag-merge path end-to-end at the layer the GUI dispatches
//! to: the action-layer *validation gate* (mirroring `KagiApp::start_merge_from_drag`
//! / `validate_merge_from_drag`) followed by the *same* backend planner the gesture
//! reuses (`Backend::plan_merge_branch`).  Dropping a branch never executes git;
//! the gesture only produces the preview plan that the user must confirm.
//!
//! All repos are created inside `TempDir`s (no network, no writes to real repos).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use kagi_git::ops::MergeKind;
use kagi_git::Backend;

// ── Validation gate (a copy of the action-layer rule under test) ──
//
// `KagiApp::start_merge_from_drag` delegates the obvious rejections to the pure
// helper `validate_merge_from_drag(source, branches, remotes, busy)`.  That
// helper lives inside `src/ui` (a binary-only module), so we re-state the
// contract here and assert the drag path honours it before reaching the planner.
// `branches` is the `(name, is_head)` list as held in `KagiApp::branches`;
// `remotes` is the list of `remote/name` refs (`KagiApp::remote_branches`) —
// an upstream-only branch is a valid source, merged directly via its ref.
fn drag_merge_gate(
    source: &str,
    branches: &[(String, bool)],
    remotes: &[String],
    busy: bool,
) -> Result<(), String> {
    if busy {
        return Err("another operation is in progress".to_string());
    }
    match branches.iter().find(|(n, _)| n == source) {
        Some((_, true)) => Err(format!(
            "Branch '{}' is already the current branch.",
            source
        )),
        Some((_, false)) => Ok(()),
        None if remotes.iter().any(|n| n == source) => Ok(()),
        None => Err(format!("Branch '{}' is not a branch.", source)),
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} exited with {:?}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

/// A repo on `main` with one base commit. Returns the kept-alive TempDir.
fn init_repo() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
    tmp
}

/// The branch list the sidebar/action layer would hold for a repo whose HEAD is
/// `main` and which also has a `feature` branch.
fn branches_main_feature() -> Vec<(String, bool)> {
    vec![("main".to_string(), true), ("feature".to_string(), false)]
}

/// Local-branch list for a repo whose only local branch is the current `main`
/// (used by the remote-only merge test, where `feature` exists only on a remote).
fn branches_main_only() -> Vec<(String, bool)> {
    vec![("main".to_string(), true)]
}

#[test]
fn drag_merge_same_branch_is_rejected_before_planning() {
    // Dragging the current branch onto itself must be rejected by the gate, so
    // the planner is never even reached (drop is a trigger, not an execution).
    let err = drag_merge_gate("main", &branches_main_feature(), &[], false)
        .expect_err("same-branch drag must be rejected");
    assert!(
        err.contains("main") && err.contains("current branch"),
        "reason should explain same-branch rejection: {}",
        err
    );
}

#[test]
fn drag_merge_unknown_source_is_rejected() {
    let err = drag_merge_gate("ghost", &branches_main_feature(), &[], false)
        .expect_err("unknown source must be rejected");
    assert!(err.contains("not a branch"), "got: {}", err);
}

#[test]
fn drag_merge_while_busy_is_rejected() {
    let err = drag_merge_gate("feature", &branches_main_feature(), &[], true)
        .expect_err("a drag while busy must be rejected");
    assert!(!err.is_empty());
}

#[test]
fn drag_merge_fast_forward_produces_ff_plan() {
    let tmp = init_repo();
    let dir = tmp.path();

    // feature is ahead of main; HEAD on main → fast-forward.
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);

    // Gate accepts (feature != current, exists, not busy).
    drag_merge_gate("feature", &branches_main_feature(), &[], false).expect("gate should accept");

    // Drag reuses the SAME planner the menu uses; nothing is executed.
    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan merge");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert_eq!(kind, MergeKind::FastForward);
    assert_eq!(plan.title.message_en(), "Merge feature into main");
}

#[test]
fn drag_merge_diverged_produces_merge_commit_plan() {
    let tmp = init_repo();
    let dir = tmp.path();

    // main and feature diverge → a merge commit (no fast-forward).
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    drag_merge_gate("feature", &branches_main_feature(), &[], false).expect("gate should accept");

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan merge");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert_eq!(kind, MergeKind::MergeCommit);
    assert_eq!(plan.title.message_en(), "Merge feature into main");
}

#[test]
fn drag_merge_dirty_working_tree_warns_in_plan() {
    let tmp = init_repo();
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);

    // Make the working tree dirty (uncommitted modification) on main.
    write_file(dir, "base.txt", "base modified\n");

    // The gate still accepts (dirty-WT is the planner's job, not the gate's).
    drag_merge_gate("feature", &branches_main_feature(), &[], false).expect("gate should accept");

    let backend = Backend::open(dir).expect("open backend");
    let (plan, _kind) = backend.plan_merge_branch("feature").expect("plan merge");
    // ADR-0105: a dirty tracked working tree is now a BLOCKER (mirrors
    // cherry-pick / revert) — merge writes conflict markers into the user's
    // uncommitted edits, and `git merge --abort` would discard both. The plan
    // must refuse execution rather than warn-and-allow.
    assert!(
        plan.blockers.iter().any(
            |b| b.message_en().to_lowercase().contains("working tree has")
                && b.message_en().to_lowercase().contains("stash or commit")
        ),
        "expected a dirty-working-tree BLOCKER, got blockers: {:?}",
        plan.blockers
    );
}

#[test]
fn drag_merge_remote_only_branch_produces_plan() {
    // An upstream-only branch: a remote-tracking ref `origin/feature` exists but
    // there is NO local `feature`. Dragging it onto the current branch must be
    // accepted by the gate and the planner must resolve the remote ref directly
    // (no local branch is created) — the user can then confirm the merge.
    let tmp = init_repo();
    let dir = tmp.path();

    // Build the would-be remote tip on a temporary local branch...
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    let feature_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .expect("rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(dir, &["checkout", "-q", "main"]);
    // ...then publish it as a remote-tracking ref and drop the local branch, so
    // `origin/feature` is the only reference to that commit.
    git(
        dir,
        &["update-ref", "refs/remotes/origin/feature", &feature_sha],
    );
    git(dir, &["branch", "-qD", "feature"]);

    // Gate: source is not in local branches, but IS a known remote ref → accept.
    let remotes = vec!["origin/feature".to_string()];
    drag_merge_gate("origin/feature", &branches_main_only(), &remotes, false)
        .expect("gate should accept a remote-only branch");

    // The planner resolves the remote ref directly (find_branch Remote / revparse).
    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend
        .plan_merge_branch("origin/feature")
        .expect("plan merge of remote-only branch");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    // main is an ancestor of origin/feature → fast-forward.
    assert_eq!(kind, MergeKind::FastForward);
    assert_eq!(plan.title.message_en(), "Merge origin/feature into main");
}
