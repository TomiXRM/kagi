//! #334 slice 2b (ADR-0214 §5): op-revert and restore-to-point put branches
//! back from **recorded** ref moves only, through plan → preflight → execute →
//! verify → oplog, and are themselves recorded — so they can be undone too.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use kagi_domain::plan_note::{HeadAt, OplogRestoreNote, PlanNote};
use kagi_git::oplog::{
    append_oplog, entry_to_json, read_oplog_tail_for_repo, OpLogEntry, OpOutcome, RefScope,
};
use kagi_git::{Backend, CommitId, GitError, Operation, OperationPlan, StateSummary};
use std::path::{Path, PathBuf};

fn repo(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "a.txt", "a\n");
    commit_all(&repo, "base");
    repo
}

fn backend(dir: &Path) -> Backend {
    let mut b = Backend::open(dir).unwrap();
    b.set_auto_snapshot(false);
    b
}

/// Run `op` through the recorded pipeline (it must succeed); its entry id.
fn run(dir: &Path, op: Operation) -> u64 {
    let mut b = backend(dir);
    let plan = b.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{op:?}: {:?}", plan.blockers);
    b.run(&op, &plan).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    newest(dir).id
}

fn newest(dir: &Path) -> OpLogEntry {
    read_oplog_tail_for_repo(dir, 1).pop().expect("recorded")
}

fn plan(dir: &Path, op: &Operation) -> OperationPlan {
    backend(dir).plan(op).unwrap()
}

fn restore_blockers(plan: &OperationPlan) -> Vec<OplogRestoreNote> {
    plan.blockers
        .iter()
        .filter_map(|b| match b {
            PlanNote::OplogRestore(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

fn branches(dir: &Path) -> String {
    git_output(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ],
    )
}

fn create(dir: &Path, name: &str) -> u64 {
    let at = CommitId(git_output(dir, &["rev-parse", "HEAD"]));
    run(
        dir,
        Operation::CreateBranch {
            name: name.into(),
            at,
        },
    )
}

fn commit(dir: &Path, content: &str) -> u64 {
    write_file(dir, "a.txt", content);
    git(dir, &["add", "a.txt"]);
    run(
        dir,
        Operation::Commit {
            message: content.trim().into(),
        },
    )
}

/// Record a real local tag write with the same observer and oplog builder used
/// for writes whose ref names are not known until execution.
fn observed_tag_write(dir: &Path, op: &str, args: &[&str]) -> u64 {
    let (result, moves) = backend(dir).observe_ref_moves(|_| {
        git(dir, args);
        Ok::<(), GitError>(())
    });
    result.unwrap();
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let entry = OpLogEntry::new(
        op,
        dir.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    )
    .with_worktree(Some(dir.display().to_string()))
    .with_ref_moves(moves);
    append_oplog(&entry).unwrap();
    newest(dir).id
}

#[test]
fn recorded_lightweight_tag_creation_can_be_reverted_and_recreated() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let tip = git_output(&repo, &["rev-parse", "HEAD"]);
    let created = run(
        &repo,
        Operation::CreateTag {
            name: "light".into(),
            at: CommitId(tip.clone()),
        },
    );
    assert_eq!(newest(&repo).ref_scope, RefScope::HeadsAndTags);
    assert_eq!(
        newest(&repo)
            .ref_moves
            .as_ref()
            .and_then(|moves| moves.iter().find(|m| m.refname == "refs/tags/light"))
            .and_then(|m| m.new.as_deref()),
        Some(tip.as_str()),
        "record the local tag at its raw ref OID"
    );
    let revert = Operation::OpRevert { entry_id: created };
    let approved = plan(&repo, &revert);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    backend(&repo).run(&revert, &approved).unwrap();
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/light"]
    ));
    let undone = newest(&repo).id;
    let recreate = Operation::OpRevert { entry_id: undone };
    let approved = plan(&repo, &recreate);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    backend(&repo).run(&recreate, &approved).unwrap();
    assert_eq!(git_output(&repo, &["rev-parse", "refs/tags/light"]), tip);
}

#[test]
fn legacy_branch_only_tag_entry_blocks_revert_and_restore_to_point() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "point");
    // The pre-tag observer recorded branch moves only: CreateTag succeeded,
    // but its receipt said `ref_moves: []`. Reproduce the old wire row, not
    // a new receipt with tag-inclusive observation scope.
    git(&repo, &["tag", "legacy"]);
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let mut entry = OpLogEntry::new(
        "create-tag",
        repo.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    )
    .with_worktree(Some(repo.display().to_string()))
    .with_ref_moves(Some(Vec::new()));
    entry.id = point + 1;
    entry.parent = Some(point);
    let mut old_wire: serde_json::Value = serde_json::from_str(&entry_to_json(&entry)).unwrap();
    old_wire.as_object_mut().unwrap().remove("ref_scope");
    let log = PathBuf::from(std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl");
    use std::io::Write;
    writeln!(
        std::fs::OpenOptions::new().append(true).open(log).unwrap(),
        "{old_wire}"
    )
    .unwrap();
    let legacy_id = newest(&repo).id;
    assert_eq!(legacy_id, entry.id);
    assert_eq!(newest(&repo).ref_moves, Some(Vec::new()));
    assert_eq!(newest(&repo).ref_scope, RefScope::LegacyOrUnknown);

    for (op, expected) in [
        (
            Operation::OpRevert {
                entry_id: legacy_id,
            },
            legacy_id,
        ),
        (Operation::RestoreToPoint { entry_id: point }, legacy_id),
    ] {
        assert!(
            restore_blockers(&plan(&repo, &op)).contains(&OplogRestoreNote::NotRecorded {
                id: expected,
                op: "create-tag".into(),
            }),
            "{op:?} must not treat a branch-only observation as tag-complete"
        );
    }
    assert!(git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/legacy"]
    ));
}

