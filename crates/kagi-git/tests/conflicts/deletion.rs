use super::*;
use kagi_domain::conflict_family::{ConflictProgress, Resolution};
use kagi_git::{backend::ExecutionPolicy, Backend};

pub(super) fn fixture(rename: bool, delete_current: bool) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    init_repo(root, "main");
    write_file(root, "renamed-src.ts", "export const value = 1;\n");
    write_file(root, "unrelated.txt", "preserve\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["checkout", "-qb", "incoming"]);
    if rename {
        git(root, &["mv", "renamed-src.ts", "incoming.ts"]);
    } else if delete_current {
        write_file(root, "renamed-src.ts", "export const value = 2;\n");
    } else {
        git(root, &["rm", "renamed-src.ts"]);
    }
    git(root, &["commit", "-qam", "incoming"]);
    git(root, &["checkout", "-q", "main"]);
    if rename {
        git(root, &["mv", "renamed-src.ts", "current.ts"]);
    } else if delete_current {
        git(root, &["rm", "renamed-src.ts"]);
    } else {
        write_file(root, "renamed-src.ts", "export const value = 3;\n");
    }
    git(root, &["commit", "-qam", "current"]);
    assert!(!git_allow_fail(root, &["merge", "incoming"]));
    tmp
}

pub(super) fn plan(root: &Path, path: &Path) -> kagi_git::backend::conflict_ops::ConflictPlan {
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(buffer
        .file_resolutions(path)
        .unwrap()
        .contains(&Resolution::Delete));
    let request = Backend::conflict_delete_request(
        snapshot.observation.revision,
        &buffer,
        path,
        snapshot.observation.kind,
    )
    .unwrap();
    Backend::plan_recorded_conflict(root, request).unwrap()
}

fn execute(plan: &kagi_git::backend::conflict_ops::ConflictPlan) -> ConflictProgress {
    Backend::run_recorded_conflict(plan, ExecutionPolicy::human(false), None)
        .evidence
        .progress
}

#[test]
fn rename_rename_dd_deletes_source_preserves_destinations_and_reaches_continue() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(true, false);
    let root = tmp.path();
    let path = Path::new("renamed-src.ts");
    let approved = plan(root, path);
    assert_eq!(
        approved.path_changes().remove_index,
        vec![path.to_path_buf()]
    );
    assert_eq!(execute(&approved), ConflictProgress::Verified);
    let repo = Repository::open(root).unwrap();
    assert!((0..=3).all(|stage| repo.index().unwrap().get_path(path, stage).is_none()));
    assert!(!root.join(path).exists());
    for name in ["current.ts", "incoming.ts"] {
        assert_eq!(
            std::fs::read_to_string(root.join(name)).unwrap(),
            "export const value = 1;\n"
        );
        let backend = Backend::open(root).unwrap();
        let mut buffer = backend.resolution_buffer_from_repo().unwrap();
        let file = Path::new(name);
        if let Some(routes) = buffer.file_resolutions(file) {
            let choice = if routes.contains(&Resolution::Current) {
                ResolutionChoice::Current
            } else {
                ResolutionChoice::Incoming
            };
            buffer.apply_choice(file, choice).unwrap();
            let snapshot = backend.conflict_snapshot().unwrap().unwrap();
            let request = Backend::conflict_save_request(
                snapshot.observation.revision,
                &buffer,
                file,
                snapshot.observation.kind,
                "",
            )
            .unwrap();
            let saved = Backend::plan_recorded_conflict(root, request).unwrap();
            assert_eq!(execute(&saved), ConflictProgress::Verified);
        }
    }
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(backend
        .continue_blockers(&snapshot.session, &buffer)
        .unwrap()
        .is_empty());
    backend
        .plan_conflict_continue_route(
            &snapshot.session,
            &buffer,
            "main",
            &snapshot.observation.revision,
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("unrelated.txt")).unwrap(),
        "preserve\n"
    );
}

