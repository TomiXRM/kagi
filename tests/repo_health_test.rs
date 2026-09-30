//! #358 / ADR-0205: Analyze's repository-health fixes.
//!
//! Detection is a read; each fix is a planned `Operation` that changes only
//! its own cache or config key, and only when `Backend::run` executes it.
//! Each test runs in its own child process with a fresh `KAGI_LOG_DIR` and a
//! fresh `HOME`, so the developer's global git config (libgit2 reads it) can
//! neither suggest nor block anything here.

#[path = "support/git_fixture.rs"]
mod git_fixture;
#[path = "support/isolated.rs"]
mod test_support;

use git_fixture::{commit_all, git, git_output, init_repo, write_file};
use kagi_domain::plan_note::{MaintenanceNote, PlanNote};
use kagi_domain::repo_health::HealthFinding;
use kagi_git::backend::ExecutionPolicy;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_git::{Backend, Operation};
use std::path::Path;

/// A fresh HOME / XDG_CONFIG_HOME for this child, before libgit2 first
/// resolves its global config search path.
fn isolate_global_config() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path());
    home
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    write_file(dir.path(), "a.txt", "a\n");
    commit_all(dir.path(), "first");
    dir
}

fn open(path: &Path) -> Backend {
    Backend::open_with_policy(path, ExecutionPolicy::human(false)).unwrap()
}

fn commit_graph_exists(path: &Path) -> bool {
    let info = path.join(".git/objects/info");
    info.join("commit-graph").exists() || info.join("commit-graphs/commit-graph-chain").exists()
}

/// Every local config entry, sorted: where git places a key is its business.
fn local_config(path: &Path) -> Vec<String> {
    let mut entries: Vec<String> = git_output(path, &["config", "--local", "--list"])
        .lines()
        .map(str::to_string)
        .collect();
    entries.sort();
    entries
}

#[test]
fn a_missing_commit_graph_is_suggested_and_written_only_on_run() {
    if !test_support::run_isolated() {
        return;
    }
    let _home = isolate_global_config();
    let dir = fixture();
    let head = git_output(dir.path(), &["rev-parse", "HEAD"]);
    let mut backend = open(dir.path());
    assert!(backend
        .repo_health()
        .unwrap()
        .contains(&HealthFinding::CommitGraphMissing));

    let op = Operation::WriteCommitGraph;
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert_eq!(
        plan.equivalent_command.as_deref(),
        Some("git commit-graph write --reachable")
    );
    let recovery = plan.recovery.as_ref().expect("a way back");
    assert!(
        recovery.commands.iter().any(|c| c.contains("commit-graph")),
        "the recovery names the files to delete: {:?}",
        recovery.commands
    );
    assert!(
        !commit_graph_exists(dir.path()),
        "detecting and planning write nothing"
    );

    backend.run(&op, &plan).unwrap();
    assert!(commit_graph_exists(dir.path()));
    assert_eq!(git_output(dir.path(), &["rev-parse", "HEAD"]), head);
    assert!(!open(dir.path())
        .repo_health()
        .unwrap()
        .iter()
        .any(|f| matches!(
            f,
            HealthFinding::CommitGraphMissing | HealthFinding::CommitGraphStale
        )));
    let recorded: Vec<_> = read_oplog_tail_for_repo(dir.path(), 10)
        .into_iter()
        .filter(|entry| entry.op == "write-commit-graph")
        .collect();
    assert!(
        matches!(recorded.as_slice(), [entry] if matches!(entry.outcome, OpOutcome::Success { .. })),
        "one durable success: {recorded:?}"
    );
}

#[test]
fn a_repository_without_commits_cannot_write_a_commit_graph() {
    if !test_support::run_isolated() {
        return;
    }
    let _home = isolate_global_config();
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path(), "main");
    let backend = open(dir.path());
    assert!(backend.repo_health().unwrap().iter().all(|f| !matches!(
        f,
        HealthFinding::CommitGraphMissing | HealthFinding::CommitGraphStale
    )));
    let plan = backend.plan(&Operation::WriteCommitGraph).unwrap();
    assert_eq!(
        plan.blockers,
        vec![PlanNote::Maintenance(MaintenanceNote::NoCommits)]
    );
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn fsmonitor_is_written_to_local_config_and_nothing_else() {
    if !test_support::run_isolated() {
        return;
    }
    let _home = isolate_global_config();
    let dir = fixture();
    let before = local_config(dir.path());
    let mut backend = open(dir.path());
    assert!(backend
        .repo_health()
        .unwrap()
        .contains(&HealthFinding::FsmonitorUnset));

    let op = Operation::EnableFsmonitor;
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert_eq!(
        plan.equivalent_command.as_deref(),
        Some("git config core.fsmonitor true")
    );
    assert_eq!(
        plan.recovery.as_ref().unwrap().commands,
        vec!["git config --unset core.fsmonitor".to_string()]
    );
    assert_eq!(local_config(dir.path()), before, "planning writes nothing");

    backend.run(&op, &plan).unwrap();
    let after = local_config(dir.path());
    let mut expected = before.clone();
    expected.push("core.fsmonitor=true".to_string());
    expected.sort();
    assert_eq!(after, expected, "only core.fsmonitor was added");
    assert!(!open(dir.path())
        .repo_health()
        .unwrap()
        .contains(&HealthFinding::FsmonitorUnset));
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn an_existing_fsmonitor_setting_is_left_alone() {
    if !test_support::run_isolated() {
        return;
    }
    let _home = isolate_global_config();
    let dir = fixture();
    git(dir.path(), &["config", "core.fsmonitor", "false"]);
    let before = local_config(dir.path());
    let mut backend = open(dir.path());
    assert!(!backend
        .repo_health()
        .unwrap()
        .contains(&HealthFinding::FsmonitorUnset));
    let op = Operation::EnableFsmonitor;
    let plan = backend.plan(&op).unwrap();
    assert_eq!(
        plan.blockers,
        vec![PlanNote::Maintenance(
            MaintenanceNote::FsmonitorAlreadySet {
                value: "false".into()
            }
        )]
    );
    assert!(
        backend.run(&op, &plan).is_err(),
        "a blocked plan never runs"
    );
    assert_eq!(local_config(dir.path()), before);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn fsmonitor_is_not_offered_where_git_has_no_builtin_monitor() {
    if !test_support::run_isolated() {
        return;
    }
    let _home = isolate_global_config();
    let dir = fixture();
    let backend = open(dir.path());
    assert!(!backend
        .repo_health()
        .unwrap()
        .contains(&HealthFinding::FsmonitorUnset));
    let plan = backend.plan(&Operation::EnableFsmonitor).unwrap();
    assert_eq!(
        plan.blockers,
        vec![PlanNote::Maintenance(MaintenanceNote::FsmonitorUnsupported)]
    );
}