#[test]
fn a_tag_receipt_losing_its_observation_scope_after_confirmation_refuses_preflight() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let tip = git_output(&repo, &["rev-parse", "HEAD"]);
    let created = run(
        &repo,
        Operation::CreateTag {
            name: "release".into(),
            at: CommitId(tip.clone()),
        },
    );
    let revert = Operation::OpRevert { entry_id: created };
    let approved = plan(&repo, &revert);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);

    // A changed/older receipt is no longer the tag-complete observation the
    // user approved. Preflight must re-read it before moving any ref.
    let log = PathBuf::from(std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl");
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value["id"] == created {
                value.as_object_mut().unwrap().remove("ref_scope");
            }
            format!("{value}\n")
        })
        .collect();
    std::fs::write(&log, lines.concat()).unwrap();

    assert!(backend(&repo).run(&revert, &approved).is_err());
    assert_eq!(git_output(&repo, &["rev-parse", "refs/tags/release"]), tip);
}

#[test]
fn annotated_tag_delete_and_lightweight_tag_move_round_trip_to_point() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    run(
        &repo,
        Operation::CreateTag {
            name: "light".into(),
            at: CommitId(first.clone()),
        },
    );
    observed_tag_write(
        &repo,
        "create-annotated-tag",
        &["tag", "-a", "annotated", "-m", "note"],
    );
    let tag_object = git_output(&repo, &["rev-parse", "refs/tags/annotated"]);
    assert_ne!(
        tag_object, first,
        "annotated tag retains its own object OID"
    );
    let point = create(&repo, "point");
    commit(&repo, "later\n");
    let second = git_output(&repo, &["rev-parse", "HEAD"]);
    observed_tag_write(
        &repo,
        "move-tag",
        &["update-ref", "refs/tags/light", &second, &first],
    );
    observed_tag_write(
        &repo,
        "delete-tag",
        &["update-ref", "-d", "refs/tags/annotated", &tag_object],
    );
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/annotated"]
    ));

    let restore = Operation::RestoreToPoint { entry_id: point };
    let approved = plan(&repo, &restore);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    backend(&repo).run(&restore, &approved).unwrap();
    assert_eq!(git_output(&repo, &["rev-parse", "refs/tags/light"]), first);
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/annotated"]),
        tag_object,
        "restore the tag object, not the peeled commit"
    );
    let receipt = newest(&repo);
    assert!(
        receipt
            .backup_refs
            .iter()
            .any(|backup| { git_output(&repo, &["rev-parse", backup]) == second }),
        "moved tag tip must have a durable backup"
    );
    let revert = Operation::OpRevert {
        entry_id: receipt.id,
    };
    let approved = plan(&repo, &revert);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    backend(&repo).run(&revert, &approved).unwrap();
    assert_eq!(git_output(&repo, &["rev-parse", "refs/tags/light"]), second);
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/tags/annotated"]
    ));
}

#[test]
fn moved_annotated_tag_retains_its_raw_object_and_leaves_index_tree_and_remote_alone() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    observed_tag_write(
        &repo,
        "create-annotated-tag",
        &["tag", "-a", "release", "-m", "first"],
    );
    let first_tag = git_output(&repo, &["rev-parse", "refs/tags/release"]);
    let point = create(&repo, "point");
    observed_tag_write(
        &repo,
        "move-annotated-tag",
        &["tag", "-fa", "release", "-m", "second"],
    );
    let second_tag = git_output(&repo, &["rev-parse", "refs/tags/release"]);
    assert_ne!(first_tag, second_tag);
    git(
        &repo,
        &[
            "update-ref",
            "refs/remotes/origin/main",
            &git_output(&repo, &["rev-parse", "HEAD"]),
        ],
    );
    write_file(&repo, "a.txt", "staged\n");
    git(&repo, &["add", "a.txt"]);
    write_file(&repo, "a.txt", "unstaged\n");
    let index = git_output(&repo, &["ls-files", "-s", "--", "a.txt"]);
    let tree = std::fs::read_to_string(repo.join("a.txt")).unwrap();
    let remote = git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]);

    let restore = Operation::RestoreToPoint { entry_id: point };
    let approved = plan(&repo, &restore);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    let report = backend(&repo).run_recorded(&restore, &approved);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(
        matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "restore must persist its durable recovery receipt: {:?}",
        report.recording
    );
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        first_tag
    );
    let receipt = newest(&repo);
    let backups: Vec<_> = receipt
        .backup_refs
        .iter()
        .map(|backup| {
            (
                backup.clone(),
                git_output(&repo, &["rev-parse", backup]),
                git_output(&repo, &["cat-file", "-t", backup]),
            )
        })
        .collect();
    assert!(
        backups
            .iter()
            .any(|(_, oid, kind)| oid == &second_tag && kind == "tag"),
        "the moved annotated tag object {second_tag} needs a durable backup: {backups:?}"
    );
    assert!(receipt.recovery.iter().any(|handle| {
        handle.kind == kagi_git::oplog::recovery::TAG_REF
            && handle.oid == second_tag
            && handle
                .reference
                .as_ref()
                .is_some_and(|reference| receipt.backup_refs.contains(reference))
    }));
    assert_eq!(git_output(&repo, &["ls-files", "-s", "--", "a.txt"]), index);
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), tree);
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        remote
    );

    let revert = Operation::OpRevert {
        entry_id: receipt.id,
    };
    let approved = plan(&repo, &revert);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    backend(&repo).run(&revert, &approved).unwrap();
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        second_tag
    );
    assert_eq!(git_output(&repo, &["ls-files", "-s", "--", "a.txt"]), index);
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), tree);
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        remote
    );
    // Explicit retirement must accept a raw tag-object recovery root; its
    // lifetime ends with the receipt, without touching the restored tag.
    let backend = backend(&repo);
    let forget = backend.plan_forget_oplog_entry(&receipt).unwrap();
    assert_eq!(
        forget.backup_refs().collect::<Vec<_>>(),
        receipt
            .backup_refs
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    backend.execute_forget_oplog_entry(&forget).result.unwrap();
    assert!(receipt.backup_refs.iter().all(|reference| {
        !git_fixture::git_succeeds(&repo, &["show-ref", "--verify", reference])
    }));
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        second_tag
    );
}

