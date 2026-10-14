#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output, init_repo, write_file};
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
    let before_refs = git_output(path, &["show-ref"]);
    let before_stash_reflog = std::fs::read(path.join(".git/logs/refs/stash")).unwrap();
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
                ("modified.txt".into(), ChangeKind::Modified),
                ("staged.txt".into(), ChangeKind::Added),
                ("untracked.txt".into(), ChangeKind::Added),
            ]
        );
        let index_only = plan
            .warnings
            .iter()
            .find_map(|note| match note {
                kagi_domain::plan_note::PlanNote::Stash(
                    kagi_domain::plan_note::StashNote::IndexOnlyNotRestored { files },
                ) => Some(files),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            index_only,
            &vec![kagi_domain::status::FileStatus {
                path: "index-only.txt".into(),
                change: ChangeKind::Modified,
            }]
        );
    }
    assert_eq!(
        std::fs::read(path.join(".git/index")).unwrap(),
        before_index
    );
    assert_eq!(git_output(path, &["show-ref"]), before_refs);
    assert_eq!(
        std::fs::read(path.join(".git/logs/refs/stash")).unwrap(),
        before_stash_reflog
    );
    assert_eq!(std::fs::read(path.join("modified.txt")).unwrap(), b"base\n");
    assert!(!path.join("untracked.txt").exists());
    assert!(kagi_git::plan_stash_apply(&mut repo, 9)
        .unwrap()
        .preview_files
        .is_empty());
}

#[cfg(unix)]
#[test]
fn stash_manifest_preserves_file_symlink_type_changes_both_directions() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    init_repo(path, "main");
    write_file(path, "file-to-link", "base\n");
    symlink("file-to-link", path.join("link-to-file")).unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "base types"]);
    std::fs::remove_file(path.join("file-to-link")).unwrap();
    symlink("link-to-file", path.join("file-to-link")).unwrap();
    std::fs::remove_file(path.join("link-to-file")).unwrap();
    write_file(path, "link-to-file", "regular now\n");
    git(path, &["stash", "push", "-qm", "type changes"]);
    let mut repo = git2::Repository::open(path).unwrap();
    for plan in [
        kagi_git::plan_stash_apply(&mut repo, 0).unwrap(),
        kagi_git::plan_stash_pop(&mut repo, 0).unwrap(),
    ] {
        assert_eq!(plan.preview_files.len(), 2);
        assert!(plan
            .preview_files
            .iter()
            .all(|file| file.change == ChangeKind::TypeChange));
        assert_eq!(
            plan.preview_files[0].path,
            std::path::PathBuf::from("file-to-link")
        );
        assert_eq!(
            plan.preview_files[1].path,
            std::path::PathBuf::from("link-to-file")
        );
    }
}
