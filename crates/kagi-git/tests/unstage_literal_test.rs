#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod isolated;

use git_fixture::{git, init_repo, write_file};
use kagi_git::Backend;
use std::path::{Path, PathBuf};

fn literal_unstage(batch: bool) {
    let cases = [
        ("a[b].txt", "ab.txt"),
        ("star*.txt", "star-neighbour.txt"),
        ("question?.txt", "questionX.txt"),
        ("#leading.txt", "leading.txt"),
        ("back\\slash.txt", "back/slash.txt"),
    ];
    for (selected, neighbour) in cases {
        if cfg!(windows)
            && (selected.contains('*') || selected.contains('?') || selected.contains('\\'))
        {
            continue;
        }
        for state in ["modified", "added", "deleted", "unborn"] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            init_repo(dir, "main");
            std::fs::create_dir_all(dir.join("back")).unwrap();
            write_file(dir, neighbour, "neighbour base\n");
            write_file(dir, "also.txt", "also base\n");
            if state != "added" && state != "unborn" {
                write_file(dir, selected, "selected base\n");
            }
            if state != "unborn" {
                git(dir, &["add", "."]);
                git(dir, &["commit", "-qm", "base"]);
            }
            if state == "deleted" {
                std::fs::remove_file(dir.join(selected)).unwrap();
            } else {
                write_file(dir, selected, "selected staged\n");
            }
            write_file(dir, neighbour, "neighbour staged\n");
            write_file(dir, "also.txt", "also staged\n");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    dir.join(neighbour),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            git(dir, &["add", "."]);
            let repo = git2::Repository::open(dir).unwrap();
            let neighbour_before = repo
                .index()
                .unwrap()
                .get_path(Path::new(neighbour), 0)
                .unwrap();
            let worktree_before: Vec<_> = [selected, neighbour, "also.txt"]
                .into_iter()
                .map(|name| std::fs::read(dir.join(name)).ok())
                .collect();
            let backend = Backend::open(dir).unwrap();
            if batch {
                assert_eq!(
                    backend
                        .unstage_files(&[PathBuf::from(selected), PathBuf::from("also.txt")])
                        .unwrap(),
                    2
                );
            } else {
                backend.unstage_file(Path::new(selected)).unwrap();
            }
            let mut index = repo.index().unwrap();
            index.read(true).unwrap();
            let neighbour_after = index.get_path(Path::new(neighbour), 0).unwrap();
            assert_eq!(
                (neighbour_after.id, neighbour_after.mode),
                (neighbour_before.id, neighbour_before.mode),
                "unselected {neighbour} changed: {selected}, batch={batch}, state={state}"
            );
            let selected_after = index.get_path(Path::new(selected), 0);
            if state == "added" || state == "unborn" {
                assert!(
                    selected_after.is_none(),
                    "selected addition remains staged: {selected}"
                );
            } else {
                let head = repo.head().unwrap().peel_to_tree().unwrap();
                let entry = head.get_path(Path::new(selected)).unwrap();
                let selected_after = selected_after.unwrap();
                assert_eq!(
                    (selected_after.id, selected_after.mode),
                    (entry.id(), entry.filemode() as u32)
                );
            }
            if batch {
                let also = index.get_path(Path::new("also.txt"), 0);
                if state == "unborn" {
                    assert!(also.is_none());
                } else {
                    let head = repo.head().unwrap().peel_to_tree().unwrap();
                    let entry = head.get_path(Path::new("also.txt")).unwrap();
                    let also = also.unwrap();
                    assert_eq!((also.id, also.mode), (entry.id(), entry.filemode() as u32));
                }
            }
            assert_eq!(
                worktree_before,
                [selected, neighbour, "also.txt"]
                    .into_iter()
                    .map(|name| std::fs::read(dir.join(name)).ok())
                    .collect::<Vec<_>>()
            );
            let status = backend.working_tree_status().unwrap();
            assert!(status
                .staged
                .iter()
                .any(|file| file.path == Path::new(neighbour)));
            assert!(!status
                .staged
                .iter()
                .any(|file| file.path == Path::new(selected)));
        }
    }
}

#[test]
fn unstage_literal_single_preserves_neighbour() {
    if isolated::run_isolated() {
        literal_unstage(false);
    }
}

#[test]
fn unstage_literal_bulk_preserves_neighbour() {
    if isolated::run_isolated() {
        literal_unstage(true);
    }
}
