//! Migrated executor fixtures must record inside their isolated child directory.
#[path = "support/backend_ops.rs"]
mod backend_ops;
#[path = "support/isolated.rs"]
mod test_support;

#[test]
fn migrated_fixture_records_in_child_log_dir_and_preserves_home_oplog() {
    if !test_support::run_isolated() {
        return;
    }
    let log_dir = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    let fake_home = tempfile::tempdir().unwrap();
    let home_log = fake_home.path().join(".kagi/operations.jsonl");
    std::fs::create_dir_all(home_log.parent().unwrap()).unwrap();
    std::fs::write(&home_log, b"existing user oplog sentinel\n").unwrap();
    // This child has one test; changing its fake HOME never affects the parent.
    std::env::set_var("HOME", fake_home.path());
    let fixture = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(fixture.path()).unwrap();
    let mut index = repo.index().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    let head = repo
        .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .unwrap();
    let id = kagi_git::CommitId(head.to_string());
    backend_ops::execute_create_branch(&repo, "isolated-branch", &id).unwrap();
    assert_eq!(
        repo.refname_to_id("refs/heads/isolated-branch").unwrap(),
        head
    );
    let recorded_path = log_dir.join("operations.jsonl");
    let bytes = std::fs::read_to_string(&recorded_path).expect("record in child log directory");
    assert!(bytes.contains("create-branch"));
    let entries = kagi_git::read_oplog_tail(10);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].op, "create-branch");
    assert!(matches!(
        entries[0].outcome,
        kagi_git::OpOutcome::Success { .. }
    ));
    assert_ne!(recorded_path, home_log);
    assert_eq!(
        std::fs::read(home_log).unwrap(),
        b"existing user oplog sentinel\n"
    );
}
