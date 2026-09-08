use super::{pull_blocking, PullBlockingResult};

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
    let promised = backend
        .working_tree_status()
        .expect("status")
        .digest();
    let result = pull_blocking(&repos.local, &plan, true, Some(promised));

    assert!(
        matches!(result, PullBlockingResult::Success { .. }),
        "auto-stash pull should complete"
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
    let promised = backend
        .working_tree_status()
        .expect("status")
        .digest();
    let result = pull_blocking(&repos.local, &plan, true, Some(promised));

    match result {
        PullBlockingResult::Partial { error, .. } => {
            assert!(
                error.contains("base.txt"),
                "conflict names its path: {error}"
            );
        }
        _ => panic!("conflicted restoration must be Partial"),
    }
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
