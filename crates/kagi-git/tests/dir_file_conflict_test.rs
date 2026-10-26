//! Integration tests for directory/file conflict classification + resolution
//! (#320 / ADR-0164).
//!
//! Native libgit2 and real CLI Git fixtures exercise both namespace representations.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_dir_file_resolution;
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{detect_conflict_session, plan_dir_file_resolution, ConflictKind, DirFileChoice};

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git spawn");
    assert!(s.success(), "git {:?}", args);
}

/// Build a repo with `file-side` (commits `thing` as a file) and `dir-side`
/// (commits `thing/child` as a directory), check out `checkout`, then perform a
/// libgit2 merge of `other` into it — leaving a genuine D/F conflict in the
/// index. Returns the temp dir (kept alive) and the opened repo.
fn df_conflict(checkout: &str, other: &str) -> (TempDir, Repository) {
    df_conflict_with_cli(checkout, other, false)
}

fn df_conflict_with_cli(checkout: &str, other: &str, cli: bool) -> (TempDir, Repository) {
    df_conflict_options(checkout, other, cli, false, false)
}

fn df_conflict_options(
    checkout: &str,
    other: &str,
    cli: bool,
    nested: bool,
    crlf: bool,
) -> (TempDir, Repository) {
    let td = TempDir::new().unwrap();
    let d = td.path();
    git(d, &["init", "-q", "-b", "main"]);
    let child = if nested { "thing/a/b/c" } else { "thing/child" };
    let directory = if nested { "thing/a/b" } else { "thing" };
    if crlf {
        std::fs::write(d.join(".gitattributes"), b"thing/** text eol=crlf\n").unwrap();
    }
    std::fs::write(d.join("base.txt"), b"base\n").unwrap();
    if cli {
        std::fs::create_dir_all(d.join(directory)).unwrap();
        std::fs::write(d.join(child), b"base child\n").unwrap();
        std::fs::write(
            d.join("thing~tracked-lookalike"),
            b"unrelated tracked file\n",
        )
        .unwrap();
    }
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "base"]);

    git(d, &["checkout", "-q", "-b", "file-side"]);
    if cli {
        git(d, &["rm", "-r", "thing"]);
    }
    std::fs::write(d.join("thing"), b"i am a file\n").unwrap();
    git(d, &["add", "thing"]);
    git(d, &["commit", "-qm", "file"]);

    git(d, &["checkout", "-q", "main"]);
    git(d, &["checkout", "-q", "-b", "dir-side"]);
    std::fs::create_dir_all(d.join(directory)).unwrap();
    std::fs::write(d.join(child), b"inside dir\n").unwrap();
    #[cfg(unix)]
    if nested {
        std::os::unix::fs::symlink("../../../base.txt", d.join("thing/a/b/link")).unwrap();
    }
    git(d, &["add", "thing"]);
    git(d, &["commit", "-qm", "dir"]);

    git(d, &["checkout", "-q", checkout]);
    let repo = Repository::open(d).unwrap();
    if cli {
        let result = Command::new("git")
            .args(["merge", "--no-edit", other])
            .current_dir(d)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", d)
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(1),
            "expected real CLI conflict: {:?}",
            result
        );
        return (td, repo);
    }
    let other_commit = repo
        .revparse_single(other)
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let other_oid = other_commit.id();
    drop(other_commit);
    let annotated = repo.find_annotated_commit(other_oid).unwrap();
    let mut co = git2::build::CheckoutBuilder::new();
    co.safe();
    repo.merge(&[&annotated], None, Some(&mut co)).unwrap();
    drop(annotated);
    (td, repo)
}

fn blob_text(repo: &Repository, oid: git2::Oid) -> String {
    let blob = repo.find_blob(oid).unwrap();
    String::from_utf8_lossy(blob.content()).into_owned()
}

