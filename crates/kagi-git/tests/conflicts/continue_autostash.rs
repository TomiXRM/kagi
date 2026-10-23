//! Real autostashes are operation inputs; ordinary unrelated refs are not.
use super::continue_metadata::{assert_stale_approval, saved_fixture};
use super::*;

#[derive(Clone, Copy)]
enum Mode {
    Merge,
    Rebase,
    Apply,
}

fn autostash_fixture(mode: Mode) -> (TempDir, String) {
    let fixture = match mode {
        Mode::Merge => merge_conflict_repo(),
        _ => rebase_conflict_repo(),
    };
    let path = fixture.path();
    match mode {
        Mode::Merge => git(path, &["merge", "--abort"]),
        _ => git(path, &["rebase", "--abort"]),
    }
    write_file(path, "aux.txt", "BASE AUTOSTASH TRACKED\n");
    git(path, &["add", "aux.txt"]);
    git(path, &["commit", "-qm", "track autostash auxiliary file"]);
    write_file(path, "aux.txt", "ALTERNATE AUTOSTASH WORKTREE\n");
    let alternate = git_output(path, &["stash", "create", "alternate operation autostash"]);
    git2::Oid::from_str(&alternate).unwrap();
    write_file(path, "aux.txt", "ORIGINAL AUTOSTASH WORKTREE\n");
    let args: &[&str] = match mode {
        Mode::Merge => &["merge", "--autostash", "feature"],
        Mode::Rebase => &["rebase", "--autostash", "main"],
        Mode::Apply => &["rebase", "--apply", "--autostash", "main"],
    };
    assert!(!git_allow_fail(path, args));
    (fixture, alternate)
}

fn expected_rebase_tree(path: &Path) -> String {
    let repo = Repository::open(path).unwrap();
    let saved = git_output(path, &["write-tree"]);
    let saved = repo
        .find_tree(git2::Oid::from_str(&saved).unwrap())
        .unwrap();
    let mut tree = repo.treebuilder(Some(&saved)).unwrap();
    tree.insert(
        "aux.txt",
        repo.blob(b"BASE AUTOSTASH TRACKED\n").unwrap(),
        0o100644,
    )
    .unwrap();
    tree.write().unwrap().to_string()
}

fn rebase_autostash_operand(mode: Mode, relative: &str) {
    let (fixture, alternate) = autostash_fixture(mode);
    let (fixture, backend) = saved_fixture(fixture);
    let path = fixture.path();
    let metadata = path.join(".git").join(relative);
    let original = std::fs::read_to_string(&metadata).unwrap();
    git2::Oid::from_str(original.trim()).unwrap();
    assert_ne!(original.trim(), alternate);
    git(
        path,
        &[
            "update-ref",
            "refs/heads/selected-autostash",
            original.trim(),
        ],
    );
    std::fs::write(&metadata, "refs/heads/selected-autostash\n").unwrap();
    let tree = expected_rebase_tree(path);
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
        &["update-ref", "refs/heads/selected-autostash", &alternate],
    );
    assert_eq!(
        std::fs::read_to_string(&metadata).unwrap(),
        "refs/heads/selected-autostash\n"
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
        std::fs::read_to_string(path.join("aux.txt")).unwrap(),
        "ALTERNATE AUTOSTASH WORKTREE\n"
    );
    assert_eq!(git_output(path, &["diff", "--name-only"]), "aux.txt");
    assert!(git_output(path, &["diff", "--cached", "--name-only"]).is_empty());
    assert_eq!(
        git_output(path, &["rev-list", "--count", "main..HEAD"]),
        "2"
    );
    assert_eq!(git_output(path, &["branch", "--show-current"]), "side");
    assert!(!metadata.exists());
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn merge_backend_rebase_autostash_operand_requires_new_approval_and_applies_correct_stash() {
    if !test_support::run_isolated() {
        return;
    }
    rebase_autostash_operand(Mode::Rebase, "rebase-merge/autostash");
}

#[test]
fn apply_backend_rebase_autostash_operand_requires_new_approval_and_applies_correct_stash() {
    if !test_support::run_isolated() {
        return;
    }
    rebase_autostash_operand(Mode::Apply, "rebase-apply/autostash");
}

#[test]
fn merge_root_autostash_is_conservatively_bound_before_commit_panel_staging() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, alternate) = autostash_fixture(Mode::Merge);
    let (fixture, backend) = saved_fixture(fixture);
    let path = fixture.path();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    backend
        .plan_conflict_continue_route(
            &approved.session,
            &buffer,
            "main",
            &approved.observation.revision,
        )
        .unwrap();
    git(path, &["update-ref", "MERGE_AUTOSTASH", &alternate]);
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_ne!(current.observation.revision, approved.observation.revision);
    assert_stale_approval(path, &backend, &approved, &buffer);
    backend
        .plan_conflict_continue_route(
            &current.session,
            &buffer,
            "main",
            &current.observation.revision,
        )
        .unwrap();
    backend
        .stage_conflict_resolution(&current.session, &buffer, &current.observation.revision)
        .unwrap();
    // Kagi's Merge path opens Commit Panel, not `git merge --continue`.
    // This is staging proof, not a claim that Merge restores an autostash.
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Merge
    );
    assert_eq!(
        git_output(path, &["rev-parse", "MERGE_AUTOSTASH"]),
        alternate
    );
}

#[test]
fn symbolic_merge_root_autostash_is_not_authorizable() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, alternate) = autostash_fixture(Mode::Merge);
    let (fixture, backend) = saved_fixture(fixture);
    let path = fixture.path();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    git(
        path,
        &["update-ref", "refs/heads/selected-autostash", &alternate],
    );
    git(
        path,
        &[
            "symbolic-ref",
            "MERGE_AUTOSTASH",
            "refs/heads/selected-autostash",
        ],
    );
    assert!(backend.conflict_snapshot().is_err());
    assert_stale_approval(path, &backend, &approved, &buffer);
}
