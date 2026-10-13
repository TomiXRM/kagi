//! Volatile stash indices must never substitute another entry after approval.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, init_repo, write_file};
use kagi_domain::plan_note::{PlanNote, StashNote};

#[test]
fn same_count_stash_replacement_blocks_apply_pop_and_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    init_repo(path, "main");
    write_file(path, "README.md", "base\n");
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "base"]);
    write_file(path, "README.md", "selected\n");
    git(path, &["stash", "push", "-qm", "selected message"]);
    let mut repo = git2::Repository::open(path).unwrap();
    let plans = [
        kagi_git::plan_stash_apply(&mut repo, 0).unwrap(),
        kagi_git::plan_stash_pop(&mut repo, 0).unwrap(),
        kagi_git::plan_stash_drop(&mut repo, 0).unwrap(),
    ];
    let expected = plans[0].stash_identity.as_ref().unwrap().oids[0].clone();
    git(path, &["stash", "drop", "-q", "stash@{0}"]);
    write_file(path, "README.md", "replacement\n");
    git(path, &["stash", "push", "-qm", "replacement message"]);
    let before = kagi_git::snapshot(&mut repo, 100).unwrap();
    for plan in plans {
        assert_eq!(plan.stash_identity.as_ref().unwrap().oids[0], expected);
        let error = kagi_git::preflight_check_stash(&mut repo, &plan, 1).unwrap_err();
        assert!(
            matches!(error.blocker(), Some(PlanNote::Stash(StashNote::TargetChanged { index: 0, expected: oid })) if oid == &expected)
        );
    }
    assert_eq!(
        kagi_git::snapshot(&mut repo, 100).unwrap().stashes,
        before.stashes
    );
    assert_eq!(std::fs::read(path.join("README.md")).unwrap(), b"base\n");
}