#[test]
fn modify_delete_routes_remove_only_approved_file() {
    if !test_support::run_isolated() {
        return;
    }
    for delete_current in [false, true] {
        let tmp = fixture(false, delete_current);
        write_file(tmp.path(), "untracked.txt", "untouched\n");
        let path = Path::new("renamed-src.ts");
        let approved = plan(tmp.path(), path);
        assert_eq!(
            approved.path_changes().remove_worktree,
            vec![path.to_path_buf()]
        );
        assert_eq!(execute(&approved), ConflictProgress::Verified);
        assert!(!tmp.path().join(path).exists());
        assert!(!Repository::open(tmp.path())
            .unwrap()
            .index()
            .unwrap()
            .has_conflicts());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("untracked.txt")).unwrap(),
            "untouched\n"
        );
    }
}

#[test]
fn edit_after_delete_plan_is_refused_without_removing_anything() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(false, false);
    let path = Path::new("renamed-src.ts");
    let approved = plan(tmp.path(), path);
    write_file(tmp.path(), "renamed-src.ts", "external edit\n");
    assert_eq!(execute(&approved), ConflictProgress::NotStarted);
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(path)).unwrap(),
        "external edit\n"
    );
    assert!(Repository::open(tmp.path())
        .unwrap()
        .index()
        .unwrap()
        .has_conflicts());
}

#[test]
fn dd_refuses_unrelated_artifact_and_directory() {
    if !test_support::run_isolated() {
        return;
    }
    for directory in [false, true] {
        let tmp = fixture(true, false);
        let path = Path::new("renamed-src.ts");
        if directory {
            std::fs::create_dir(tmp.path().join(path)).unwrap();
            write_file(tmp.path(), "renamed-src.ts/child", "preserve\n");
        } else {
            write_file(tmp.path(), "renamed-src.ts", "unrelated replacement\n");
        }
        let backend = Backend::open(tmp.path()).unwrap();
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let request = Backend::conflict_delete_request(
            snapshot.observation.revision,
            &buffer,
            path,
            snapshot.observation.kind,
        )
        .unwrap();
        assert!(Backend::plan_recorded_conflict(tmp.path(), request).is_err());
        assert!(tmp.path().join(path).exists());
    }
}

#[cfg(unix)]
#[test]
fn deletion_unlinks_symlink_without_following_target() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(false, false);
    let path = Path::new("renamed-src.ts");
    std::fs::remove_file(tmp.path().join(path)).unwrap();
    std::os::unix::fs::symlink("unrelated.txt", tmp.path().join(path)).unwrap();
    let approved = plan(tmp.path(), path);
    assert_eq!(execute(&approved), ConflictProgress::Verified);
    assert!(std::fs::symlink_metadata(tmp.path().join(path)).is_err());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("unrelated.txt")).unwrap(),
        "preserve\n"
    );
}

#[test]
fn dd_exact_ancestor_artifact_is_removed() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(true, false);
    write_file(tmp.path(), "renamed-src.ts", "export const value = 1;\n");
    let approved = plan(tmp.path(), Path::new("renamed-src.ts"));
    assert_eq!(approved.path_changes().remove_worktree.len(), 1);
    assert_eq!(execute(&approved), ConflictProgress::Verified);
    assert!(!tmp.path().join("renamed-src.ts").exists());
    assert!(tmp.path().join("current.ts").exists());
    assert!(tmp.path().join("incoming.ts").exists());
}

#[test]
fn deletion_draft_digest_cannot_be_reused_as_empty_text() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(false, false);
    let approved = plan(tmp.path(), Path::new("renamed-src.ts"));
    let mut request = approved.request().clone();
    let kagi_domain::conflict_family::ConflictRequest::Save { draft, .. } = &mut request else {
        panic!("expected Save");
    };
    *draft = kagi_domain::conflict_family::ConflictDraft::Text {
        bytes: Vec::new(),
        origins: Vec::new(),
    };
    assert!(Backend::plan_recorded_conflict(tmp.path(), request).is_err());
    assert!(tmp.path().join("renamed-src.ts").exists());
}