#[test]
fn tree_tag_restore_records_its_backup_and_can_retire_the_receipt() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first_tree = git_output(&repo, &["rev-parse", "HEAD^{tree}"]);
    observed_tag_write(&repo, "create-tree-tag", &["tag", "release", &first_tree]);
    let point = create(&repo, "point");
    commit(&repo, "later\n");
    let second_tree = git_output(&repo, &["rev-parse", "HEAD^{tree}"]);
    assert_ne!(first_tree, second_tree);
    observed_tag_write(
        &repo,
        "move-tree-tag",
        &["update-ref", "refs/tags/release", &second_tree, &first_tree],
    );

    let restore = Operation::RestoreToPoint { entry_id: point };
    let approved = plan(&repo, &restore);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    let report = backend(&repo).run_recorded(&restore, &approved);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(
        matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Appended { .. }
        ),
        "tree-tag restore must record its ref update: {:?}",
        report.recording
    );
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        first_tree
    );
    let receipt = newest(&repo);
    assert_eq!(receipt.op, "restore-to-point");
    let backup = receipt
        .backup_refs
        .iter()
        .find(|reference| git_output(&repo, &["rev-parse", reference]) == second_tree)
        .expect("the moved tree tag needs a retained backup");
    assert_eq!(git_output(&repo, &["cat-file", "-t", backup]), "tree");
    assert!(receipt.recovery.iter().any(|handle| {
        handle.kind == kagi_git::oplog::recovery::TAG_REF
            && handle.oid == second_tree
            && handle.reference.as_deref() == Some(backup)
    }));
    let backend = backend(&repo);
    let forget = backend.plan_forget_oplog_entry(&receipt).unwrap();
    backend.execute_forget_oplog_entry(&forget).result.unwrap();
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", backup]
    ));
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        first_tree
    );
}

#[test]
fn local_tag_drift_after_confirmation_refuses_without_writing() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    let created = run(
        &repo,
        Operation::CreateTag {
            name: "release".into(),
            at: CommitId(first.clone()),
        },
    );
    commit(&repo, "later\n");
    let drifted = git_output(&repo, &["rev-parse", "HEAD"]);
    let revert = Operation::OpRevert { entry_id: created };
    let approved = plan(&repo, &revert);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    git(
        &repo,
        &["update-ref", "refs/tags/release", &drifted, &first],
    );
    let err = backend(&repo).run(&revert, &approved).unwrap_err();
    assert!(err.to_string().contains("refs/tags/release"), "{err}");
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        drifted
    );
}

/// #915: Remove from a surviving managing worktree observes deleted branch
/// OIDs (or `Some(empty)` when kept), so RestoreToPoint can cross its receipt.
#[test]
fn restore_to_point_crosses_remove_from_a_separate_tab() {
    if !test_support::run_isolated() {
        return;
    }
    for delete_branch in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let linked = tmp.path().join("linked");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "side",
                linked.to_str().unwrap(),
            ],
        );
        let side_tip = git_output(&repo, &["rev-parse", "refs/heads/side"]);
        let point = create(&repo, "point");
        let remove = Backend::plan_recorded_remove(&repo, "linked", delete_branch).unwrap();
        assert!(
            remove.preview.blockers.is_empty(),
            "{:?}",
            remove.preview.blockers
        );
        let report = Backend::run_recorded_remove(&remove, kagi_git::oplog::Actor::Human, None);
        assert!(
            matches!(report.recording.entry().outcome, OpOutcome::Success { .. }),
            "{:?}",
            report.recording.entry()
        );
        assert!(!linked.exists());
        let entry = newest(&repo);
        assert_eq!(entry.op, "remove-worktree");
        let moves = entry
            .ref_moves
            .expect("a separate-tab Remove must record branch observations");
        if delete_branch {
            assert_eq!(moves.len(), 1, "{moves:?}");
            assert_eq!(moves[0].refname, "refs/heads/side");
            assert_eq!(moves[0].old.as_deref(), Some(side_tip.as_str()));
            assert_eq!(moves[0].new, None);
        } else {
            assert!(moves.is_empty(), "{moves:?}");
        }
        create(&repo, "after");

        let op = Operation::RestoreToPoint { entry_id: point };
        let planned = plan(&repo, &op);
        assert!(planned.blockers.is_empty(), "{:?}", planned.blockers);
        backend(&repo).run(&op, &planned).unwrap();
        assert_eq!(
            git_output(&repo, &["rev-parse", "refs/heads/side"]),
            side_tip
        );
        assert!(!git_fixture::git_succeeds(
            &repo,
            &["show-ref", "--verify", "refs/heads/after"]
        ));
    }
}

/// #938: a bare common dir has no usable Backend, but its branch refs remain
/// observable before and after Remove through a surviving linked tab.
#[test]
fn bare_backed_recorded_remove_crosses_restore_with_branch_tip_preserved() {
    if !test_support::run_isolated() {
        return;
    }
    for delete_branch in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let seed = repo(tmp.path());
        let bare = tmp.path().join("bare.git");
        git(
            &seed,
            &[
                "clone",
                "--bare",
                "-q",
                seed.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let manager = tmp.path().join("manager");
        let target = tmp.path().join("target");
        for (name, path) in [("manager", &manager), ("target", &target)] {
            git(
                &bare,
                &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()],
            );
        }
        let target_tip = git_output(&manager, &["rev-parse", "refs/heads/target"]);
        let point = create(&manager, "point");
        let removal = Backend::plan_recorded_remove(&manager, "target", delete_branch).unwrap();
        assert!(removal.preview.blockers.is_empty());
        let report = Backend::run_recorded_remove(&removal, kagi_git::oplog::Actor::Human, None);
        assert!(
            matches!(report.recording.entry().outcome, OpOutcome::Success { .. }),
            "{:?}",
            report.recording.entry()
        );
        assert!(!target.exists());
        assert!(bare.join("HEAD").exists());
        let moves = report
            .recording
            .entry()
            .ref_moves
            .as_ref()
            .expect("bare common dir must record its branch snapshot");
        if delete_branch {
            assert_eq!(moves.len(), 1, "{moves:?}");
            assert_eq!(moves[0].refname, "refs/heads/target");
            assert_eq!(moves[0].old.as_deref(), Some(target_tip.as_str()));
            assert_eq!(moves[0].new, None);
        } else {
            assert!(moves.is_empty(), "{moves:?}");
        }
        create(&manager, "after");
        let op = Operation::RestoreToPoint { entry_id: point };
        let planned = plan(&manager, &op);
        assert!(planned.blockers.is_empty(), "{:?}", planned.blockers);
        backend(&manager).run(&op, &planned).unwrap();
        assert_eq!(
            git_output(&manager, &["rev-parse", "refs/heads/target"]),
            target_tip
        );
        assert!(!git_fixture::git_succeeds(
            &manager,
            &["show-ref", "--verify", "refs/heads/after"]
        ));
    }
}

