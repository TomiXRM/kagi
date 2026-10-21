use super::*;
use crate::backend::ExecutionPolicy;
use crate::{Backend, Operation};

fn fixture(log: bool) -> (tempfile::TempDir, Repository, git2::Oid) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.logAllRefUpdates", log)
        .unwrap();
    repo.set_head("refs/heads/main").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tip = {
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap()
    };
    repo.reference("refs/heads/victim", tip, false, "fixture branch")
        .unwrap();
    (dir, repo, tip)
}

#[test]
fn commit_failure_after_reflog_removal_records_partial_and_recovery() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    let log = repo.path().join("logs/refs/heads/victim");
    assert!(log.exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    assert!(plan.blockers.is_empty());
    FAULT.with(|fault| fault.set(Some(DeleteFault::CommitFailure)));
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report
        .result
        .unwrap_err()
        .to_string()
        .contains("injected branch ref transaction failure"));
    assert!(!log.exists());
    assert_eq!(
        repo.find_reference("refs/heads/victim").unwrap().target(),
        Some(tip)
    );
    let entry = report.recording.entry();
    let crate::oplog::OpOutcome::Partial { after, error } = &entry.outcome else {
        panic!("{entry:?}");
    };
    assert!(after.dirty.contains("reflog removed"));
    assert!(error.contains("transaction failure"));
    assert_eq!(entry.backup_refs.len(), 1);
    assert_eq!(
        repo.find_reference(&entry.backup_refs[0]).unwrap().target(),
        Some(tip)
    );
    let persisted = crate::oplog::read_oplog_tail_for_repo(dir.path(), 10);
    assert!(persisted.iter().any(|e| e.id == entry.id
        && matches!(e.outcome, crate::oplog::OpOutcome::Partial { .. })
        && e.backup_refs == entry.backup_refs));
}

#[test]
fn branch_without_reflog_deletes_and_retains_tip() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(false);
    assert!(!repo.path().join("logs/refs/heads/victim").exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_ok());
    assert!(matches!(
        report.recording.entry().outcome,
        crate::oplog::OpOutcome::Success { .. }
    ));
    assert!(repo.find_reference("refs/heads/victim").is_err());
    assert_eq!(
        repo.find_reference(&report.recording.entry().backup_refs[0])
            .unwrap()
            .target(),
        Some(tip)
    );
}

#[test]
fn directly_written_ref_without_reflog_is_deletable() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    std::fs::write(repo.path().join("refs/heads/raw"), format!("{tip}\n")).unwrap();
    assert!(!repo.path().join("logs/refs/heads/raw").exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("raw").unwrap();
    let report = backend.run_recorded(&Operation::DeleteBranch { name: "raw".into() }, &plan);
    assert!(report.result.is_ok());
    assert!(repo.find_reference("refs/heads/raw").is_err());
}

#[test]
fn reflog_errors_other_than_not_found_keep_branch_and_fail() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    let path = repo.path().join("logs/refs/heads/victim");
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    FAULT.with(|fault| fault.set(Some(DeleteFault::ReflogFailure)));
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_err());
    assert_eq!(
        repo.find_reference("refs/heads/victim").unwrap().target(),
        Some(tip)
    );
    assert!(path.is_file());
}

fn unmerged_fixture() -> (tempfile::TempDir, Repository, git2::Oid) {
    let (dir, repo, base) = fixture(true);
    let tip = {
        let blob = repo.blob(b"branch-only work\n").unwrap();
        let mut tree = repo.treebuilder(None).unwrap();
        tree.insert("branch-only.txt", blob, 0o100644).unwrap();
        let tree = repo.find_tree(tree.write().unwrap()).unwrap();
        let parent = repo.find_commit(base).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        repo.commit(
            Some("refs/heads/victim"),
            &sig,
            &sig,
            "unique",
            &tree,
            &[&parent],
        )
        .unwrap()
    };
    (dir, repo, tip)
}

