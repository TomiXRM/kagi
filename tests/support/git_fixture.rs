//! Hermetic `git` CLI for building integration-test fixtures (#514).
//!
//! Every fixture command runs with the developer's Git setup shut out, so the
//! repository a test builds is the same on every machine:
//!
//! - every inherited `GIT_*` variable is dropped: `GIT_DIR`/`GIT_WORK_TREE`
//!   exported by a hook cannot redirect a fixture into the real repository, and
//!   `GIT_TEMPLATE_DIR`, `GIT_CONFIG_PARAMETERS`/`GIT_CONFIG_COUNT`,
//!   `GIT_DEFAULT_HASH` and author dates cannot leak in;
//! - global and system config are `/dev/null` (so no `core.hooksPath`,
//!   `init.templateDir`, `init.defaultBranch`, `commit.gpgSign`, filters…);
//! - `HOME` and `XDG_CONFIG_HOME` point at an empty directory under Cargo's
//!   target tmp dir, which also removes the global `git/ignore` and
//!   `git/attributes` files that `GIT_CONFIG_GLOBAL` does not cover;
//! - identity is fixed to [`NAME`] / [`EMAIL`], and prompts are off;
//! - no detached background process is left behind (`maintenance.auto` /
//!   `gc.auto` off, #819), so a fixture is complete the moment the command
//!   returns and can be snapshotted or deleted without racing `git`.
//!
//! Only the spawned `git` process is isolated; the helper never changes the
//! test process's environment. Code under test that runs *in process*
//! (libgit2, or Kagi's own `git` subprocesses) still sees the host config, so
//! [`init_repo`] pins the keys that would change a result (`user.*`,
//! `commit.gpgsign`) repo-locally, where they outrank global config.
//!
//! A test that deliberately exercises inherited configuration opts in
//! explicitly: take [`git_command`] and layer the one variable it is about on
//! top (`.env("GIT_CONFIG_GLOBAL", path)`), or keep its own raw `Command`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

pub const NAME: &str = "Test";
pub const EMAIL: &str = "test@example.com";

/// Empty directory used as `HOME` and `XDG_CONFIG_HOME` for fixture commands.
fn empty_home() -> PathBuf {
    let target_tmp = option_env!("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"));
    let home = target_tmp.join("kagi-git-fixture-home");
    std::fs::create_dir_all(&home).expect("create fixture HOME");
    home
}

/// A hermetic `git` command running in `dir`; the caller adds the arguments.
pub fn git_command(dir: &Path) -> Command {
    let home = empty_home();
    let mut cmd = Command::new("git");
    cmd.current_dir(dir);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", NAME)
        .env("GIT_AUTHOR_EMAIL", EMAIL)
        .env("GIT_COMMITTER_NAME", NAME)
        .env("GIT_COMMITTER_EMAIL", EMAIL)
        // #819: `git commit` (and fetch/merge…) spawns `git maintenance run
        // --auto --detach`, a detached process that briefly holds
        // `.git/objects/maintenance.lock` *after* the fixture command has
        // returned. A test that snapshots or deletes the repository right
        // after building it then races that process (CI macOS caught the lock
        // file in a "before" snapshot). Nothing in a fixture ever needs
        // maintenance, so turn it off at the source; `gc.auto` covers the
        // pre-`maintenance` path of older gits.
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "maintenance.auto")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_CONFIG_KEY_1", "gc.auto")
        .env("GIT_CONFIG_VALUE_1", "0");
    cmd
}

fn run(dir: &Path, args: &[&str]) -> Output {
    git_command(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} failed to start: {e}"))
}

/// Run `git args` in `dir`; panic with stderr unless it succeeds.
pub fn git(dir: &Path, args: &[&str]) {
    git_output(dir, args);
}

/// Run `git args` in `dir` and return its trimmed stdout; panic unless it succeeds.
pub fn git_output(dir: &Path, args: &[&str]) -> String {
    let out = run(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run `git args` in `dir` where failure is an expected outcome (a conflicting
/// merge, a missing ref); returns whether it succeeded.
pub fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    run(dir, args).status.success()
}

pub fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write fixture file");
}

/// `git init` a work tree at `dir` on `branch`, with the identity and signing
/// settings pinned repo-locally for in-process code under test.
pub fn init_repo(dir: &Path, branch: &str) {
    git(dir, &["init", "-q", "-b", branch, "."]);
    git(dir, &["config", "user.name", NAME]);
    git(dir, &["config", "user.email", EMAIL]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

/// Stage everything and commit it with `message`.
pub fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", message]);
}

/// A work tree at `local` whose `origin` is the bare repository `remote`.
pub struct RemoteFixture {
    _tmp: TempDir,
    pub local: PathBuf,
    pub remote: PathBuf,
}

/// A work tree on `branch` with one commit (`a.txt`) pushed to a bare `origin`,
/// upstream set.
pub fn repo_with_bare_origin(branch: &str) -> RemoteFixture {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    let remote_arg = remote.to_str().expect("utf-8 temp path");
    git(
        tmp.path(),
        &["init", "-q", "--bare", "-b", branch, remote_arg],
    );
    std::fs::create_dir(&local).expect("create local");
    init_repo(&local, branch);
    git(&local, &["remote", "add", "origin", remote_arg]);
    write_file(&local, "a.txt", "a\n");
    commit_all(&local, "base");
    git(&local, &["push", "-q", "-u", "origin", branch]);
    RemoteFixture {
        _tmp: tmp,
        local,
        remote,
    }
}