/// #938 review: a linked manager survives removal of another linked worktree.
/// A trusted pre_remove checkout in the manager must be recorded as a HEAD
/// switch; otherwise restoring across this receipt can delete a later branch
/// while incorrectly claiming the earlier checkout has been restored.
#[test]
fn remove_from_surviving_manager_records_pre_remove_checkout_and_blocks_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let manager = tmp.path().join("manager");
    let target = tmp.path().join("target");
    std::fs::create_dir(repo.join(".kagi")).unwrap();
    write_file(
        &repo,
        ".kagi/worktree.toml",
        &format!(
            "[[pre_remove]]\ntype = \"command\"\nrun = \"git -C {} checkout -q other\"\n",
            manager.display()
        ),
    );
    git(&repo, &["add", ".kagi/worktree.toml"]);
    git(&repo, &["commit", "-qm", "trusted remove step"]);
    git(&repo, &["branch", "other"]);
    for (name, path) in [("manager", &manager), ("target", &target)] {
        git(
            &repo,
            &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()],
        );
    }
    let point = create(&manager, "point");
    let remove = Backend::plan_recorded_remove(&manager, "target", false).unwrap();
    assert!(
        remove.preview.blockers.is_empty(),
        "{:?}",
        remove.preview.blockers
    );
    let report = Backend::run_recorded_remove(&remove, kagi_git::oplog::Actor::Human, None);
    assert!(
        matches!(report.recording.entry().outcome, OpOutcome::Success { .. }),
        "{:?}",
        report.recording.entry()
    );
    assert_eq!(
        git_output(&manager, &["symbolic-ref", "HEAD"]),
        "refs/heads/other"
    );
    let removed = report.recording.entry();
    let moves = removed
        .ref_moves
        .as_ref()
        .expect("Remove must observe manager HEAD");
    assert!(
        moves.iter().any(|m| {
            m.refname == "HEAD"
                && m.old_symbolic.as_deref() == Some("refs/heads/manager")
                && m.new_symbolic.as_deref() == Some("refs/heads/other")
        }),
        "pre_remove checkout must not disappear into common HEAD: {moves:?}"
    );
    create(&manager, "late");
    let restore = plan(&manager, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&restore).iter().any(|note| matches!(
            note,
            OplogRestoreNote::HeadMoved { id, op, .. }
                if *id == removed.id && op == "remove-worktree"
        )),
        "restore across manager checkout must refuse before deleting later refs: {:?}",
        restore.blockers
    );
    assert!(git_fixture::git_succeeds(
        &manager,
        &["show-ref", "--verify", "refs/heads/late"]
    ));
}

#[test]
fn restoring_three_operations_back_puts_every_branch_where_it_was() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let at_point = branches(&repo);
    commit(&repo, "two\n");
    create(&repo, "b");
    commit(&repo, "three\n");
    assert_ne!(branches(&repo), at_point);

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert!(p.destructive, "two-stage confirm");
    let mut b = backend(&repo);
    b.run(&op, &p).unwrap();

    assert_eq!(branches(&repo), at_point, "main back, b deleted, a kept");
    let entry = newest(&repo);
    assert_eq!(entry.op, "restore-to-point");
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    // Both branches it moved keep their pre-restore tips.
    assert_eq!(entry.backup_refs.len(), 2, "{:?}", entry.backup_refs);
    let moved: Vec<String> = entry
        .ref_moves
        .expect("the restore records its own moves")
        .into_iter()
        .map(|m| m.refname)
        .collect();
    assert!(moved.contains(&"refs/heads/main".to_string()), "{moved:?}");
    assert!(moved.contains(&"refs/heads/b".to_string()), "{moved:?}");
}

/// Reverting the restore puts back exactly what it moved (PM: round trip).
#[test]
fn reverting_a_restore_returns_to_before_it() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    commit(&repo, "two\n");
    create(&repo, "b");
    let before_restore = branches(&repo);

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    backend(&repo).run(&op, &p).unwrap();
    assert_ne!(branches(&repo), before_restore);

    let restore = newest(&repo).id;
    let undo = Operation::OpRevert { entry_id: restore };
    let p = plan(&repo, &undo);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    backend(&repo).run(&undo, &p).unwrap();
    assert_eq!(branches(&repo), before_restore);
    assert_eq!(newest(&repo).op, "op-revert");
}

#[test]
fn reverting_a_middle_operation_keeps_the_later_ones() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    create(&repo, "a");
    let middle = create(&repo, "b");
    create(&repo, "c");

    let op = Operation::OpRevert { entry_id: middle };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    backend(&repo).run(&op, &p).unwrap();
    let names = git_output(
        &repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    );
    assert_eq!(names, "a\nc\nmain", "only b's creation was undone");
}

#[test]
fn a_revert_whose_ref_moved_again_later_is_blocked() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = commit(&repo, "two\n");
    let later = commit(&repo, "three\n");
    let before = branches(&repo);

    let op = Operation::OpRevert { entry_id: first };
    let p = plan(&repo, &op);
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::LaterEntryMoved {
            refname: "refs/heads/main".into(),
            id: later,
            op: "commit".into(),
        }),
        "{:?}",
        p.blockers
    );
    let err = backend(&repo).run(&op, &p).unwrap_err();
    assert!(
        matches!(err, GitError::Other(_) | GitError::Preflight(_)),
        "{err}"
    );
    assert_eq!(branches(&repo), before, "a blocked plan moves nothing");
}

#[test]
fn an_unrecorded_entry_in_the_range_blocks_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    // An entry of this repository written without a record (as before #334
    // slice 2a, or by a path that does not record moves).
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let unrecorded = OpLogEntry::new(
        "unrecorded",
        repo.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    )
    .with_worktree(Some(repo.display().to_string()));
    append_oplog(&unrecorded).unwrap();
    let unrecorded_id = newest(&repo).id;
    create(&repo, "b");

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::NotRecorded {
            id: unrecorded_id,
            op: "unrecorded".into()
        }),
        "{:?}",
        p.blockers
    );
}