fn loss_count(plan: &OperationPlan) -> usize {
    plan.warnings
        .iter()
        .find_map(|note| match note {
            PlanNote::Branch(kagi_domain::plan_note::BranchNote::DeleteUnmerged {
                commits,
                ..
            }) => Some(*commits),
            _ => None,
        })
        .expect("fixture must take the unmerged delete route")
}

fn refs(repo: &Repository) -> Vec<(String, Option<git2::Oid>, Option<String>)> {
    let mut refs: Vec<_> = repo
        .references()
        .unwrap()
        .map(|reference| {
            let reference = reference.unwrap();
            (
                reference.name().unwrap().to_owned(),
                reference.target(),
                reference.symbolic_target().unwrap().map(str::to_owned),
            )
        })
        .collect();
    refs.sort();
    refs
}

#[test]
fn review_1695_delete_restore_replan_still_loses_ordinary_reachability() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = unmerged_fixture();
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    assert_eq!(loss_count(&plan), 1);
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.recording.entry().backup_refs.len(), 1);
    repo.reference("refs/heads/victim", tip, false, "restore deleted branch")
        .unwrap();
    assert_eq!(
        loss_count(&backend.plan_delete_branch("victim").unwrap()),
        1
    );
}

#[test]
fn review_1695_recovery_namespaces_and_symbolic_aliases_are_not_retention() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for recovery in [
        "refs/kagi/backups/attempt/0",
        "refs/kagi/snapshots/savepoint",
        "refs/kagi/recovery/attempt",
        "refs/kagi/oplog/attempt",
        "refs/backups/attempt",
        "refs/snapshots/savepoint",
        "refs/recovery/attempt",
        "refs/oplog/attempt",
    ] {
        let (dir, repo, tip) = unmerged_fixture();
        repo.reference(recovery, tip, false, "recovery retention")
            .unwrap();
        repo.reference_symbolic("refs/heads/recovery-alias-hop", recovery, false, "alias")
            .unwrap();
        repo.reference_symbolic(
            "refs/heads/recovery-alias",
            "refs/heads/recovery-alias-hop",
            false,
            "alias chain",
        )
        .unwrap();
        let backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
        assert_eq!(
            loss_count(&backend.plan_delete_branch("victim").unwrap()),
            1,
            "{recovery}"
        );
    }
}

#[test]
fn review_1695_ordinary_refs_still_retain_commits() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for retained in [
        "refs/heads/retained",
        "refs/tags/retained",
        "refs/remotes/origin/retained",
        "refs/kagi/pr/123",
    ] {
        let (dir, repo, tip) = unmerged_fixture();
        repo.reference(retained, tip, false, "ordinary retention")
            .unwrap();
        let backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
        assert_eq!(
            loss_count(&backend.plan_delete_branch("victim").unwrap()),
            0,
            "{retained}"
        );
    }
}

#[test]
fn review_1695_changed_retention_ref_refuses_before_writes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for move_ref in [false, true] {
        let (dir, repo, tip) = unmerged_fixture();
        repo.reference("refs/heads/retained", tip, false, "ordinary retention")
            .unwrap();
        let staged = dir.path().join("staged.txt");
        std::fs::write(&staged, b"staged content\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        std::fs::write(&staged, b"unstaged content\n").unwrap();
        let untracked = dir.path().join("untracked.txt");
        std::fs::write(&untracked, b"untracked content\n").unwrap();
        let mut backend =
            Backend::open_with_policy(dir.path(), ExecutionPolicy::human(true)).unwrap();
        let plan = backend.plan_delete_branch("victim").unwrap();
        assert_eq!(loss_count(&plan), 0);
        let head = repo.head().unwrap().target().unwrap();
        if move_ref {
            repo.find_reference("refs/heads/retained")
                .unwrap()
                .set_target(head, "move retained root")
                .unwrap();
        } else {
            repo.find_reference("refs/heads/retained")
                .unwrap()
                .delete()
                .unwrap();
        }
        assert_eq!(
            loss_count(&backend.plan_delete_branch("victim").unwrap()),
            1
        );
        let before_refs = refs(&repo);
        let files = [
            repo.path().join("HEAD"),
            repo.path().join("index"),
            repo.path().join("config"),
            repo.path().join("logs/refs/heads/victim"),
            staged,
            untracked,
        ];
        let before_files: Vec<_> = files
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let report = backend.run_recorded(
            &Operation::DeleteBranch {
                name: "victim".into(),
            },
            &plan,
        );
        let error = report.result.unwrap_err();
        assert!(error.is_preflight(), "{error:?}");
        assert_eq!(
            error.blocker(),
            Some(&PlanNote::Branch(
                kagi_domain::plan_note::BranchNote::DeleteReachabilityChanged {
                    name: "victim".into(),
                    approved: 0,
                    current: 1,
                },
            )),
        );
        assert_eq!(refs(&repo), before_refs);
        for (path, before) in files.iter().zip(before_files) {
            assert_eq!(std::fs::read(path).unwrap(), before, "{}", path.display());
        }
        assert!(report.recording.entry().backup_refs.is_empty());
        assert!(matches!(
            report.recording.entry().outcome,
            crate::oplog::OpOutcome::Failed { .. }
        ));
        let persisted = crate::oplog::read_oplog_tail_for_repo(dir.path(), 10);
        assert!(persisted
            .iter()
            .any(|entry| entry.id == report.recording.entry().id));
    }
}

