//! Frozen staging preserves Git's mode/filter semantics through Save and Continue.
use super::*;
use kagi_domain::conflict_family::ConflictProgress;
use kagi_git::{backend::ExecutionPolicy, oplog::OpOutcome, Backend};

fn recorded_save(path: &Path, file: &str, text: &str) {
    let file = Path::new(file);
    let backend = Backend::open(path).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer.set_manual_text(file, text).unwrap();
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        file,
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
            recorded_save(path, "file.txt", "APPROVED\n");
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
        recorded_save(path, "file.txt", "APPROVED\r\n");
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

#[test]
fn recorded_save_filters_attributes_using_their_resolved_self_policy() {
    if !test_support::run_isolated() {
        return;
    }
    for name in [".gitattributes", "file.txt"] {
        let fixture = TempDir::new().unwrap();
        let path = fixture.path();
        init_repo(path, "main");
        git(path, &["config", "core.autocrlf", "false"]);
        write_file(path, ".gitattributes", "* -text\n");
        write_file(path, name, "* -text\n# base\n");
        git(path, &["add", "."]);
        git(path, &["commit", "-qm", "base"]);
        git(path, &["checkout", "-qb", "feature"]);
        write_file(path, name, "* -text\n# incoming\n");
        git(path, &["commit", "-qam", "incoming"]);
        git(path, &["checkout", "-q", "main"]);
        write_file(path, name, "* -text\n# current\n");
        git(path, &["commit", "-qam", "current"]);
        assert!(!git_allow_fail(path, &["merge", "feature"]));
        write_file(path, name, "* -text\n");

        let result = "* text eol=lf\r\n";
        recorded_save(path, name, result);
        let repo = Repository::open(path).unwrap();
        let mut index = repo.index().unwrap();
        assert!(!index.has_conflicts());
        let staged = index.get_path(Path::new(name), 0).unwrap();
        let expected = if name == ".gitattributes" {
            b"* text eol=lf\n".as_slice()
        } else {
            result.as_bytes()
        };
        assert_eq!(repo.find_blob(staged.id).unwrap().content(), expected);
        assert_eq!(std::fs::read(path.join(name)).unwrap(), result.as_bytes());
        git(path, &["add", name]);
        index.read(true).unwrap();
        assert_eq!(
            index.get_path(Path::new(name), 0).unwrap().id,
            staged.id,
            "Save and ordinary restaging must apply the same clean policy"
        );
    }
}

#[test]
fn continue_cleans_frozen_results_using_attributes_resolved_in_the_same_batch() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = TempDir::new().unwrap();
    let path = fixture.path();
    init_repo(path, "main");
    git(path, &["config", "core.autocrlf", "false"]);
    write_file(path, ".gitattributes", "*.txt -text\n");
    write_file(path, "a.txt", "BASE\r\n");
    git(path, &["add", "."]);
    git(path, &["commit", "-qm", "base"]);
    git(path, &["checkout", "-qb", "feature"]);
    write_file(path, ".gitattributes", "*.txt text eol=lf\n");
    write_file(path, "a.txt", "INCOMING\r\n");
    git(path, &["commit", "-qam", "incoming"]);
    git(path, &["checkout", "-q", "main"]);
    write_file(path, ".gitattributes", "* -text\n");
    write_file(path, "a.txt", "CURRENT\r\n");
    git(path, &["commit", "-qam", "current"]);
    assert!(!git_allow_fail(path, &["merge", "feature"]));
    // Retain the current policy in the WT until Continue materializes Result.
    write_file(path, ".gitattributes", "* -text\n");
    let backend = Backend::open(path).unwrap();
    let mut snapshot = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(snapshot.session.files.len(), 2);
    // Deliberately visit the dependent before its attributes, not lexical order.
    snapshot.session.files.reverse();
    assert_eq!(snapshot.session.files[0].path, Path::new("a.txt"));
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer
        .set_manual_text(Path::new(".gitattributes"), "*.txt text eol=lf\n")
        .unwrap();
    buffer
        .set_manual_text(Path::new("a.txt"), "APPROVED\r\n")
        .unwrap();
    let before = backend
        .plan_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
        .unwrap()
        .current;
    let report = Backend::run_recorded_conflict_handoff(
        path,
        &snapshot.session,
        &buffer,
        &snapshot.observation.revision,
        before,
    );
    report.result.unwrap();
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    let repo = Repository::open(path).unwrap();
    let index = repo.index().unwrap();
    assert!(!index.has_conflicts());
    for (name, expected) in [
        (".gitattributes", b"*.txt text eol=lf\n".as_slice()),
        ("a.txt", b"APPROVED\n".as_slice()),
    ] {
        let entry = index.get_path(Path::new(name), 0).unwrap();
        assert_eq!(repo.find_blob(entry.id).unwrap().content(), expected);
    }
    assert_eq!(std::fs::read(path.join("a.txt")).unwrap(), b"APPROVED\r\n");
    let staged = index.get_path(Path::new("a.txt"), 0).unwrap().id;
    git(path, &["add", "a.txt"]);
    assert_eq!(
        repo.index()
            .unwrap()
            .get_path(Path::new("a.txt"), 0)
            .unwrap()
            .id,
        staged,
        "ordinary restaging must not change the approved clean-filtered blob"
    );
}
