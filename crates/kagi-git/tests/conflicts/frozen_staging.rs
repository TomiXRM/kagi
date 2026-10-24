//! Frozen staging must preserve Git's mode/filter semantics through recorded Save.
use super::*;
use kagi_domain::conflict_family::ConflictProgress;
use kagi_git::{backend::ExecutionPolicy, oplog::OpOutcome, Backend};

fn recorded_save(path: &Path, text: &str) {
    let backend = Backend::open(path).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer.set_manual_text(Path::new("file.txt"), text).unwrap();
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("file.txt"),
        snapshot.observation.kind,
        "",
    )
    .unwrap();
    let plan = Backend::plan_recorded_conflict(path, request).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), None);
    assert_eq!(
        report.evidence.progress,
        ConflictProgress::Verified,
        "{report:?}"
    );
    assert!(
        matches!(report.recording.entry().outcome, OpOutcome::Success { .. }),
        "{report:?}"
    );
    assert!(report.blocker.is_none(), "{report:?}");
}

#[test]
fn recorded_save_plans_and_verifies_configured_filemode() {
    if !test_support::run_isolated() {
        return;
    }
    for trust_filemode in [false, true] {
        for (ours, worktree) in [(0o100755, 0o644), (0o100644, 0o755)] {
            let fixture = merge_conflict_repo();
            let path = fixture.path();
            let repo = Repository::open(path).unwrap();
            repo.config()
                .unwrap()
                .set_bool("core.filemode", trust_filemode)
                .unwrap();
            let mut index = repo.index().unwrap();
            let mut entry = index.get_path(Path::new("file.txt"), 2).unwrap();
            entry.mode = ours;
            index.add(&entry).unwrap();
            index.write().unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    path.join("file.txt"),
                    std::fs::Permissions::from_mode(worktree),
                )
                .unwrap();
            }
            recorded_save(path, "APPROVED\n");
            index.read(true).unwrap();
            let staged = index.get_path(Path::new("file.txt"), 0).unwrap();
            let expected = if !trust_filemode {
                ours
            } else if cfg!(unix) && worktree & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            };
            assert_eq!(staged.mode, expected);
            assert_eq!(repo.find_blob(staged.id).unwrap().content(), b"APPROVED\n");
        }
    }
}

#[test]
fn recorded_save_cleans_frozen_crlf_only_when_git_policy_requires_it() {
    if !test_support::run_isolated() {
        return;
    }
    for (attributes, autocrlf, expected) in [
        (None, false, b"APPROVED\r\n".as_slice()),
        (Some("* text=auto\n"), false, b"APPROVED\n".as_slice()),
        (Some("* text eol=lf\n"), false, b"APPROVED\n".as_slice()),
        (None, true, b"APPROVED\n".as_slice()),
    ] {
        let fixture = merge_conflict_repo();
        let path = fixture.path();
        let repo = Repository::open(path).unwrap();
        repo.config()
            .unwrap()
            .set_bool("core.autocrlf", autocrlf)
            .unwrap();
        if let Some(attributes) = attributes {
            write_file(path, ".gitattributes", attributes);
        }
        recorded_save(path, "APPROVED\r\n");
        let before = repo
            .index()
            .unwrap()
            .get_path(Path::new("file.txt"), 0)
            .unwrap()
            .id;
        assert_eq!(repo.find_blob(before).unwrap().content(), expected);
        assert_eq!(
            std::fs::read(path.join("file.txt")).unwrap(),
            b"APPROVED\r\n"
        );
        git(path, &["add", "file.txt"]);
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert_eq!(index.get_path(Path::new("file.txt"), 0).unwrap().id, before);
    }
}
