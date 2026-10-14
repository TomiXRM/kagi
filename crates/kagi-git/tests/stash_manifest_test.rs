#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, init_repo, write_file};
use kagi_domain::status::ChangeKind;

#[test]
fn stash_manifest_contains_worktree_index_and_untracked_without_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    init_repo(path, "main");
    write_file(path, "modified.txt", "base\n");
    write_file(path, "deleted.txt", "base\n");
    write_file(path, "index-only.txt", "base\n");
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "base"]);
    write_file(path, "modified.txt", "modified\n");
    std::fs::remove_file(path.join("deleted.txt")).unwrap();
    write_file(path, "staged.txt", "staged\n");
    write_file(path, "index-only.txt", "index\n");
    git(path, &["add", "staged.txt", "index-only.txt"]);
    write_file(path, "index-only.txt", "base\n");
    write_file(path, "untracked.txt", "untracked\n");
    git(path, &["stash", "push", "-u", "-qm", "manifest"]);
    let before_index = std::fs::read(path.join(".git/index")).unwrap();
    let before_refs = git(path, &["show-ref"]);
    let mut repo = git2::Repository::open(path).unwrap();
    for plan in [
        kagi_git::plan_stash_apply(&mut repo, 0).unwrap(),
        kagi_git::plan_stash_pop(&mut repo, 0).unwrap(),
    ] {
        let files: Vec<_> = plan
            .preview_files
            .iter()
            .map(|f| (f.path.to_string_lossy().into_owned(), f.change.clone()))
            .collect();
        assert_eq!(
            files,
            vec![
                ("deleted.txt".into(), ChangeKind::Deleted),
                ("index-only.txt".into(), ChangeKind::Modified),
                ("modified.txt".into(), ChangeKind::Modified),
                ("staged.txt".into(), ChangeKind::Added),
                ("untracked.txt".into(), ChangeKind::Added),
            ]
        );
    }
    assert_eq!(
        std::fs::read(path.join(".git/index")).unwrap(),
        before_index
    );
    assert_eq!(git(path, &["show-ref"]), before_refs);
    assert_eq!(std::fs::read(path.join("modified.txt")).unwrap(), b"base\n");
    assert!(!path.join("untracked.txt").exists());
    assert!(kagi_git::plan_stash_apply(&mut repo, 9)
        .unwrap()
        .preview_files
        .is_empty());
}
