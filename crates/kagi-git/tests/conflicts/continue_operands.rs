//! Bind only the external operands actually consumed by the frozen replay.
use super::continue_metadata::{assert_stale_approval, execute_fresh, saved_fixture};
use super::*;

fn merges_fixture() -> TempDir {
    let fixture = rebase_conflict_repo();
    git(fixture.path(), &["rebase", "--abort"]);
    assert!(!git_allow_fail(
        fixture.path(),
        &["rebase", "--rebase-merges", "main"]
    ));
    fixture
}

fn external_target(path: &Path) -> (git2::Oid, git2::Oid) {
    let repo = Repository::open(path).unwrap();
    let parent = repo
        .find_reference("refs/heads/main")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let parent_tree = parent.tree().unwrap();
    let mut tree = repo.treebuilder(Some(&parent_tree)).unwrap();
    tree.insert(
        "aux.txt",
        repo.blob(b"EXTERNAL OPERAND CONTENT\n").unwrap(),
        0o100644,
    )
    .unwrap();
    let tree = repo.find_tree(tree.write().unwrap()).unwrap();
    let signature = repo.signature().unwrap();
    let target = repo
        .commit(
            None,
            &signature,
            &signature,
            "external operand commit",
            &tree,
            &[&parent],
        )
        .unwrap();
    (parent.id(), target)
}

fn combined_tree(path: &Path, saved_tree: &str) -> String {
    let repo = Repository::open(path).unwrap();
    let saved = repo
        .find_tree(git2::Oid::from_str(saved_tree).unwrap())
        .unwrap();
    let mut builder = repo.treebuilder(Some(&saved)).unwrap();
    builder
        .insert(
            "aux.txt",
            repo.blob(b"EXTERNAL OPERAND CONTENT\n").unwrap(),
            0o100644,
        )
        .unwrap();
    builder.write().unwrap().to_string()
}

#[derive(Clone, Copy, PartialEq)]
enum Instruction {
    Reset,
    Merge,
    Pick,
}

fn configure_target(path: &Path, symbolic_label: bool, original: git2::Oid) -> &'static str {
    let target_name = if symbolic_label {
        "refs/heads/label-target"
    } else {
        "refs/heads/external-target"
    };
    let original = original.to_string();
    git(path, &["update-ref", target_name, &original]);
    git(path, &["update-ref", "refs/heads/unrelated", &original]);
    if symbolic_label {
        git(
            path,
            &[
                "symbolic-ref",
                "refs/rewritten/external-target",
                target_name,
            ],
        );
    } else {
        // A packed fallback must be resolved just like a loose branch.
        git(path, &["pack-refs", "--all"]);
    }
    target_name
}

fn assert_operand_commit(path: &Path, instruction: Instruction, target: git2::Oid, tree: &str) {
    let repo = Repository::open(path).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    if instruction == Instruction::Reset {
        assert_eq!(head.id(), target);
    } else {
        assert_eq!(head.tree_id().to_string(), tree);
        if instruction == Instruction::Merge {
            assert_eq!(head.parent_count(), 2);
            assert_eq!(head.parent_id(1).unwrap(), target);
        } else {
            assert_eq!(
                head.message().unwrap().trim_end(),
                "external operand commit"
            );
        }
    }
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
    assert_eq!(git_output(path, &["branch", "--show-current"]), "side");
}

fn changed_external_operand(instruction: Instruction, symbolic_label: bool) {
    let (fixture, backend) = saved_fixture(merges_fixture());
    let path = fixture.path();
    let (original_target, changed_target) = external_target(path);
    let target_name = configure_target(path, symbolic_label, original_target);
    let source = git_output(path, &["rev-parse", "REBASE_HEAD"]);
    let todo = match instruction {
        Instruction::Reset => "reset external-target\n".to_owned(),
        Instruction::Merge => format!("merge -C {source} external-target\n"),
        Instruction::Pick => "pick external-target ignored subject\n".to_owned(),
    };
    let todo_path = path.join(".git/rebase-merge/git-rebase-todo");
    std::fs::write(&todo_path, &todo).unwrap();
    let saved_tree = git_output(path, &["write-tree"]);
    let merged_tree = combined_tree(path, &saved_tree);
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    backend
        .plan_conflict_continue_route(
            &approved.session,
            &buffer,
            "side",
            &approved.observation.revision,
        )
        .unwrap();
    git(
        path,
        &[
            "update-ref",
            "refs/heads/unrelated",
            &changed_target.to_string(),
        ],
    );
    assert_eq!(
        backend
            .conflict_snapshot()
            .unwrap()
            .unwrap()
            .observation
            .revision,
        approved.observation.revision
    );
    git(
        path,
        &["update-ref", target_name, &changed_target.to_string()],
    );
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_ne!(current.observation.revision, approved.observation.revision);
    assert_eq!(std::fs::read_to_string(&todo_path).unwrap(), todo);
    assert_stale_approval(path, &backend, &approved, &buffer);
    execute_fresh(&backend, &current, &buffer);
    assert_operand_commit(path, instruction, changed_target, &merged_tree);
}

#[test]
fn reset_fallback_ref_movement_requires_new_approval_not_unrelated_refs() {
    if !test_support::run_isolated() {
        return;
    }
    changed_external_operand(Instruction::Reset, false);
}

#[test]
fn merge_fallback_ref_movement_requires_new_approval_not_unrelated_refs() {
    if !test_support::run_isolated() {
        return;
    }
    changed_external_operand(Instruction::Merge, false);
}