#[test]
fn a_checkout_in_the_range_or_a_branch_moved_outside_blocks() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let checkout = run(&repo, Operation::Checkout { branch: "a".into() });
    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    // #886: the blocker names the entry and both sides of the switch.
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::HeadMoved {
            id: checkout,
            op: "checkout".into(),
            from: HeadAt::Branch("main".into()),
            to: HeadAt::Branch("a".into()),
            worktree: None,
        }),
        "{:?}",
        p.blockers
    );
    commit(&repo, "two\n");
    let tip = git_output(&repo, &["rev-parse", "HEAD~1"]);
    let detach = run(
        &repo,
        Operation::CheckoutCommit {
            id: CommitId(tip.clone()),
        },
    );
    let p = plan(&repo, &Operation::OpRevert { entry_id: detach });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::HeadMoved {
            id: detach,
            op: "checkout-commit".into(),
            from: HeadAt::Branch("a".into()),
            to: HeadAt::Detached(tip.into()),
            worktree: None,
        }),
        "{:?}",
        p.blockers
    );

    let tmp = tempfile::tempdir().unwrap();
    let repo = self::repo(tmp.path());
    let created = create(&repo, "a");
    commit_all_on(&repo, "a", "outside\n");
    let p = plan(&repo, &Operation::OpRevert { entry_id: created });
    assert!(
        restore_blockers(&p)
            .iter()
            .any(|n| matches!(n, OplogRestoreNote::RefMovedSince { refname, .. } if refname == "refs/heads/a")),
        "{:?}",
        p.blockers
    );
}

/// Move `branch` with plain git, outside the recorded pipeline.
fn commit_all_on(dir: &Path, branch: &str, content: &str) {
    git(dir, &["checkout", "-q", branch]);
    write_file(dir, "a.txt", content);
    commit_all(dir, "outside");
    git(dir, &["checkout", "-q", "main"]);
}

/// A branch that moves between confirm and execute: preflight re-plans,
/// refuses, and nothing changes.
#[test]
fn a_branch_moved_after_planning_refuses_and_changes_nothing() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    create(&repo, "b");
    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);

    commit_all_on(&repo, "b", "moved after planning\n");
    let after_move = branches(&repo);
    assert!(backend(&repo).run(&op, &p).is_err());
    assert_eq!(branches(&repo), after_move, "refused before any ref moved");
}

#[test]
fn deleting_a_checked_out_branch_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let created = create(&repo, "a");
    let wt = tmp.path().join("wt-a");
    git(&repo, &["worktree", "add", "-q", wt.to_str().unwrap(), "a"]);
    let p = plan(&repo, &Operation::OpRevert { entry_id: created });
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::DeletesCheckedOutBranch { branch, .. } if branch == "a"
        )),
        "{:?}",
        p.blockers
    );
}

/// Sleep into the next wall-clock second, so a later git write's reflog time
/// is strictly after an entry's timestamp.
fn next_second() {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    std::thread::sleep(Duration::from_nanos(
        1_000_000_000 - u64::from(now.subsec_nanos()) + 20_000_000,
    ));
}

/// #878 review: an entry missing from the log (forgotten by retention, lost
/// to a corrupt line) breaks the chain; restoring across it would silently
/// keep whatever that entry did, so it is refused.
#[test]
fn a_missing_entry_in_the_range_blocks_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let forgotten = create(&repo, "b");
    create(&repo, "c");
    let log = PathBuf::from(std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl");
    let kept: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter(|line| !line.contains(&format!("\"id\":{forgotten},")))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&log, kept.concat()).unwrap();

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::HistoryGap {
            after: point,
            next: forgotten + 1
        }),
        "{:?}",
        p.blockers
    );
}

/// Rewrite the recorded `repo_identity` of entry `id` in the log (`None`
/// removes it, as a line written before #894).
fn set_identity(id: u64, identity: Option<serde_json::Value>) {
    let log = PathBuf::from(std::env::var("KAGI_LOG_DIR").unwrap()).join("operations.jsonl");
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value["id"] == id {
                let object = value.as_object_mut().unwrap();
                match &identity {
                    Some(identity) => {
                        object.insert("repo_identity".into(), identity.clone());
                    }
                    None => {
                        object.remove("repo_identity");
                    }
                }
            }
            format!("{value}\n")
        })
        .collect();
    std::fs::write(&log, lines.concat()).unwrap();
}

/// This repository's recorded identity, as JSON (unix: carries dev / ino and
/// the creation time). `None` — the caller skips — on a filesystem that
/// reports no creation time (NFS, some Linux filesystems) or only whole
/// seconds: there the identity is ambiguous by design and these cases cannot
/// be set up (#900 review).
#[cfg(unix)]
fn identity_of(dir: &Path) -> Option<serde_json::Value> {
    let id = kagi_git::oplog::RepoIdentity::of(dir).expect("opens");
    let (dev, ino) = id.file_id.expect("unix file id");
    let Some((born_s, born_ns)) = id.created.filter(|&(_, ns)| ns != 0) else {
        eprintln!(
            "skipped: {} reports no sub-second creation time",
            dir.display()
        );
        return None;
    };
    Some(serde_json::json!({
        "common_dir": id.common_dir, "dev": dev, "ino": ino,
        "born_s": born_s, "born_ns": born_ns,
    }))
}

/// A worktree `wt` on a branch made before `point`, an operation in it after
/// `point`, then the worktree removed and pruned.
fn removed_worktree_entry(tmp: &Path, repo: &Path) -> (u64, u64) {
    git(repo, &["branch", "wtb"]);
    let point = create(repo, "a");
    let wt = tmp.join("wt");
    git(
        repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "wtb"],
    );
    let moved_there = create(&wt, "made-in-wt");
    std::fs::remove_dir_all(&wt).unwrap();
    git(repo, &["worktree", "prune"]);
    (point, moved_there)
}

/// #894: an entry records its repository, so one made in a worktree that is
/// gone is still ours — its move is restored, nothing blocks.
#[test]
fn an_entry_of_a_removed_worktree_is_still_this_repositorys() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let (point, _) = removed_worktree_entry(tmp.path(), &repo);

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert!(
        p.preview_commits
            .iter()
            .any(|line| line.starts_with("restore refs/heads/made-in-wt - ")),
        "the branch made in the removed worktree is deleted: {:?}",
        p.preview_commits
    );
}

