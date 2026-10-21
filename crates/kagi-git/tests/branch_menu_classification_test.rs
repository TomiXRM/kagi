//! #1699: unreadable merge evidence is unknown, not a known-unmerged tip.
use git2::Repository;
use kagi_git::{Backend, CommitId};
use tempfile::TempDir;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

fn missing_object_stays_an_error(revision: &str, squash_probe: bool) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    git_fixture::init_repo(dir, "main");
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "main"]);
    let tip = CommitId(git_output(dir, &["rev-parse", "feature"]));
    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let refs = git_output(dir, &["show-ref"]);
    assert!(!Backend::open(dir)
        .unwrap()
        .branch_is_merged_into_head("feature", &tip, &head)
        .unwrap());

    let oid = git_output(dir, &["rev-parse", revision]);
    let object = dir.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    let held = object.with_extension("held");
    std::fs::rename(&object, &held).unwrap();
    if squash_probe {
        let repo = Repository::open(dir).unwrap();
        let head = git2::Oid::from_str(&head).unwrap();
        let tip = git2::Oid::from_str(&tip.0).unwrap();
        assert!(!repo.graph_descendant_of(head, tip).unwrap());
        assert!(kagi_git::ops::squash_merged_as(&repo, tip, head).is_err());
    }
    let backend = Backend::open(dir).unwrap();
    assert!(backend
        .branch_is_merged_into_head("feature", &tip, &head)
        .is_err());
    assert!(backend.plan_delete_branch("feature").is_err());
    drop(backend);
    std::fs::rename(&held, &object).unwrap();

    assert!(!Backend::open(dir)
        .unwrap()
        .branch_is_merged_into_head("feature", &tip, &head)
        .unwrap());
    assert_eq!(git_output(dir, &["show-ref"]), refs);
    assert!(git_output(dir, &["status", "--porcelain"]).is_empty());
}

#[test]
fn branch_menu_merge_classification_preserves_commit_read_failures() {
    if !test_support::run_isolated() {
        return;
    }
    for revision in ["feature", "feature^"] {
        missing_object_stays_an_error(revision, false);
    }
}

#[test]
fn branch_menu_merge_classification_preserves_squash_tree_read_failures() {
    if !test_support::run_isolated() {
        return;
    }
    missing_object_stays_an_error("feature^{tree}", true);
}

#[test]
fn branch_menu_orphan_is_unmerged_and_delete_plan_keeps_unmerged_warning() {
    if !test_support::run_isolated() {
        return;
    }
    use kagi_domain::plan_note::{BranchNote, PlanNote};

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    git_fixture::init_repo(dir, "main");
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
    git(dir, &["checkout", "-q", "--orphan", "orphan"]);
    write_file(dir, "orphan.txt", "independent history\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "independent root"]);
    git(dir, &["checkout", "-q", "main"]);
    let tip = CommitId(git_output(dir, &["rev-parse", "orphan"]));
    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let refs = git_output(dir, &["show-ref"]);
    let repo = Repository::open(dir).unwrap();
    let tip_oid = git2::Oid::from_str(&tip.0).unwrap();
    let head_oid = git2::Oid::from_str(&head).unwrap();
    let no_base = repo.merge_base(head_oid, tip_oid).unwrap_err();
    assert_eq!(no_base.code(), git2::ErrorCode::NotFound);
    assert_eq!(no_base.class(), git2::ErrorClass::Merge);
    assert_eq!(
        kagi_git::ops::squash_merged_as(&repo, tip_oid, head_oid).unwrap(),
        None
    );
    let backend = Backend::open(dir).unwrap();
    assert!(!backend
        .branch_is_merged_into_head("orphan", &tip, &head)
        .unwrap());
    let plan = backend.plan_delete_branch("orphan").unwrap();
    assert!(plan.blockers.is_empty());
    assert!(plan.recovery.as_ref().is_some_and(|recovery| recovery
        .commands
        .iter()
        .any(|command| command.contains(&tip.0))));
    assert!(plan
        .warnings
        .iter()
        .any(|warning| matches!(warning, PlanNote::Branch(BranchNote::DeleteUnmerged { .. }))));
    assert_eq!(git_output(dir, &["show-ref"]), refs);
    assert!(git_output(dir, &["status", "--porcelain"]).is_empty());
}