#[test]
fn deletion_refuses_gitlink_stages() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(false, false);
    let path = Path::new("renamed-src.ts");
    let repo = Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(path, 2).unwrap();
    entry.mode = 0o160000;
    entry.id = repo.head().unwrap().target().unwrap();
    index.add(&entry).unwrap();
    index.write().unwrap();
    let backend = Backend::open(tmp.path()).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let request = Backend::conflict_delete_request(
        snapshot.observation.revision,
        &buffer,
        path,
        snapshot.observation.kind,
    )
    .unwrap();
    assert!(Backend::plan_recorded_conflict(tmp.path(), request).is_err());
    assert!(tmp.path().join(path).exists());
}

#[test]
fn deletion_refuses_unaffected_index_drift_without_losing_external_update() {
    if !test_support::run_isolated() {
        return;
    }
    for rename in [false, true] {
        for change in ["oid", "mode", "stage", "added", "removed"] {
            let tmp = fixture(rename, false);
            let approved = plan(tmp.path(), Path::new("renamed-src.ts"));
            let repo = Repository::open(tmp.path()).unwrap();
            let mut index = repo.index().unwrap();
            let mut entry = index.get_path(Path::new("unrelated.txt"), 0).unwrap();
            match change {
                "oid" => entry.id = repo.blob(b"external staged update\n").unwrap(),
                "mode" => entry.mode = 0o100755,
                "stage" => {
                    index.remove(Path::new("unrelated.txt"), 0).unwrap();
                    entry.flags |= 2 << 12;
                }
                "added" => entry.path = b"external-added".to_vec(),
                "removed" => {
                    index.remove(Path::new("unrelated.txt"), 0).unwrap();
                }
                _ => unreachable!(),
            }
            if change != "removed" {
                index.add(&entry).unwrap();
            }
            index.write().unwrap();
            let before = std::fs::read(repo.path().join("index")).unwrap();
            assert_eq!(execute(&approved), ConflictProgress::NotStarted, "{change}");
            assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
            assert_eq!(
                std::fs::read(tmp.path().join("unrelated.txt")).unwrap(),
                b"preserve\n"
            );
            if rename {
                assert!(tmp.path().join("current.ts").exists());
                assert!(tmp.path().join("incoming.ts").exists());
            } else {
                assert!(tmp.path().join("renamed-src.ts").exists());
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn dd_refuses_symlink_parent_without_touching_outside_ancestor_artifact() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = fixture(true, false);
    let repo = Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    let source = Path::new("renamed-src.ts");
    let mut ancestor = index.get_path(source, 1).unwrap();
    index.conflict_remove(source).unwrap();
    ancestor.path = b"parent/renamed-src.ts".to_vec();
    index.add(&ancestor).unwrap();
    index.write().unwrap();
    let outside = TempDir::new().unwrap();
    write_file(
        outside.path(),
        "renamed-src.ts",
        "export const value = 1;\n",
    );
    std::os::unix::fs::symlink(outside.path(), tmp.path().join("parent")).unwrap();
    let before = std::fs::read(repo.path().join("index")).unwrap();
    let backend = Backend::open(tmp.path()).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let request = Backend::conflict_delete_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("parent/renamed-src.ts"),
        snapshot.observation.kind,
    )
    .unwrap();
    let error = Backend::plan_recorded_conflict(tmp.path(), request).unwrap_err();
    assert!(error.to_string().contains("parent"), "{error}");
    assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
    assert_eq!(
        std::fs::read(outside.path().join("renamed-src.ts")).unwrap(),
        b"export const value = 1;\n"
    );
    assert!(tmp.path().join("current.ts").exists());
    assert!(tmp.path().join("incoming.ts").exists());
}