#[test]
fn review_1695_unchanged_zero_loss_plan_still_executes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = unmerged_fixture();
    repo.reference("refs/heads/retained", tip, false, "ordinary retention")
        .unwrap();
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    assert_eq!(loss_count(&plan), 0);
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(repo.find_reference("refs/heads/victim").is_err());
    assert_eq!(
        repo.find_reference("refs/heads/retained").unwrap().target(),
        Some(tip)
    );
    assert_eq!(report.recording.entry().backup_refs.len(), 1);
}

#[test]
fn review_1695_locked_execution_boundary_rechecks_loss_count() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_dir, repo, tip) = unmerged_fixture();
    repo.reference("refs/heads/retained", tip, false, "ordinary retention")
        .unwrap();
    let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
    assert_eq!(loss_count(&plan), 0);
    repo.find_reference("refs/heads/retained")
        .unwrap()
        .delete()
        .unwrap();
    let before_refs = refs(&repo);
    let before_log = std::fs::read(repo.path().join("logs/refs/heads/victim")).unwrap();
    let mut backups = Vec::new();
    let mut partial = None;
    let error = super::super::branch::execute_delete_branch(
        &repo,
        &plan,
        "victim",
        &mut backups,
        &mut partial,
    )
    .unwrap_err();
    assert!(error.is_preflight(), "{error:?}");
    assert_eq!(
        error.blocker(),
        Some(&PlanNote::Branch(
            kagi_domain::plan_note::BranchNote::DeleteReachabilityChanged {
                name: "victim".into(),
                approved: 0,
                current: 1,
            },
        )),
    );
    assert!(backups.is_empty());
    assert!(partial.is_none());
    assert_eq!(refs(&repo), before_refs);
    assert_eq!(
        std::fs::read(repo.path().join("logs/refs/heads/victim")).unwrap(),
        before_log,
    );
}

#[test]
fn review_1695_changed_nonzero_loss_count_also_requires_reapproval() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = unmerged_fixture();
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    assert_eq!(loss_count(&plan), 1);
    repo.reference("refs/tags/retained", tip, false, "ordinary retention")
        .unwrap();
    let before_refs = refs(&repo);
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    let error = report.result.unwrap_err();
    assert!(error.is_preflight(), "{error:?}");
    assert_eq!(
        error.blocker(),
        Some(&PlanNote::Branch(
            kagi_domain::plan_note::BranchNote::DeleteReachabilityChanged {
                name: "victim".into(),
                approved: 1,
                current: 0,
            },
        )),
    );
    assert!(report.recording.entry().backup_refs.is_empty());
    assert_eq!(refs(&repo), before_refs);
}

struct RetentionProbe {
    dir: PathBuf,
    commands: Vec<Vec<String>>,
    attempts: Vec<std::process::Output>,
}

thread_local! {
    static RETENTION_PROBE: std::cell::RefCell<Option<RetentionProbe>> = const {
        std::cell::RefCell::new(None)
    };
}