/// A D/F conflict must be classified `DirFile`, not the modify/delete it used to
/// masquerade as. Mutation: make `classify_kind` skip the DirFile arm (return
/// early false) and this asserts fails with ModifyDelete.
#[test]
fn dir_file_conflict_is_classified() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for (co, other) in [("file-side", "dir-side"), ("dir-side", "file-side")] {
        let (_td, repo) = df_conflict(co, other);
        let session = detect_conflict_session(&repo).unwrap();
        let f = session
            .files
            .iter()
            .find(|f| f.path == PathBuf::from("thing"))
            .unwrap_or_else(|| panic!("no `thing` conflict merging {other} into {co}"));
        assert_eq!(
            f.kind,
            ConflictKind::DirFile,
            "merging {other} into {co} must classify `thing` as DirFile"
        );
    }
}

/// Keep-directory yields a tree with the directory side (`thing/child`) and no
/// file blob at `thing`.
#[test]
fn keep_directory_yields_directory_side() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepDirectory).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    let mut index = repo.index().unwrap();
    assert!(!index.has_conflicts(), "conflict must be cleared");
    assert!(
        index.get_path(Path::new("thing/child"), 0).is_some(),
        "directory side must survive"
    );
    assert!(
        index.get_path(Path::new("thing"), 0).is_none(),
        "file blob must be gone"
    );
    // The staged index must form a valid tree (no D/F namespace clash left).
    index.write_tree().expect("index must write a clean tree");
}

/// Keep-file yields a tree with the file blob at `thing` (its bytes intact) and
/// no directory entries under `thing/`.
#[test]
fn keep_file_yields_file_side() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    let mut index = repo.index().unwrap();
    assert!(!index.has_conflicts(), "conflict must be cleared");
    let entry = index
        .get_path(Path::new("thing"), 0)
        .expect("file side must be staged at stage 0");
    assert_eq!(
        blob_text(&repo, entry.id),
        "i am a file\n",
        "kept file must carry the file side's bytes"
    );
    assert!(
        index.get_path(Path::new("thing/child"), 0).is_none(),
        "directory side must be removed"
    );
    index.write_tree().expect("index must write a clean tree");
}

/// The resolution is recorded to the oplog. Mutation: drop the `append_oplog`
/// call in `execute_dir_file_resolution` and this asserts fails (no entry).
///
/// Also guards #408: the destructive choice must take a savepoint under
/// `refs/kagi/snapshots/` and record its id as a recovery handle in the oplog.
/// Mutation: drop the `create_snapshot` call and both the ref count and the
/// `snapshot=` assertion fail.
#[test]
fn resolution_recorded_in_oplog() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let logdir = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    // #408: a savepoint ref must now exist (recovery in git's own terms).
    let snap_count = repo
        .references_glob("refs/kagi/snapshots/*")
        .unwrap()
        .count();
    assert!(
        snap_count >= 1,
        "destructive dir-file resolution must create a snapshot ref"
    );

    let log = std::fs::read_to_string(logdir.path().join("operations.jsonl"))
        .expect("oplog file must exist");
    assert!(
        log.contains("conflict-dir-file:keep-file"),
        "oplog must record the dir-file resolution, got: {log}"
    );
    // #408: the recovery handle (snapshot id) must be embedded in the oplog.
    assert!(
        log.contains("snapshot="),
        "oplog must record the snapshot recovery handle, got: {log}"
    );

    std::env::remove_var("KAGI_LOG_DIR");
}

/// #407: after KeepDirectory the working tree must agree with the index — the
/// losing file side is removed from disk and the kept directory child exists.
///
/// libgit2's merge leaves the *directory* on disk in both directions, so to
/// exercise the broken path this first forces the working tree to hold the
/// losing *file* namespace (the exact "working tree still holds the other
/// namespace" state the issue describes). Mutation: drop the `reconcile_worktree`
/// call and both assertions fail (nothing rewrites the working tree).
#[test]
fn keep_directory_reconciles_working_tree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let wd = repo.workdir().unwrap().to_path_buf();

    // Put the losing (file) namespace on disk so the resolution must reconcile it.
    std::fs::remove_dir_all(wd.join("thing")).unwrap();
    std::fs::write(wd.join("thing"), b"stale file on disk\n").unwrap();
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepDirectory).unwrap();

    execute_dir_file_resolution(&repo, &wd, &plan).unwrap();

    assert!(
        !wd.join("thing").is_file(),
        "losing file side must be removed from the working tree"
    );
    assert!(
        wd.join("thing/child").exists(),
        "kept directory child must exist in the working tree"
    );
}