/// #878 review, still true for a line written before #894: with no recorded
/// repository and a worktree that cannot be opened, the entry may be ours,
/// so the restore fails closed.
#[test]
fn a_legacy_entry_of_a_removed_worktree_blocks_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let (point, moved_there) = removed_worktree_entry(tmp.path(), &repo);
    set_identity(moved_there, None);

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::UnknownRepository { id, .. } if *id == moved_there
        )),
        "{:?}",
        p.blockers
    );
}

/// #894: an operation in an unrelated repository that has since been deleted
/// is proven another repository's by its recorded identity: it no longer
/// blocks the restore.
#[test]
fn a_deleted_unrelated_repositorys_entry_does_not_block() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    create(&other, "elsewhere");
    create(&repo, "b");
    drop(other_tmp);
    assert!(!other.exists());

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
}

/// #894 limit: a repository moved to another filesystem changes both its
/// path and its `(dev, ino)`, so its own earlier entries read as another
/// repository's. The restore must not then pass silently: the branch those
/// entries moved changed after the point with no record that explains it.
#[test]
fn own_entries_unrecognised_after_a_cross_filesystem_move_fail_closed() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    next_second();
    let moved = create(&repo, "b");
    set_identity(
        moved,
        Some(serde_json::json!({ "common_dir": "/other-volume/repo/.git", "dev": 1, "ino": 1 })),
    );

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::RefChangedOutsideRecord {
            refname: "refs/heads/b".into()
        }),
        "{:?}",
        p.blockers
    );
}

/// #894 limit: an unrelated repository whose `.git` inode number was reused
/// by ours reads as ours. Its record then names refs and OIDs this
/// repository does not have, so the restore stops on them instead of acting.
#[cfg(unix)]
#[test]
fn another_repositorys_entry_misread_as_ours_fails_closed() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    write_file(&other, "a.txt", "only in the other repository\n");
    commit_all(&other, "other");
    let foreign = create(&other, "x");
    let (Some(mut misread), Some(theirs)) = (identity_of(&repo), identity_of(&other)) else {
        return;
    };
    misread["common_dir"] = theirs["common_dir"].clone();
    set_identity(foreign, Some(misread));

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::RefMovedSince { refname, current: None, .. }
                if refname == "refs/heads/x"
        )),
        "{:?}",
        p.blockers
    );
}

/// #900 review: a repository deleted and re-created at the same path is a
/// different repository. An entry that names this path but another file id
/// (here: another repository's, rewritten onto our path) is not ours, so it
/// cannot be a restore point here.
#[cfg(unix)]
#[test]
fn an_entry_at_our_path_with_another_file_id_is_not_ours() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    create(&repo, "a");
    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    let earlier = create(&other, "from-the-old-repository");
    let (Some(mut recreated), Some(ours)) = (identity_of(&other), identity_of(&repo)) else {
        return;
    };
    recreated["common_dir"] = ours["common_dir"].clone();
    set_identity(earlier, Some(recreated));

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: earlier });
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::EntryNotLoaded { id: earlier }],
        "same path, different file id: another repository's entry"
    );
}

/// #900 review: a repository deleted and re-cloned at the same place can get
/// the old `.git` inode back — same path, same `(dev, ino)`. Its different
/// creation time keeps the old entries from being taken for its own; with no
/// creation time to compare, the entry is ambiguous and the restore fails
/// closed.
#[cfg(unix)]
#[test]
fn a_reused_inode_with_another_birth_time_is_not_ours() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    let old = create(&other, "from-the-deleted-clone");
    create(&repo, "b");

    let Some(mut reused) = identity_of(&repo) else {
        return;
    };
    reused["born_s"] = serde_json::json!(1);
    set_identity(old, Some(reused.clone()));
    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: old });
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::EntryNotLoaded { id: old }],
        "same path and inode, another birth: not ours"
    );

    let object = reused.as_object_mut().unwrap();
    object.remove("born_s");
    object.remove("born_ns");
    set_identity(old, Some(reused));
    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::UnknownRepository { id, .. } if *id == old
        )),
        "same inode, no birth time to confirm: ambiguous, fails closed: {:?}",
        p.blockers
    );
}

/// #878 review: a merge in progress in *another* worktree blocks too — moving
/// the branch it builds on would change its HEAD mid-operation.
#[test]
fn an_operation_in_progress_in_another_worktree_blocks() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let wt = tmp.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    write_file(&wt, "a.txt", "side\n");
    commit_all(&wt, "side");
    let point = create(&repo, "x");
    commit(&repo, "main\n");
    assert!(!git_fixture::git_succeeds(&wt, &["merge", "-q", "main"]));

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    let wt_shown = std::fs::canonicalize(&wt).unwrap();
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::OperationInProgress { path, .. }
                if std::fs::canonicalize(path).ok().as_deref() == Some(wt_shown.as_path())
        )),
        "{:?}",
        p.blockers
    );
}

/// #878 review: a branch created after the target outside Kagi (a terminal
/// `git branch`) is in no record, so restoring would leave it: refused.
#[test]
fn a_branch_created_after_the_point_outside_the_record_blocks() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    create(&repo, "b");
    next_second();
    git(&repo, &["branch", "outside"]);

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::RefChangedOutsideRecord {
            refname: "refs/heads/outside".into()
        }],
        "b is explained by its entry; outside by nothing"
    );
}

#[test]
fn a_tag_moved_outside_the_record_with_a_reflog_blocks_restore_to_point() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    commit(&repo, "later\n");
    let second = git_output(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["config", "core.logAllRefUpdates", "always"]);
    git(&repo, &["tag", "release", &first]);
    let point = create(&repo, "point");
    create(&repo, "later");
    let restore = Operation::RestoreToPoint { entry_id: point };
    let approved = plan(&repo, &restore);
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    next_second();
    git(&repo, &["update-ref", "refs/tags/release", &second, &first]);
    assert!(
        repo.join(".git/logs/refs/tags/release").exists(),
        "the external move needs an actual tag reflog"
    );

    let p = plan(&repo, &restore);
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::RefChangedOutsideRecord {
            refname: "refs/tags/release".into()
        }],
        "no recorded operation explains the later tag move"
    );
    assert!(
        backend(&repo).run(&restore, &approved).is_err(),
        "preflight must see the later tag reflog update too"
    );
    assert!(git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/heads/later"]
    ));
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        second
    );
}