fn observe_retention_before_commit() {
    RETENTION_PROBE.with(|probe| {
        let mut probe = probe.borrow_mut();
        let probe = probe.as_mut().expect("retention probe must be installed");
        for args in &probe.commands {
            probe.attempts.push(
                std::process::Command::new("git")
                    .current_dir(&probe.dir)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("HOME", &probe.dir)
                    .args(args)
                    .output()
                    .unwrap(),
            );
        }
    });
}

fn assert_retention_locked_until_commit(
    repo: &Repository,
    plan: &OperationPlan,
    commands: Vec<Vec<String>>,
) {
    RETENTION_PROBE.with(|probe| {
        *probe.borrow_mut() = Some(RetentionProbe {
            dir: repo.workdir().unwrap().to_path_buf(),
            commands,
            attempts: Vec::new(),
        });
    });
    FAULT.with(|fault| {
        fault.set(Some(DeleteFault::BeforeCommit(
            observe_retention_before_commit,
        )))
    });
    let mut backups = Vec::new();
    let mut partial = None;
    let result = super::super::branch::execute_delete_branch(
        repo,
        plan,
        "victim",
        &mut backups,
        &mut partial,
    );
    let probe = RETENTION_PROBE.with(|probe| probe.borrow_mut().take().unwrap());
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(probe.attempts.len(), probe.commands.len());
    for (args, attempt) in probe.commands.iter().zip(&probe.attempts) {
        assert!(
            !attempt.status.success(),
            "external git {args:?} invalidated retention before victim commit"
        );
        assert!(
            String::from_utf8_lossy(&attempt.stderr).contains(".lock"),
            "external git {args:?}: {}",
            String::from_utf8_lossy(&attempt.stderr)
        );
    }
    assert!(repo.find_reference("refs/heads/victim").is_err());
    assert_eq!(backups.len(), 1);
    for args in &probe.commands {
        let after = std::process::Command::new("git")
            .current_dir(&probe.dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", &probe.dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            after.status.success(),
            "external git {args:?} stayed blocked after victim commit: {}",
            String::from_utf8_lossy(&after.stderr)
        );
    }
}

#[test]
fn inline_1695_retention_roots_lock_external_delete_until_victim_commit() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for retained in [
        "refs/heads/retained",
        "refs/tags/retained",
        "refs/remotes/origin/retained",
        "refs/kagi/pr/123",
    ] {
        let (_dir, repo, tip) = unmerged_fixture();
        repo.reference(retained, tip, false, "ordinary retention")
            .unwrap();
        let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
        assert_eq!(loss_count(&plan), 0);
        assert_retention_locked_until_commit(
            &repo,
            &plan,
            vec![vec!["update-ref".into(), "-d".into(), retained.into()]],
        );
    }
}

#[test]
fn inline_1695_annotated_retention_tag_cannot_move_before_victim_commit() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_dir, repo, tip) = unmerged_fixture();
    let tag = repo
        .tag(
            "retained",
            &repo.find_object(tip, None).unwrap(),
            &git2::Signature::now("Test", "test@example.com").unwrap(),
            "ordinary annotated retention",
            false,
        )
        .unwrap();
    let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
    assert_eq!(loss_count(&plan), 0);
    assert_retention_locked_until_commit(
        &repo,
        &plan,
        vec![vec![
            "update-ref".into(),
            "refs/tags/retained".into(),
            repo.head().unwrap().target().unwrap().to_string(),
            tag.to_string(),
        ]],
    );
}

#[test]
fn inline_1695_symbolic_retention_chain_locks_alias_hop_and_terminal() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_dir, repo, tip) = unmerged_fixture();
    repo.reference("refs/retention/terminal", tip, false, "retained terminal")
        .unwrap();
    repo.reference_symbolic(
        "refs/retention/hop",
        "refs/retention/terminal",
        false,
        "retention hop",
    )
    .unwrap();
    repo.reference_symbolic(
        "refs/heads/retained",
        "refs/retention/hop",
        false,
        "retention alias",
    )
    .unwrap();
    let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
    assert_eq!(loss_count(&plan), 0);
    assert_retention_locked_until_commit(
        &repo,
        &plan,
        vec![
            vec![
                "update-ref".into(),
                "--no-deref".into(),
                "-d".into(),
                "refs/heads/retained".into(),
            ],
            vec![
                "symbolic-ref".into(),
                "refs/retention/hop".into(),
                "refs/heads/main".into(),
            ],
            vec![
                "update-ref".into(),
                "-d".into(),
                "refs/retention/terminal".into(),
            ],
        ],
    );
}

