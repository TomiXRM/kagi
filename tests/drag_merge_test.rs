//! T-DNDMERGE-001 / ADR-0079: drag-and-drop branch merge — integration tests.
//!
//! These exercise the drag-merge path end-to-end at the layer the GUI dispatches
//! to: the action-layer *validation gate* (mirroring `KagiApp::start_merge_from_drag`
//! / `validate_merge_from_drag`) followed by the *same* backend planner the gesture
//! reuses (`Backend::plan_merge_branch`).  Dropping a branch never executes git;
//! the gesture only produces the preview plan that the user must confirm.
//!
//! All repos are created inside `TempDir`s (no network, no writes to real repos).

#[path = "support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_merge_branch;
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
    let err = drag_merge_gate("ghost", &branches_main_feature(), &[], false)
        .expect_err("unknown source must be rejected");
    assert!(err.contains("not a branch"), "got: {}", err);
}

#[test]
fn drag_merge_while_busy_is_rejected() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let err = drag_merge_gate("feature", &branches_main_feature(), &[], true)
        .expect_err("a drag while busy must be rejected");
    assert!(!err.is_empty());
}

#[test]
fn drag_merge_fast_forward_produces_ff_plan() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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

/// The one path the drag tests above never take: actually confirming the plan.
/// A diverged merge must produce a real merge commit — two parents, both
/// sides' content in the tree and on disk, and the current branch advanced to
/// it (the target branch left where it was).
#[test]
fn execute_merge_branch_creates_a_two_parent_merge_commit() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = init_repo();
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan merge");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert_eq!(kind, MergeKind::MergeCommit);

    let main_before = rev_parse(dir, "main");
    let feature_before = rev_parse(dir, "feature");

    let merged = execute_merge_branch(&git2::Repository::open(dir).unwrap(), "feature")
        .expect("execute_merge_branch");

    // The current branch advanced to the new commit; HEAD is still on main.
    assert_eq!(rev_parse(dir, "HEAD"), merged.0);
    assert_eq!(rev_parse(dir, "main"), merged.0);
    assert_eq!(
        rev_parse(dir, "feature"),
        feature_before,
        "merging must not move the target branch"
    );

    // A real merge commit: both sides are parents, in first-parent order.
    let repo = git2::Repository::open(dir).expect("open repo");
    let commit = repo
        .find_commit(git2::Oid::from_str(&merged.0).unwrap())
        .expect("merge commit");
    assert_eq!(commit.parent_count(), 2, "a merge commit has two parents");
    assert_eq!(commit.parent_id(0).unwrap().to_string(), main_before);
    assert_eq!(commit.parent_id(1).unwrap().to_string(), feature_before);

    // The merged content is in the tree AND checked out on disk.
    let tree = commit.tree().expect("tree");
    for name in ["base.txt", "main.txt", "feature.txt"] {
        assert!(
            tree.get_name(name).is_some(),
            "{} must be in the merge tree",
            name
        );
        assert!(dir.join(name).exists(), "{} must be checked out", name);
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("feature.txt")).unwrap(),
        "feature\n",
        "the merged-in side's content must land in the working tree"
    );
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("rev-parse");
    assert!(out.status.success(), "git rev-parse {} failed", rev);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[path = "support/isolated.rs"]
mod test_support;

// #590: remote sources use exactly the same Backend boundary as local sources.
fn remote_source_fixture(diverged: bool) -> (TempDir, git2::Oid, git2::Oid) {
    let tmp = init_repo();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let base = repo.head().unwrap().peel_to_commit().unwrap();
    let sig = repo.signature().unwrap();
    let make_commit = |name: &str| {
        let mut tree = repo.treebuilder(Some(&base.tree().unwrap())).unwrap();
        tree.insert(name, repo.blob(name.as_bytes()).unwrap(), 0o100644)
            .unwrap();
        repo.commit(
            None,
            &sig,
            &sig,
            name,
            &repo.find_tree(tree.write().unwrap()).unwrap(),
            &[&base],
        )
        .unwrap()
    };
    let source = make_commit("remote.txt");
    let target = if diverged {
        make_commit("target.txt")
    } else {
        base.id()
    };
    repo.branch("target", &repo.find_commit(target).unwrap(), false)
        .unwrap();
    repo.reference(
        "refs/remotes/origin/source",
        source,
        false,
        "fixture remote ref",
    )
    .unwrap();
    // Any implicit fetch would fail; FETCH_HEAD also proves no fetch was attempted.
    repo.remote("origin", "file:///kagi-590-no-such-remote")
        .unwrap();
    std::fs::write(repo.path().join("FETCH_HEAD"), "last explicit fetch\n").unwrap();
    (tmp, source, target)
}