/// #407: after KeepFile the working tree must agree with the index — the losing
/// directory side is removed from disk and the kept file exists. Checks out the
/// dir side first so the working tree genuinely holds the losing namespace.
/// Mutation: drop the `reconcile_worktree` call and the dir-gone assertion fails.
#[test]
fn keep_file_reconciles_working_tree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("dir-side", "file-side");
    let wd = repo.workdir().unwrap().to_path_buf();
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    execute_dir_file_resolution(&repo, &wd, &plan).unwrap();

    assert!(
        wd.join("thing").is_file(),
        "kept file side must exist in the working tree"
    );
    assert!(
        !wd.join("thing/child").exists(),
        "losing directory side must be removed from the working tree"
    );
}

#[test]
fn frozen_dir_file_boundary_refuses_untrusted_or_changed_plan() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let before_child = std::fs::read(repo.workdir().unwrap().join("thing/child")).unwrap();
    let mut backend = kagi_git::Backend::open(repo.workdir().unwrap()).unwrap();
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    assert!(matches!(
        backend.execute_planned_dir_file_resolution(&plan),
        Err(kagi_git::GitError::Untrusted(_))
    ));
    let mut changed = plan.clone();
    changed.file_oid = repo.blob(b"changed file identity").unwrap();
    let trusted = kagi_git::Backend::open(repo.workdir().unwrap()).unwrap();
    assert!(
        trusted
            .execute_planned_dir_file_resolution(&changed)
            .is_err(),
        "must validate the supplied plan, not silently replace it"
    );
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
    assert_eq!(
        std::fs::read(repo.workdir().unwrap().join("thing/child")).unwrap(),
        before_child
    );
    assert_eq!(
        repo.references_glob("refs/kagi/snapshots/*")
            .unwrap()
            .count(),
        0
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;

#[test]
fn frozen_dir_file_plan_rejects_extra_missing_or_duplicate_children() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for choice in [DirFileChoice::KeepDirectory, DirFileChoice::KeepFile] {
        let (_td, repo) = df_conflict("file-side", "dir-side");
        let root = repo.workdir().unwrap();
        std::fs::write(root.join("base.txt"), b"UNSAVED outside the conflict\n").unwrap();
        let plan = plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap();
        assert_eq!(plan.dir_children, vec![PathBuf::from("thing/child")]);
        let before_index = std::fs::read(repo.path().join("index")).unwrap();
        let before_head = repo.head().unwrap().target();
        let child = std::fs::read(root.join("thing/child")).unwrap();
        let backend = kagi_git::Backend::open(root).unwrap();
        for children in [
            vec![PathBuf::from("thing/child"), PathBuf::from("base.txt")],
            Vec::new(),
            vec![PathBuf::from("thing/child"), PathBuf::from("thing/child")],
        ] {
            let mut changed = plan.clone();
            changed.dir_children = children;
            let error = backend
                .execute_planned_dir_file_resolution(&changed)
                .expect_err("unreviewed child list must be rejected before mutation");
            assert!(error.to_string().contains("changed since it was planned"));
            assert_eq!(
                std::fs::read(repo.path().join("index")).unwrap(),
                before_index
            );
            assert_eq!(repo.head().unwrap().target(), before_head);
            assert_eq!(std::fs::read(root.join("thing/child")).unwrap(), child);
            assert_eq!(
                std::fs::read(root.join("base.txt")).unwrap(),
                b"UNSAVED outside the conflict\n"
            );
            assert_eq!(
                repo.references_glob("refs/kagi/snapshots/*")
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}

#[test]
fn frozen_dir_file_plan_rejects_a_new_index_child() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let root = repo.workdir().unwrap();
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    std::fs::write(root.join("thing/new"), b"newly staged child\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("thing/new")).unwrap();
    index.write().unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let backend = kagi_git::Backend::open(root).unwrap();
    assert!(backend.execute_planned_dir_file_resolution(&plan).is_err());
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
    assert_eq!(
        std::fs::read(root.join("thing/new")).unwrap(),
        b"newly staged child\n"
    );
    assert_eq!(
        repo.references_glob("refs/kagi/snapshots/*")
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn keep_file_refuses_when_recovery_snapshot_cannot_be_written() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (td, repo) = df_conflict("dir-side", "file-side");
    std::fs::write(td.path().join("thing/untracked"), b"irreplaceable\n").unwrap();
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    // Block creation of the recovery ref without relying on Unix permissions.
    std::fs::create_dir_all(repo.path().join("refs/kagi")).unwrap();
    std::fs::write(repo.path().join("refs/kagi/snapshots"), b"blocked\n").unwrap();
    let index = std::fs::read(repo.path().join("index")).unwrap();
    let head = repo.head().unwrap().target();
    let merge_head = std::fs::read(repo.path().join("MERGE_HEAD")).unwrap();
    let child = std::fs::read(td.path().join("thing/child")).unwrap();
    let error = execute_dir_file_resolution(&repo, td.path(), &plan).unwrap_err();
    assert_eq!(
        error.blocker(),
        Some(&kagi_domain::plan_note::PlanNote::Conflicts(
            kagi_domain::plan_note::ConflictsNote::DraftPreservationFailed
        ))
    );
    assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
    assert_eq!(repo.head().unwrap().target(), head);
    assert_eq!(
        std::fs::read(repo.path().join("MERGE_HEAD")).unwrap(),
        merge_head
    );
    assert_eq!(std::fs::read(td.path().join("thing/child")).unwrap(), child);
    assert_eq!(
        std::fs::read(td.path().join("thing/untracked")).unwrap(),
        b"irreplaceable\n"
    );
    let entries = kagi_git::oplog::read_oplog_tail(1);
    assert!(matches!(
        entries.last().unwrap().outcome,
        kagi_git::oplog::OpOutcome::Refused { .. }
    ));
}

#[test]
fn cli_parked_namespace_resolves_both_directions_and_choices() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for (current, incoming) in [("file-side", "dir-side"), ("dir-side", "file-side")] {
        for choice in [DirFileChoice::KeepDirectory, DirFileChoice::KeepFile] {
            let (td, repo) = df_conflict_with_cli(current, incoming, true);
            let root = td.path();
            std::fs::write(root.join("thing~unrelated"), b"leave this alone\n").unwrap();
            let session = detect_conflict_session(&repo).unwrap();
            assert_eq!(session.files.len(), 1);
            assert_eq!(session.files[0].path, Path::new("thing"));
            assert_eq!(session.files[0].kind, ConflictKind::DirFile);
            let plan = plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap();
            assert_ne!(plan.file_path, plan.path);
            assert!(plan.child_entries.iter().any(|e| e.stage == 1));
            assert!(plan
                .child_entries
                .iter()
                .any(|e| e.stage == 2 || e.stage == 3));
            assert!(plan.removal_paths.contains(&plan.file_path));
            execute_dir_file_resolution(&repo, root, &plan).unwrap();
            let mut index = repo.index().unwrap();
            assert!(!index.has_conflicts());
            assert!(!root.join(&plan.file_path).exists());
            assert!(!root.join("thing~HEAD").exists());
            for stage in 0..=3 {
                assert!(index.get_path(&plan.file_path, stage).is_none());
            }
            match choice {
                DirFileChoice::KeepFile => {
                    let file = index.get_path(Path::new("thing"), 0).unwrap();
                    assert_eq!(file.id, plan.file_oid);
                    assert_eq!(std::fs::read(root.join("thing")).unwrap(), b"i am a file\n");
                    assert!(index.get_path(Path::new("thing/child"), 0).is_none());
                }
                DirFileChoice::KeepDirectory => {
                    assert_eq!(
                        std::fs::read(root.join("thing/child")).unwrap(),
                        b"inside dir\n"
                    );
                    assert!(index.get_path(Path::new("thing/child"), 0).is_some());
                    assert!(index.get_path(Path::new("thing"), 0).is_none());
                }
            }
            index.write_tree().unwrap();
            assert_eq!(
                std::fs::read(root.join("thing~unrelated")).unwrap(),
                b"leave this alone\n"
            );
            assert!(index
                .get_path(Path::new("thing~tracked-lookalike"), 0)
                .is_some());
            assert_eq!(
                std::fs::read(root.join("thing~tracked-lookalike")).unwrap(),
                b"unrelated tracked file\n"
            );
        }
    }
}

#[test]
fn cli_parked_plan_refuses_changed_child_identity_or_removal_contents() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for change in 0..3 {
        let (td, repo) = df_conflict_with_cli("file-side", "dir-side", true);
        let root = td.path();
        let plan =
            plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
        if change == 0 {
            let child = plan.kept_children.first().unwrap();
            let mut index = repo.index().unwrap();
            let mut entry = index.get_path(&child.path, child.stage as i32).unwrap();
            entry.id = repo.blob(b"new directory identity\n").unwrap();
            index.add(&entry).unwrap();
            index.write().unwrap();
        } else if change == 1 {
            std::fs::write(root.join("thing/new-untracked"), b"unapproved removal\n").unwrap();
        } else {
            std::fs::write(root.join("thing/child"), b"new unstaged work\n").unwrap();
        }
        let before = std::fs::read(repo.path().join("index")).unwrap();
        let work = std::fs::read(root.join("thing/child")).unwrap();
        assert!(execute_dir_file_resolution(&repo, root, &plan).is_err());
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
        assert_eq!(std::fs::read(root.join("thing/child")).unwrap(), work);
        if change == 1 {
            assert_eq!(
                std::fs::read(root.join("thing/new-untracked")).unwrap(),
                b"unapproved removal\n"
            );
        }
        assert_eq!(
            repo.references_glob("refs/kagi/snapshots/*")
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn suffix_without_side_tree_identity_is_not_a_parked_namespace() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (td, repo) = df_conflict_with_cli("file-side", "dir-side", true);
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    let mut index = repo.index().unwrap();
    let stage = if index.get_path(&plan.file_path, 2).is_some() {
        2
    } else {
        3
    };
    let mut entry = index.get_path(&plan.file_path, stage).unwrap();
    entry.id = repo.blob(b"not the original file side\n").unwrap();
    index.add(&entry).unwrap();
    index.write().unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert!(!session
        .files
        .iter()
        .any(|f| f.kind == ConflictKind::DirFile));
    assert!(session.files.iter().any(|f| f.path == plan.file_path));
    assert!(plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).is_err());
    assert!(td.path().join(&plan.file_path).exists());
}

#[test]
fn dir_file_plan_refuses_gitlinks_before_any_mutation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for file_side in [false, true] {
        let (td, repo) = df_conflict("dir-side", "file-side");
        let plan =
            plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
        let mut index = repo.index().unwrap();
        let mut entry = if file_side {
            index.get_path(&plan.file_path, 3).unwrap()
        } else {
            let child = &plan.kept_children[0];
            index.get_path(&child.path, child.stage as i32).unwrap()
        };
        entry.mode = 0o160000;
        entry.id = repo.head().unwrap().target().unwrap();
        index.add(&entry).unwrap();
        index.write().unwrap();
        let before = std::fs::read(repo.path().join("index")).unwrap();
        let child = std::fs::read(td.path().join("thing/child")).unwrap();
        for choice in [DirFileChoice::KeepFile, DirFileChoice::KeepDirectory] {
            let error = plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap_err();
            assert!(error.to_string().contains("submodules"), "{error}");
        }
        assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
        assert_eq!(std::fs::read(td.path().join("thing/child")).unwrap(), child);
        assert_eq!(
            repo.references_glob("refs/kagi/snapshots/*")
                .unwrap()
                .count(),
            0
        );
    }
}

#[test]
fn dir_file_plan_refuses_nested_repository_metadata_before_recovery_or_mutation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (td, repo) = df_conflict("dir-side", "file-side");
    let root = td.path();
    let approved = [DirFileChoice::KeepFile, DirFileChoice::KeepDirectory]
        .map(|choice| plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap());
    let nested = root.join("thing/vendor");
    std::fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q", "-b", "main"]);
    std::fs::write(nested.join("treasure"), b"nested repository work\n").unwrap();
    git(&nested, &["add", "."]);
    git(&nested, &["commit", "-qm", "nested"]);
    let nested_repo = Repository::open(&nested).unwrap();
    let nested_head = nested_repo.head().unwrap().target().unwrap();
    let metadata = std::fs::read(nested.join(".git/config")).unwrap();
    let index = std::fs::read(repo.path().join("index")).unwrap();
    for plan in &approved {
        let error = plan_dir_file_resolution(&repo, Path::new("thing"), plan.choice).unwrap_err();
        assert!(
            error.to_string().contains("nested repository metadata"),
            "{error}"
        );
        assert!(execute_dir_file_resolution(&repo, root, plan).is_err());
    }
    assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), index);
    assert_eq!(
        repo.references_glob("refs/kagi/snapshots/*")
            .unwrap()
            .count(),
        0
    );
    assert_eq!(std::fs::read(nested.join(".git/config")).unwrap(), metadata);
    assert_eq!(nested_repo.head().unwrap().target().unwrap(), nested_head);
    assert!(nested_repo.find_commit(nested_head).is_ok());
    assert_eq!(
        std::fs::read(nested.join("treasure")).unwrap(),
        b"nested repository work\n"
    );
}