#[test]
fn a_tag_without_a_reflog_stays_unchanged_with_an_explicit_restore_warning() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = git_output(&repo, &["rev-parse", "HEAD"]);
    commit(&repo, "later\n");
    let second = git_output(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["config", "core.logAllRefUpdates", "false"]);
    git(&repo, &["tag", "release", &first]);
    let point = create(&repo, "point");
    create(&repo, "later");
    next_second();
    git(&repo, &["update-ref", "refs/tags/release", &second, &first]);
    assert!(
        !repo.join(".git/logs/refs/tags/release").exists(),
        "the target state cannot be proved without a tag reflog"
    );

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert!(
        p.warnings
            .iter()
            .any(|w| w.to_string().contains("tags changed outside Kagi")),
        "the unchanged row must name the unsupported external tag history: {:?}",
        p.warnings
    );
    backend(&repo).run(&op, &p).unwrap();
    assert_eq!(
        git_output(&repo, &["rev-parse", "refs/tags/release"]),
        second
    );
    assert!(!git_fixture::git_succeeds(
        &repo,
        &["show-ref", "--verify", "refs/heads/later"]
    ));
}

/// The fetch receipt as the UI writes it (#885, `fetch_async_for`): the
/// fetch observed by `observe_ref_moves`, recorded through `with_ref_moves`.
fn failed_fetch(dir: &Path, outcome: impl FnOnce(String) -> OpOutcome) -> u64 {
    let (result, moves) = backend(dir).observe_ref_moves(|b| b.fetch_remote());
    let error = result.err().expect("the remote does not exist").to_string();
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let entry = OpLogEntry::new("fetch", dir.display().to_string(), state, outcome(error))
        .with_worktree(Some(dir.display().to_string()))
        .with_ref_moves(moves);
    append_oplog(&entry).unwrap();
    newest(dir).id
}

/// #885: a fetch moves no local branch, and its receipt says so
/// (`Some(empty)`), so a restore to a point before it is not blocked as
/// "not recorded"; the branch made after it is still taken back.
#[test]
fn a_fetch_in_the_range_does_not_block_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let missing = tmp.path().join("no-such-remote");
    git(
        &repo,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    let point = create(&repo, "a");
    let at_point = branches(&repo);
    let fetch = failed_fetch(&repo, |error| OpOutcome::Failed { error });
    assert_eq!(newest(&repo).ref_moves, Some(Vec::new()), "fetch {fetch}");
    create(&repo, "b");

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert_eq!(restore_blockers(&p), Vec::new(), "{:?}", p.blockers);
    backend(&repo).run(&op, &p).unwrap();
    assert_eq!(branches(&repo), at_point, "b taken back across the fetch");
}

/// #885 / #891 review: a fetch whose termination is unconfirmed may still be
/// writing refs, so it is not recorded however little was observed, and a
/// restore across it stays blocked.
#[test]
fn a_fetch_of_unknown_termination_still_blocks_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let missing = tmp.path().join("no-such-remote");
    git(
        &repo,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    let point = create(&repo, "a");
    let fetch = failed_fetch(&repo, |evidence| OpOutcome::Unknown {
        after: StateSummary {
            head: "unknown".into(),
            dirty: "unknown".into(),
        },
        evidence,
    });
    create(&repo, "b");

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::NotRecorded {
            id: fetch,
            op: "fetch".into()
        }),
        "{:?}",
        p.blockers
    );
}

/// Remove the linked worktree `wt` (on `wtb`) through the recorded path and
/// return its receipt.
fn remove_wt(dir: &Path, delete_branch: bool) -> OpLogEntry {
    let plan = Backend::plan_recorded_remove(dir, "wt", delete_branch).unwrap();
    let report = Backend::run_recorded_remove(&plan, kagi_git::oplog::Actor::Human, None);
    let entry = newest(dir);
    assert_eq!(entry.op, "remove-worktree");
    assert!(
        matches!(entry.outcome, OpOutcome::Success { .. }),
        "{:?} / {:?}",
        entry.outcome,
        report.progress
    );
    entry
}

/// #885 (#900 review): a remove-worktree records the refs it moved,
/// observed, not assumed — nothing when the branch is kept, the branch's
/// deletion when it is not. With #900 the removed worktree's entries are this
/// repository's, so a restore across both removes has no blocker at all, and
/// reverting the deleting remove is planned in this repository.
#[test]
fn a_remove_worktree_records_what_it_moved() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    git(&repo, &["branch", "wtb"]);
    let tip = git_output(&repo, &["rev-parse", "wtb"]);
    let point = create(&repo, "a");
    let wt = tmp.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "wtb"],
    );
    let kept = remove_wt(&repo, false);
    assert_eq!(
        kept.ref_moves,
        Some(Vec::new()),
        "branch kept: nothing moved"
    );

    git(
        &repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "wtb"],
    );
    let deleted = remove_wt(&repo, true);
    let moves = deleted.ref_moves.clone().expect("observed");
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert_eq!(moves[0].refname, "refs/heads/wtb");
    assert_eq!(moves[0].old.as_deref(), Some(tip.as_str()));
    assert_eq!(moves[0].new, None, "deleted");

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert_eq!(restore_blockers(&p), Vec::new(), "{:?}", p.blockers);

    let p = plan(
        &repo,
        &Operation::OpRevert {
            entry_id: deleted.id,
        },
    );
    let notes = restore_blockers(&p);
    assert!(
        !notes
            .iter()
            .any(|n| matches!(n, OplogRestoreNote::EntryNotLoaded { .. })),
        "the remove is this repository's entry: {notes:?}"
    );
    assert_eq!(notes, Vec::new(), "{notes:?}");
}

