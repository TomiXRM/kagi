use super::pull_blocking;
use crate::app::{PullPresentation, PullReport};

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

struct LogEnv {
    previous: Option<OsString>,
}

impl LogEnv {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", path);
        Self { previous }
    }
}

impl Drop for LogEnv {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

struct Repos {
    _root: TempDir,
    local: PathBuf,
    other: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, content).expect("write fixture");
}

fn setup() -> Repos {
    let root = TempDir::new().expect("fixture root");
    let remote = root.path().join("remote.git");
    let local = root.path().join("local");
    let other = root.path().join("other");
    git(
        root.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    std::fs::create_dir(&local).expect("local dir");
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);
    Repos {
        _root: root,
        local,
        other,
    }
}

fn push_remote_change(repos: &Repos, content: &str) {
    write_file(&repos.other, "base.txt", content);
    git(&repos.other, &["add", "base.txt"]);
    git(&repos.other, &["commit", "-qm", "remote change"]);
    git(&repos.other, &["push", "-q", "origin", "main"]);
}

#[test]
fn auto_stash_pull_restores_tracked_and_untracked_changes() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    push_remote_change(&repos, "upstream\nbase\n");
    write_file(&repos.local, "base.txt", "base\nlocal\n");
    write_file(&repos.local, "scratch.txt", "untracked\n");
    git(&repos.local, &["add", "base.txt"]);

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    // #625: the confirmation's promise — the dirty set it was shown for. The UI
    // captures it when the modal opens; here the plan was just built, so it is
    // the tree as it stands.
    let promised = backend.working_tree_status().expect("status").digest();
    let result = pull_blocking(&repos.local, &plan, true, Some(promised)).expect("repo opens");

    assert!(
        matches!(
            result.terminal.presentation,
            PullPresentation::Success { .. }
        ),
        "auto-stash pull should complete"
    );
    // ADR-0196 決定 5: the workflow carries the receipt of every child it ran,
    // in execution order, and settles on the one that decided it.
    assert_eq!(
        ops(&result),
        vec!["stash-push", "pull", "stash-pop"],
        "a dirty pull runs and records all three children"
    );
    assert_eq!(decisive_op(&result), "stash-pop");
    assert!(
        result.terminal.stash.is_none(),
        "a restored stash leaves no recovery context"
    );
    assert_eq!(
        std::fs::read_to_string(repos.local.join("base.txt")).unwrap(),
        "upstream\nbase\nlocal\n"
    );
    assert_eq!(
        std::fs::read_to_string(repos.local.join("scratch.txt")).unwrap(),
        "untracked\n"
    );
    let mut backend = kagi_git::Backend::open(&repos.local).expect("backend");
    assert_eq!(
        backend.stash_count().unwrap(),
        0,
        "temporary stash is popped"
    );
}

#[test]
fn auto_stash_pull_keeps_stash_when_restore_conflicts() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    push_remote_change(&repos, "remote replacement\n");
    write_file(&repos.local, "base.txt", "local replacement\n");
    git(&repos.local, &["add", "base.txt"]);

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    // #625: the confirmation's promise — the dirty set it was shown for. The UI
    // captures it when the modal opens; here the plan was just built, so it is
    // the tree as it stands.
    let promised = backend.working_tree_status().expect("status").digest();
    let result = pull_blocking(&repos.local, &plan, true, Some(promised)).expect("repo opens");

    match &result.terminal.presentation {
        PullPresentation::Partial { error } => {
            assert!(
                error.contains("base.txt"),
                "conflict names its path: {error}"
            );
        }
        other => panic!("conflicted restoration must be Partial: {other:?}"),
    }
    assert_eq!(ops(&result), vec!["stash-push", "pull", "stash-pop"]);
    assert_eq!(
        decisive_op(&result),
        "stash-pop",
        "a conflicted restore is what decided this workflow"
    );
    assert!(
        result.terminal.stash.is_some(),
        "a kept stash must travel as recovery context"
    );
    let mut backend = kagi_git::Backend::open(&repos.local).expect("backend");
    assert_eq!(
        backend.stash_count().unwrap(),
        1,
        "conflicted stash is kept"
    );
    assert!(
        !backend.working_tree_status().unwrap().conflicted.is_empty(),
        "restoration conflict remains visible"
    );
}

/// The oplog `op` of every receipt the workflow produced, in execution order.
fn ops(report: &PullReport) -> Vec<&str> {
    report
        .steps
        .iter()
        .map(|step| step.recording.entry().op.as_str())
        .collect()
}

fn decisive_op(report: &PullReport) -> &str {
    report.decisive().recording.entry().op.as_str()
}

/// ADR-0196 決定 5: a clean pull is one write and records exactly one receipt —
/// no stash child is invented for it.
#[test]
fn clean_pull_records_only_the_pull_step() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    push_remote_change(&repos, "upstream\nbase\n");

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    let result = pull_blocking(&repos.local, &plan, false, None).expect("repo opens");

    assert!(
        matches!(
            result.terminal.presentation,
            PullPresentation::Success { .. }
        ),
        "clean pull should complete: {:?}",
        result.terminal.presentation
    );
    assert_eq!(ops(&result), vec!["pull"]);
    assert_eq!(decisive_op(&result), "pull");
}