#[test]
fn keep_directory_verifies_filtered_crlf_checkout() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for cli in [false, true] {
        let (td, repo) = df_conflict_options("file-side", "dir-side", cli, false, true);
        let plan =
            plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepDirectory)
                .unwrap();
        execute_dir_file_resolution(&repo, td.path(), &plan).unwrap();
        assert_eq!(
            std::fs::read(td.path().join("thing/child")).unwrap(),
            b"inside dir\r\n"
        );
        let index = repo.index().unwrap();
        let child = index.get_path(Path::new("thing/child"), 0).unwrap();
        assert_eq!(repo.find_blob(child.id).unwrap().content(), b"inside dir\n");
        assert!(!index.has_conflicts());
    }
}

#[cfg(unix)]
#[test]
fn nested_namespace_choices_do_not_follow_internal_or_external_symlinks() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for (current, incoming) in [("file-side", "dir-side"), ("dir-side", "file-side")] {
        for choice in [DirFileChoice::KeepFile, DirFileChoice::KeepDirectory] {
            let (td, repo) = df_conflict_options(current, incoming, true, true, false);
            let outside = TempDir::new().unwrap();
            std::fs::write(outside.path().join("treasure"), b"outside\n").unwrap();
            std::os::unix::fs::symlink(outside.path(), td.path().join("thing/a/b/outside"))
                .unwrap();
            let plan = plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap();
            let directories = plan.removed_directories();
            if choice == DirFileChoice::KeepFile {
                assert_eq!(
                    directories,
                    ["thing/a/b", "thing/a", "thing"].map(PathBuf::from)
                );
            } else {
                assert!(directories.is_empty());
            }
            execute_dir_file_resolution(&repo, td.path(), &plan).unwrap();
            assert!(std::fs::symlink_metadata(td.path().join(&plan.file_path)).is_err());
            let mut index = repo.index().unwrap();
            assert!(!index.has_conflicts());
            for stage in 0..=3 {
                assert!(index.get_path(&plan.file_path, stage).is_none());
            }
            match choice {
                DirFileChoice::KeepFile => {
                    assert_eq!(
                        std::fs::read(td.path().join("thing")).unwrap(),
                        b"i am a file\n"
                    );
                    assert_eq!(
                        index.get_path(Path::new("thing"), 0).unwrap().id,
                        plan.file_oid
                    );
                    assert!(!index.iter().any(|e| e.path.starts_with(b"thing/")));
                }
                DirFileChoice::KeepDirectory => {
                    assert_eq!(
                        std::fs::read(td.path().join("thing/a/b/c")).unwrap(),
                        b"inside dir\n"
                    );
                    assert_eq!(
                        std::fs::read_link(td.path().join("thing/a/b/link")).unwrap(),
                        Path::new("../../../base.txt")
                    );
                    assert_eq!(
                        std::fs::read_link(td.path().join("thing/a/b/outside")).unwrap(),
                        outside.path()
                    );
                    assert!(index.get_path(Path::new("thing"), 0).is_none());
                    assert_eq!(
                        index.get_path(Path::new("thing/a/b/link"), 0).unwrap().mode,
                        0o120000
                    );
                }
            }
            index.write_tree().unwrap();
            assert_eq!(
                std::fs::read(td.path().join("base.txt")).unwrap(),
                b"base\n"
            );
            assert_eq!(
                std::fs::read(outside.path().join("treasure")).unwrap(),
                b"outside\n"
            );
        }
    }
}