/// #888: a restore reads the newest 1000 entries. A target older than that
/// says so — not "another repository's" — and is still refused (the range
/// is not widened). An id that is nowhere in the log keeps the old note, and
/// so does an old entry of another repository (#910 review): the entry found
/// in the whole log is attributed like the tail before it is called too old.
#[test]
fn an_entry_older_than_the_read_window_says_so() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let old = create(&repo, "a");
    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    let others_old = create(&other, "x");
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let elsewhere = OpLogEntry::new(
        "checkout",
        "/elsewhere",
        state.clone(),
        OpOutcome::Success { after: state },
    );
    for _ in 0..1000 {
        append_oplog(&elsewhere).unwrap();
    }

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: old });
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::EntryOutsideWindow {
            id: old,
            window: 1000
        }]
    );
    let p = plan(
        &repo,
        &Operation::RestoreToPoint {
            entry_id: others_old,
        },
    );
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::EntryNotLoaded { id: others_old }],
        "another repository's old entry is not this repository's"
    );
    let missing = 1_000_000;
    let p = plan(&repo, &Operation::OpRevert { entry_id: missing });
    assert_eq!(
        restore_blockers(&p),
        vec![OplogRestoreNote::EntryNotLoaded { id: missing }]
    );
}

/// #900 review: a successful remove-worktree records the worktree it just
/// deleted, which no longer opens; its identity is taken from the repository
/// it ran from instead. Without that the entry is UnknownRepository and
/// blocks a restore in *every* repository; with it, another repository
/// restores across it freely, and its own repository sees its own entry.
#[test]
fn a_removed_worktrees_own_entry_is_attributed_through_its_repository() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let at_point = branches(&repo);

    let other_tmp = tempfile::tempdir().unwrap();
    let other = self::repo(other_tmp.path());
    git(&other, &["branch", "wtb"]);
    let other_point = create(&other, "c");
    let wt = other_tmp.path().join("wt");
    git(
        &other,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "wtb"],
    );
    let removal = Backend::plan_recorded_remove(&other, "wt", false).unwrap();
    let report = Backend::run_recorded_remove(&removal, kagi_git::oplog::Actor::Human, None);
    assert!(!wt.exists(), "removed: {:?}", report.progress);
    let removed = newest(&other);
    assert_eq!(removed.op, "remove-worktree");
    assert!(matches!(removed.outcome, OpOutcome::Success { .. }));
    create(&repo, "b");

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert_eq!(restore_blockers(&p), Vec::new(), "{:?}", p.blockers);
    backend(&repo).run(&op, &p).unwrap();
    assert_eq!(branches(&repo), at_point);

    let p = plan(
        &other,
        &Operation::RestoreToPoint {
            entry_id: other_point,
        },
    );
    let notes = restore_blockers(&p);
    assert!(
        !notes
            .iter()
            .any(|n| matches!(n, OplogRestoreNote::UnknownRepository { .. })),
        "its own repository knows the entry: {notes:?}"
    );
}

/// #900 review: an identity field that is there but unreadable is not an
/// older line. Read as one, its worktree path — which opens onto this
/// repository, possibly re-created there — would make it ours; instead the
/// restore across it fails closed. An older line without the field is still
/// attributed by its path.
#[test]
fn an_unreadable_identity_fails_closed_instead_of_reading_as_legacy() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let entry = create(&repo, "b");
    let restore = Operation::RestoreToPoint { entry_id: point };

    for invalid in [
        serde_json::json!("not an identity"),
        serde_json::json!({ "common_dir": "/x/.git", "dev": 1 }),
        serde_json::json!({ "common_dir": "/x/.git", "generation": 2 }),
    ] {
        set_identity(entry, Some(invalid.clone()));
        let p = plan(&repo, &restore);
        assert!(
            restore_blockers(&p).iter().any(|n| matches!(
                n,
                OplogRestoreNote::UnknownRepository { id, .. } if *id == entry
            )),
            "{invalid}: {:?}",
            p.blockers
        );
    }

    set_identity(entry, None);
    let p = plan(&repo, &restore);
    assert_eq!(
        restore_blockers(&p),
        Vec::new(),
        "legacy: attributed by its path"
    );
}

/// #900 review: a remove-worktree entry names the repository it ran in,
/// read before anything ran. Here the repository is moved away and another
/// one created at its path while the remove is under way; the entry must
/// keep the original's identity, not take the newcomer's (the removed
/// worktree no longer opens, so an identity read at append time would come
/// from whatever sits at `plan.repo` then).
#[test]
fn a_remove_worktree_entry_keeps_the_identity_read_before_it_ran() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    git(&repo, &["branch", "wtb"]);
    let wt = tmp.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "wtb"],
    );
    let original = kagi_git::oplog::RepoIdentity::of(&repo).unwrap();

    let plan = Backend::plan_recorded_remove(&repo, "wt", false).unwrap();
    let report = Backend::run_recorded_remove_with_events(
        &plan,
        kagi_git::oplog::Actor::Human,
        None,
        |event| {
            if matches!(
                event,
                kagi_git::backend::remove::RemoveEvent::ExecutionStarting
            ) {
                std::fs::rename(&repo, tmp.path().join("moved")).unwrap();
                std::fs::create_dir(&repo).unwrap();
                init_repo(&repo, "main");
            }
        },
    );
    let newcomer = kagi_git::oplog::RepoIdentity::of(&repo).unwrap();
    assert_ne!(
        newcomer.same_repository(&original),
        kagi_git::oplog::SameRepository::Same
    );
    assert_eq!(
        report.recording.entry().repo_identity,
        kagi_git::oplog::RecordedIdentity::Known(original)
    );
}

/// #907 review: a job that writes and then analyses (the PR-ref fetch, then
/// its commits and diff) records only the write's moves. A branch moved by
/// someone else while the analysis runs is not the job's — recorded, a
/// restore across the job would undo that unrelated change.
#[test]
fn only_the_write_is_observed_not_the_analysis_after_it() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let (result, moves) = backend(&repo).observe_then(
        |_| {
            git(&repo, &["branch", "written"]);
            Ok(())
        },
        |_, ()| {
            git(&repo, &["branch", "moved-meanwhile"]);
            Ok(())
        },
    );
    result.unwrap();
    let moved: Vec<String> = moves
        .expect("observed")
        .into_iter()
        .map(|m| m.refname)
        .collect();
    assert_eq!(moved, vec!["refs/heads/written".to_string()]);
}