#[test]
fn inline_1695_retention_lock_conflict_refuses_zero_loss_before_writes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for locked in ["refs/heads/retained", "refs/retention/terminal"] {
        let (_dir, repo, tip) = unmerged_fixture();
        repo.reference("refs/retention/terminal", tip, false, "retained terminal")
            .unwrap();
        repo.reference_symbolic(
            "refs/heads/retained",
            "refs/retention/terminal",
            false,
            "retention alias",
        )
        .unwrap();
        let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
        assert_eq!(loss_count(&plan), 0);
        let before_refs = refs(&repo);
        let files = [
            repo.path().join("HEAD"),
            repo.path().join("config"),
            repo.path().join("logs/refs/heads/victim"),
        ];
        let before_files: Vec<_> = files
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let mut held = repo.transaction().unwrap();
        held.lock_ref(locked).unwrap();
        let mut backups = Vec::new();
        let mut partial = None;
        let result = super::super::branch::execute_delete_branch(
            &repo,
            &plan,
            "victim",
            &mut backups,
            &mut partial,
        );
        assert!(
            result.is_err(),
            "deletion ignored held retention lock {locked}"
        );
        assert!(result.unwrap_err().to_string().contains("lock"));
        assert!(backups.is_empty());
        assert!(partial.is_none());
        assert_eq!(refs(&repo), before_refs);
        for (path, before) in files.iter().zip(before_files) {
            assert_eq!(std::fs::read(path).unwrap(), before, "{}", path.display());
        }
        drop(held);
    }
}

#[test]
fn inline_1695_nonzero_loss_ignores_locked_recovery_and_noncommit_refs() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_dir, repo, tip) = unmerged_fixture();
    let recovery = "refs/kagi/backups/retained/0";
    repo.reference(recovery, tip, false, "excluded recovery")
        .unwrap();
    repo.reference_symbolic(
        "refs/heads/recovery-alias",
        recovery,
        false,
        "excluded alias",
    )
    .unwrap();
    repo.reference(
        "refs/tags/blob",
        repo.blob(b"not a commit").unwrap(),
        false,
        "noncommit root",
    )
    .unwrap();
    let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
    assert_eq!(loss_count(&plan), 1);
    let mut held = repo.transaction().unwrap();
    for excluded in [recovery, "refs/heads/recovery-alias", "refs/tags/blob"] {
        held.lock_ref(excluded).unwrap();
    }
    let mut backups = Vec::new();
    let mut partial = None;
    let result = super::super::branch::execute_delete_branch(
        &repo,
        &plan,
        "victim",
        &mut backups,
        &mut partial,
    );
    assert!(result.is_ok(), "{result:?}");
    assert!(repo.find_reference("refs/heads/victim").is_err());
    assert_eq!(backups.len(), 1);
}

#[test]
fn inline_1695_partial_retention_stays_locked_for_nonzero_loss() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (_dir, repo, retained) = unmerged_fixture();
    let parent = repo.find_commit(retained).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    repo.commit(
        Some("refs/heads/victim"),
        &sig,
        &sig,
        "additional branch-only commit",
        &parent.tree().unwrap(),
        &[&parent],
    )
    .unwrap();
    repo.reference("refs/tags/retained", retained, false, "partial retention")
        .unwrap();
    let plan = super::super::branch::plan_delete_branch(&repo, "victim").unwrap();
    assert_eq!(loss_count(&plan), 1);
    assert_retention_locked_until_commit(
        &repo,
        &plan,
        vec![vec![
            "update-ref".into(),
            "-d".into(),
            "refs/tags/retained".into(),
        ]],
    );
}