#[test]
fn dir_file_refuses_unaffected_index_drift() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for choice in [DirFileChoice::KeepFile, DirFileChoice::KeepDirectory] {
        for change in ["oid", "mode", "stage", "added", "removed"] {
            let (td, repo) = df_conflict_with_cli("file-side", "dir-side", true);
            let plan = plan_dir_file_resolution(&repo, Path::new("thing"), choice).unwrap();
            let mut index = repo.index().unwrap();
            let mut entry = index.get_path(Path::new("base.txt"), 0).unwrap();
            match change {
                "oid" => entry.id = repo.blob(b"external staged update\n").unwrap(),
                "mode" => entry.mode = 0o100755,
                "stage" => {
                    index.remove(Path::new("base.txt"), 0).unwrap();
                    entry.flags |= 2 << 12;
                }
                "added" => entry.path = b"external-added".to_vec(),
                "removed" => {
                    index.remove(Path::new("base.txt"), 0).unwrap();
                }
                _ => unreachable!(),
            }
            if change != "removed" {
                index.add(&entry).unwrap();
            }
            index.write().unwrap();
            let before = std::fs::read(repo.path().join("index")).unwrap();
            let parked = std::fs::read(td.path().join(&plan.file_path)).unwrap();
            assert!(
                execute_dir_file_resolution(&repo, td.path(), &plan).is_err(),
                "{change}"
            );
            assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
            assert_eq!(
                std::fs::read(td.path().join(&plan.file_path)).unwrap(),
                parked
            );
            assert_eq!(
                std::fs::read(td.path().join("thing/child")).unwrap(),
                b"inside dir\n"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn dir_file_refuses_symlinked_namespace_ancestor() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (td, repo) = df_conflict_with_cli("file-side", "dir-side", true);
    let initial =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    let mut index = repo.index().unwrap();
    let entries: Vec<_> = index.iter().collect();
    index.clear().unwrap();
    for mut entry in entries {
        entry.path = [b"parent/".as_slice(), entry.path.as_slice()].concat();
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::create_dir(outside.path().join("thing")).unwrap();
    std::fs::write(outside.path().join("thing/child"), b"outside child\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), td.path().join("parent")).unwrap();
    // Use the native index representation so attribution does not depend on
    // changing source trees when moving the conflict below the unsafe parent.
    let mut index = repo.index().unwrap();
    let parked = Path::new("parent").join(&initial.file_path);
    let mut file = index.get_path(&parked, 2).unwrap();
    index.conflict_remove(&parked).unwrap();
    file.path = b"parent/thing".to_vec();
    index.add(&file).unwrap();
    index.write().unwrap();
    let before = std::fs::read(repo.path().join("index")).unwrap();
    for choice in [DirFileChoice::KeepFile, DirFileChoice::KeepDirectory] {
        let error = plan_dir_file_resolution(&repo, Path::new("parent/thing"), choice).unwrap_err();
        assert!(error.to_string().contains("unsafe ancestor"), "{error}");
    }
    assert_eq!(std::fs::read(repo.path().join("index")).unwrap(), before);
    assert_eq!(
        std::fs::read(outside.path().join("thing/child")).unwrap(),
        b"outside child\n"
    );
}