fn merge_remote_operation() -> kagi_git::Operation {
    kagi_git::Operation::MergeIntoBranch {
        source: "origin/source".into(),
        target: "target".into(),
    }
}

#[test]
fn remote_source_merges_into_non_head_without_checkout_fetch_or_local_source() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for diverged in [false, true] {
        let (tmp, source, old_target) = remote_source_fixture(diverged);
        let repo = git2::Repository::open(tmp.path()).unwrap();
        let head = repo.head().unwrap().target().unwrap();
        write_file(tmp.path(), "base.txt", "dirty stays here\n");
        let index = std::fs::read(repo.path().join("index")).unwrap();
        let mut backend =
            Backend::open_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::human(false))
                .unwrap();
        let op = merge_remote_operation();
        let plan = backend.plan(&op).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        let note = plan
            .warnings
            .iter()
            .find_map(|note| match note {
                kagi_domain::plan_note::PlanNote::Merge(
                    note @ kagi_domain::plan_note::MergeNote::IntoRemoteSource { .. },
                ) => Some(note),
                _ => None,
            })
            .expect("remote freshness warning");
        for text in [
            note.message_en(),
            kagi_ui_core::i18n::plan::merge::note_ja(note),
        ] {
            assert!(text.contains("refs/remotes/origin/source"));
            assert!(text.contains(&source.to_string()));
            assert!(text.contains("fetch"));
        }
        assert!(note.message_en().contains("last fetch"));
        backend.preflight_check(&plan).unwrap();
        let report = backend.run_recorded(&op, &plan);
        assert!(report.result.is_ok(), "{:?}", report.result);
        assert!(matches!(
            report.recording.entry().outcome,
            kagi_git::OpOutcome::Success { .. }
        ));
        let repo = git2::Repository::open(tmp.path()).unwrap();
        let merged = repo
            .find_reference("refs/heads/target")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        if diverged {
            assert_eq!(merged.parent_count(), 2);
            assert_eq!(merged.parent_id(0).unwrap(), old_target);
            assert_eq!(merged.parent_id(1).unwrap(), source);
        } else {
            assert_eq!(merged.id(), source);
        }
        assert_eq!(repo.head().unwrap().target(), Some(head));
        assert_eq!(
            repo.find_reference("refs/remotes/origin/source")
                .unwrap()
                .target(),
            Some(source)
        );
        assert!(repo.find_branch("source", git2::BranchType::Local).is_err());
        assert!(repo
            .find_branch("origin/source", git2::BranchType::Local)
            .is_err());
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("base.txt")).unwrap(),
            "dirty stays here\n"
        );
        assert!(!tmp.path().join("remote.txt").exists());
        assert_eq!(
            std::fs::read_to_string(repo.path().join("FETCH_HEAD")).unwrap(),
            "last explicit fetch\n"
        );
        assert_eq!(
            kagi_git::oplog::read_oplog_tail_for_repo(tmp.path(), 100).len(),
            1
        );
    }
}

#[test]
fn remote_source_does_not_bypass_worktree_occupancy() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (tmp, _, old_target) = remote_source_fixture(false);
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("linked");
    let mut backend =
        Backend::open_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let op = merge_remote_operation();
    let approved = backend.plan(&op).unwrap();
    git(
        tmp.path(),
        &["worktree", "add", path.to_str().unwrap(), "target"],
    );
    let blocked = backend.plan(&op).unwrap();
    assert!(blocked.blockers.iter().any(|note| matches!(
        note,
        kagi_domain::plan_note::PlanNote::Merge(
            kagi_domain::plan_note::MergeNote::IntoCheckedOutElsewhere { .. }
        )
    )));
    assert!(backend.run_recorded(&op, &approved).result.is_err());
    let repo = git2::Repository::open(tmp.path()).unwrap();
    assert_eq!(
        repo.find_reference("refs/heads/target").unwrap().target(),
        Some(old_target)
    );
}

#[test]
fn remote_source_tip_movement_requires_new_confirmation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (tmp, source, old_target) = remote_source_fixture(false);
    let mut backend =
        Backend::open_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let op = merge_remote_operation();
    let approved = backend.plan(&op).unwrap();
    let repo = git2::Repository::open(tmp.path()).unwrap();
    let parent = repo.find_commit(source).unwrap();
    let sig = repo.signature().unwrap();
    repo.commit(
        Some("refs/remotes/origin/source"),
        &sig,
        &sig,
        "remote advances",
        &parent.tree().unwrap(),
        &[&parent],
    )
    .unwrap();
    let report = backend.run_recorded(&op, &approved);
    assert!(report.result.unwrap_err().to_string().contains("re-plan"));
    assert_eq!(
        repo.find_reference("refs/heads/target").unwrap().target(),
        Some(old_target)
    );
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(tmp.path(), 100).len(),
        1
    );
}