#[test]
fn symbolic_pick_operand_movement_requires_new_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_external_operand(Instruction::Pick, false);
}

#[test]
fn rewritten_symbolic_label_target_movement_requires_new_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_external_operand(Instruction::Reset, true);
}

#[test]
fn earlier_pending_label_is_operation_produced_not_a_missing_external_ref() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend) = saved_fixture(merges_fixture());
    let path = fixture.path();
    let tree = git_output(path, &["write-tree"]);
    let repo = Repository::open(path).unwrap();
    assert!(repo
        .find_reference("refs/rewritten/pending-result")
        .is_err());
    std::fs::write(
        repo.path().join("rebase-merge/git-rebase-todo"),
        "label pending-result\nreset pending-result\n",
    )
    .unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    backend
        .plan_conflict_continue_route(
            &approved.session,
            &buffer,
            "side",
            &approved.observation.revision,
        )
        .unwrap();
    backend
        .execute_conflict_continue(&approved.session, &buffer, &approved.observation.revision)
        .unwrap();
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn deleted_external_operand_ref_fails_closed_without_writes() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend) = saved_fixture(merges_fixture());
    let path = fixture.path();
    let target = git_output(path, &["rev-parse", "HEAD"]);
    git(path, &["update-ref", "refs/heads/external-target", &target]);
    std::fs::write(
        path.join(".git/rebase-merge/git-rebase-todo"),
        "reset external-target\n",
    )
    .unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    git(path, &["update-ref", "-d", "refs/heads/external-target"]);
    assert!(backend.conflict_snapshot().is_err());
    assert_stale_approval(path, &backend, &approved, &buffer);
}

#[test]
fn symbolic_squash_origin_is_resolved_not_only_hashed_as_text() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend) = saved_fixture(rebase_conflict_repo());
    let path = fixture.path();
    let (original, changed) = external_target(path);
    git(
        path,
        &[
            "update-ref",
            "refs/heads/squash-origin",
            &original.to_string(),
        ],
    );
    std::fs::write(
        path.join(".git/rebase-merge/squash-onto"),
        "refs/heads/squash-origin\n",
    )
    .unwrap();
    let tree = git_output(path, &["write-tree"]);
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    git(
        path,
        &[
            "update-ref",
            "refs/heads/squash-origin",
            &changed.to_string(),
        ],
    );
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_ne!(current.observation.revision, approved.observation.revision);
    assert_stale_approval(path, &backend, &approved, &buffer);
    backend
        .plan_conflict_continue_route(
            &current.session,
            &buffer,
            "side",
            &current.observation.revision,
        )
        .unwrap();
    backend
        .execute_conflict_continue(&current.session, &buffer, &current.observation.revision)
        .unwrap();
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn processed_update_ref_still_binds_the_final_compare_and_swap_target() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_two_conflicting_steps_repo();
    let path = fixture.path();
    git(path, &["rebase", "--abort"]);
    git(path, &["branch", "first-step-alias", "side~1"]);
    assert!(!git_allow_fail(path, &["rebase", "--update-refs", "main"]));
    write_file(path, "file.txt", "FIRST REWRITTEN RESOLUTION\n");
    git(path, &["add", "file.txt"]);
    assert!(!git_allow_fail(
        path,
        &["-c", "core.editor=true", "rebase", "--continue"]
    ));
    let first_rewritten = git_output(path, &["rev-parse", "HEAD"]);
    let original_alias = git_output(path, &["rev-parse", "first-step-alias"]);
    assert!(
        !std::fs::read_to_string(path.join(".git/rebase-merge/git-rebase-todo"))
            .unwrap()
            .contains("first-step-alias")
    );
    let (fixture, backend) = saved_fixture(fixture);
    let path = fixture.path();
    let tree = git_output(path, &["write-tree"]);
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let main = git_output(path, &["rev-parse", "main"]);
    assert_ne!(main, original_alias);
    git(path, &["update-ref", "refs/heads/first-step-alias", &main]);
    let changed = backend.conflict_snapshot().unwrap().unwrap();
    assert_ne!(changed.observation.revision, approved.observation.revision);
    assert_stale_approval(path, &backend, &approved, &buffer);
    // Restore the expected old target: a new approval must still execute Git's
    // real final CAS, rather than accepting a known inconsistent update-ref.
    git(
        path,
        &["update-ref", "refs/heads/first-step-alias", &original_alias],
    );
    let current = backend.conflict_snapshot().unwrap().unwrap();
    backend
        .plan_conflict_continue_route(
            &current.session,
            &buffer,
            "side",
            &current.observation.revision,
        )
        .unwrap();
    backend
        .execute_conflict_continue(&current.session, &buffer, &current.observation.revision)
        .unwrap();
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(path, &["rev-parse", "first-step-alias"]),
        first_rewritten
    );
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn malformed_or_unknown_todo_operands_cannot_be_approved() {
    if !test_support::run_isolated() {
        return;
    }
    for todo in [
        "unknown-command target\n",
        "reset [new\troot]\n",
        "merge # missing-target\n",
        "pick missing-ref subject\n",
    ] {
        let (fixture, backend) = saved_fixture(merges_fixture());
        let path = fixture.path();
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let approved = backend.conflict_snapshot().unwrap().unwrap();
        std::fs::write(path.join(".git/rebase-merge/git-rebase-todo"), todo).unwrap();
        assert!(backend.conflict_snapshot().is_err());
        assert_stale_approval(path, &backend, &approved, &buffer);
    }
}
