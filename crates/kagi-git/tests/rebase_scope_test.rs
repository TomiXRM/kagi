#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};

#[test]
fn rebase_plan_discloses_three_commits_in_replay_order_without_writes() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    init_repo(path, "main");
    write_file(path, "base", "base");
    commit_all(path, "base");
    git(path, &["checkout", "-qb", "feature"]);
    let mut expected = Vec::new();
    for n in 1..=3 {
        write_file(path, &format!("feature-{n}"), "feature");
        commit_all(path, &format!("replay {n}"));
        let oid = git_output(path, &["rev-parse", "HEAD"]);
        expected.push(format!("{}  replay {n}", &oid[..8]));
    }
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "main", "main");
    commit_all(path, "new base");
    git(path, &["checkout", "-q", "feature"]);
    let before = git_output(path, &["show-ref"]);
    let repo = git2::Repository::open(path).unwrap();
    let plan = kagi_git::ops::plan_rebase_current_onto(&repo, "main").unwrap();
    assert_eq!(
        plan.preview_commits, expected,
        "rebase scope must disclose all three replay candidates in oldest-first order"
    );
    assert_eq!(git_output(path, &["show-ref"]), before);
    assert!(git_output(path, &["status", "--porcelain"]).is_empty());
}

fn range_fixture(count: usize, divergent: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    init_repo(path, "main");
    write_file(path, "base", "base");
    commit_all(path, "base");
    git(path, &["checkout", "-qb", "feature"]);
    for n in 0..count {
        write_file(path, "feature", &n.to_string());
        commit_all(path, &format!("candidate {n}"));
    }
    if divergent {
        git(path, &["checkout", "-q", "main"]);
        write_file(path, "target", "target");
        commit_all(path, "target work");
        git(path, &["checkout", "-q", "feature"]);
    }
    dir
}

#[test]
fn rebase_scope_caps_preview_and_retains_true_total() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = range_fixture(150, true);
    let repo = git2::Repository::open(dir.path()).unwrap();
    let plan = kagi_git::ops::plan_rebase_current_onto(&repo, "main").unwrap();
    assert_eq!(plan.preview_commits.len(), 100);
    assert_eq!(plan.rebase_scope.as_ref().unwrap().total_commits, 150);
    assert!(plan.preview_commits[0].ends_with("candidate 0"));
    assert!(plan.preview_commits[99].ends_with("candidate 99"));
}

#[test]
fn rebase_linear_ancestor_is_noop_like_git() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = range_fixture(3, false);
    let path = dir.path();
    let before = git_output(path, &["rev-parse", "HEAD"]);
    let repo = git2::Repository::open(path).unwrap();
    let plan = kagi_git::ops::plan_rebase_current_onto(&repo, "main").unwrap();
    assert!(plan.preview_commits.is_empty());
    assert_eq!(plan.rebase_scope.as_ref().unwrap().total_commits, 0);
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    assert!(plan.blockers.iter().any(|note| matches!(
        note,
        kagi_domain::plan_note::PlanNote::Rebase(
            kagi_domain::plan_note::RebaseNote::AlreadyUpToDate { .. }
        )
    )));
    git(path, &["rebase", "main"]);
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), before);
}

#[test]
fn rebase_fast_forward_without_replay_has_no_conflict_warning() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = range_fixture(0, true);
    let repo = git2::Repository::open(dir.path()).unwrap();
    let plan = kagi_git::ops::plan_rebase_current_onto(&repo, "main").unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert_eq!(plan.rebase_scope.as_ref().unwrap().total_commits, 0);
    assert!(plan.preview_commits.is_empty());
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
}

#[test]
fn rebase_refuses_target_drift_without_mutating_refs() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = range_fixture(3, true);
    let path = dir.path();
    let mut backend =
        kagi_git::Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let op = kagi_git::Operation::RebaseCurrentOnto {
        onto: "main".into(),
    };
    let plan = backend.plan(&op).unwrap();
    let scope = plan.rebase_scope.as_ref().unwrap();
    assert_eq!(scope.current_ref, "refs/heads/feature");
    assert_eq!(scope.current_oid, git_output(path, &["rev-parse", "HEAD"]));
    assert_eq!(scope.onto_ref, "main");
    assert_eq!(scope.onto_oid, git_output(path, &["rev-parse", "main"]));
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "target", "moved target");
    commit_all(path, "target moved after approval");
    git(path, &["checkout", "-q", "feature"]);
    let before = git_output(path, &["show-ref"]);
    let error = backend.run(&op, &plan).unwrap_err();
    assert!(
        error.to_string().contains("changed since planning"),
        "{error}"
    );
    assert_eq!(git_output(path, &["show-ref"]), before);
}

#[test]
fn successful_rebase_retains_legacy_receipt_summary() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = range_fixture(3, true);
    let path = dir.path();
    let mut backend =
        kagi_git::Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let op = kagi_git::Operation::RebaseCurrentOnto {
        onto: "main".into(),
    };
    let plan = backend.plan(&op).unwrap();
    assert_eq!(plan.current.head, "branch: feature");
    assert_eq!(plan.current.dirty, "clean");
    assert_eq!(
        plan.predicted.head, "branch: feature",
        "successful receipt input must retain its semantic branch summary"
    );
    assert_eq!(
        plan.predicted.dirty, "clean",
        "display-only uncertainty must not become the persisted after state"
    );
    assert!(matches!(
        backend.run(&op, &plan).unwrap(),
        kagi_git::OperationOutcome::Rebase(_)
    ));
    assert_eq!(
        git_output(path, &["symbolic-ref", "HEAD"]),
        "refs/heads/feature"
    );
    assert!(git_output(path, &["status", "--porcelain"]).is_empty());
}