/// ADR-0196 決定 5: the *last* receipt is not the terminal one. A pull that
/// failed and whose auto-stash was then restored is a failed pull — settling on
/// the trailing `stash-pop` success would report the whole workflow as done.
#[test]
fn failed_pull_settles_on_the_pull_not_the_restore() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    write_file(&repos.local, "base.txt", "base\nlocal\n");
    write_file(&repos.local, "scratch.txt", "untracked\n");
    git(&repos.local, &["add", "base.txt"]);

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    let promised = backend.working_tree_status().expect("status").digest();
    // The stash still succeeds (it is local); the pull's fetch cannot.
    std::fs::remove_dir_all(repos._root.path().join("remote.git")).expect("drop the remote");

    let result = pull_blocking(&repos.local, &plan, true, Some(promised)).expect("repo opens");

    assert_eq!(
        ops(&result),
        vec!["stash-push", "pull", "stash-pop"],
        "the restore still runs after a stopped pull"
    );
    assert_eq!(
        decisive_op(&result),
        "pull",
        "the workflow settles on the pull, not on the pop that followed it"
    );
    assert!(
        matches!(
            result.decisive().recording.entry().outcome,
            kagi_git::oplog::OpOutcome::Failed { .. }
        ),
        "the decisive receipt is the failure: {:?}",
        result.decisive().recording.entry().outcome
    );
    assert!(
        matches!(
            result.terminal.presentation,
            PullPresentation::Failed { .. }
        ),
        "a restored failure is presented as failed: {:?}",
        result.terminal.presentation
    );
    assert_eq!(
        std::fs::read_to_string(repos.local.join("base.txt")).unwrap(),
        "base\nlocal\n",
        "the work is back in the working tree"
    );
}

/// #625 / ADR-0196: a confirmation whose dirty set moved refuses *before*
/// anything is stashed, and the core — not the UI — records that refusal.
#[test]
fn a_stale_confirmation_refuses_before_stashing() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    push_remote_change(&repos, "upstream\nbase\n");
    write_file(&repos.local, "base.txt", "base\nlocal\n");
    git(&repos.local, &["add", "base.txt"]);

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    let promised = backend.working_tree_status().expect("status").digest();
    // The promise goes stale: an editor writes a path the modal never named.
    write_file(&repos.local, "second.txt", "written after confirming\n");

    let result = pull_blocking(&repos.local, &plan, true, Some(promised)).expect("repo opens");

    assert_eq!(
        ops(&result),
        vec!["pull"],
        "no child may start once the confirmation is stale"
    );
    assert!(
        matches!(
            result.decisive().recording.entry().outcome,
            kagi_git::oplog::OpOutcome::Refused { .. }
        ),
        "the refusal is recorded by the core: {:?}",
        result.decisive().recording.entry().outcome
    );
    let mut backend = kagi_git::Backend::open(&repos.local).expect("backend");
    assert_eq!(backend.stash_count().unwrap(), 0, "nothing was stashed");
    assert_eq!(
        std::fs::read_to_string(repos.local.join("second.txt")).unwrap(),
        "written after confirming\n",
        "the work done after confirming is untouched"
    );
}

/// A `git` child that leaves a descendant holding the pipes: the wait resolves
/// but the capture is not proven complete, which is `run_git`'s
/// [`GitError::TerminationUnknown`]. This is the injection the real repository
/// path accepts — `remote.<name>.uploadpack` is what `git fetch` runs for a
/// local remote — and it is not neutralised by the CLI hardening.
fn leaky_upload_pack(repos: &Repos) -> String {
    let path = repos._root.path().join("upload-pack.sh");
    std::fs::write(&path, "#!/bin/sh\nsleep 5 &\nexec git upload-pack \"$@\"\n")
        .expect("write helper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_str().expect("utf-8 path").to_string()
}

/// #702 review P1 — the forbidden step, on the **production** path.
///
/// `execute_pull` used to flatten `run_git`'s `TerminationUnknown` into
/// `GitError::Other`, so a fetch that may still be running read as a stopped
/// writer and the workflow went straight on to pop the auto-stash. The mapper
/// keeps the type now, and the workflow stops with the user's work still in the
/// stash. Nothing is simulated here: the pull runs against a real repository.
#[test]
fn an_unconfirmed_fetch_stops_the_workflow_before_the_restore() {
    let _lock = crate::ui::ENV_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let repos = setup();
    let log = TempDir::new().expect("log dir");
    let _env = LogEnv::set(log.path());
    push_remote_change(&repos, "upstream\nbase\n");
    write_file(&repos.local, "base.txt", "base\nlocal\n");
    git(&repos.local, &["add", "base.txt"]);
    let helper = leaky_upload_pack(&repos);
    git(
        &repos.local,
        &["config", "remote.origin.uploadpack", &helper],
    );

    let backend = kagi_git::Backend::open(&repos.local).expect("backend");
    let plan = backend.plan_pull().expect("pull plan");
    let promised = backend.working_tree_status().expect("status").digest();
    let result = pull_blocking(&repos.local, &plan, true, Some(promised)).expect("repo opens");

    assert!(
        matches!(
            result.decisive().result,
            Err(kagi_git::GitError::TerminationUnknown(_))
        ),
        "the fetch's unproven termination must reach the workflow with its type \
         intact, not flattened into a plain failure: {:?}",
        result.decisive().result
    );
    assert_eq!(
        ops(&result),
        vec!["stash-push", "pull"],
        "no pop may follow a fetch that is not proven stopped"
    );
    let mut backend = kagi_git::Backend::open(&repos.local).expect("backend");
    assert_eq!(
        backend.stash_count().unwrap(),
        1,
        "the user's work stays in the stash, untouched"
    );
    assert!(
        result.terminal.stash.is_some(),
        "and travels on as the recovery context"
    );
}
