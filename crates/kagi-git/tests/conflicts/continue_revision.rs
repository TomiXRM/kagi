//! #1721: an older Result cannot authorize writes after another handle advances.
use super::*;
use kagi_git::Backend;

#[derive(Debug, PartialEq, Eq)]
struct PhysicalState {
    head: String,
    rebase_head: Option<String>,
    index: Vec<u8>,
    stages: String,
    worktree: String,
    committed: String,
    operation: git2::RepositoryState,
}

fn physical_state(path: &Path) -> PhysicalState {
    PhysicalState {
        head: git_output(path, &["rev-parse", "HEAD"]),
        rebase_head: std::fs::read_to_string(path.join(".git/REBASE_HEAD")).ok(),
        index: std::fs::read(path.join(".git/index")).unwrap(),
        stages: git_output(path, &["ls-files", "--stage"]),
        worktree: std::fs::read_to_string(path.join("file.txt")).unwrap(),
        committed: git_output(path, &["show", "HEAD:file.txt"]),
        operation: Repository::open(path).unwrap().state(),
    }
}

#[test]
fn public_continue_refuses_old_result_after_external_same_path_advance() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_two_conflicting_steps_repo();
    let path = fixture.path();
    let policy = || kagi_git::backend::ExecutionPolicy::human(false);
    let old_backend = Backend::open_with_policy(path, policy()).unwrap();
    let repo = Repository::open(path).unwrap();
    let old_session = detect_conflict_session(&repo).unwrap();
    let mut old_buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    old_buffer
        .set_manual_text(Path::new("file.txt"), "RESOLVED ONE\n")
        .unwrap();
    let first = physical_state(path);
    assert!(old_backend
        .plan_conflict_continue_route(&old_session, &old_buffer, "side")
        .is_ok());
    Backend::open_with_policy(path, policy())
        .unwrap()
        .execute_conflict_continue(&old_session, &old_buffer)
        .unwrap();
    let before = physical_state(path);
    assert_ne!(before.head, first.head);
    assert_ne!(before.rebase_head, first.rebase_head);
    assert_eq!(before.committed, "RESOLVED ONE");
    assert!(!git_output(path, &["ls-files", "-u"]).is_empty());
    assert!(before.worktree.contains("SIDE TWO"));

    let plan = old_backend.plan_conflict_continue_route(&old_session, &old_buffer, "side");
    let execution = old_backend.execute_conflict_continue(&old_session, &old_buffer);
    let after = physical_state(path);
    assert!(
        plan.is_err() && execution.is_err() && before == after,
        "old Result must be refused without mutation after external advance; \
         plan_admitted={} execution_admitted={} execution_error={:?}\nbefore={before:#?}\nafter={after:#?}",
        plan.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
}
