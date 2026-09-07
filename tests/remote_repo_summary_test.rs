//! Issue #604: an empty remote repository is not a failed remote read.
//!
//! `repo_summary` is exercised over a local `ssh` stand-in (the same transport
//! trick `remote_oplog_test.rs` uses) against real `git` repositories, so the
//! exit codes and stderr are git's own, not a hand-written fixture.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// PATH is process-global; `run_isolated` gives this test its own process.
struct RestorePath(Option<OsString>);

impl Drop for RestorePath {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// Install an `ssh` stand-in at `bin/ssh`. `body` is the whole script after the
/// shebang, so a test can pick between a working transport and a refusing one.
fn install_ssh(bin: &Path, body: &str) {
    let ssh = bin.join("ssh");
    std::fs::write(&ssh, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// ssh hands its last argument to the remote login shell, and so does this.
const SSH_PASSTHROUGH: &str =
    "for argument do command=$argument; done\nexec /bin/sh -c \"$command\"\n";

/// A host Kagi cannot reach: ssh itself refuses, exactly as it does on a
/// missing key.
const SSH_REFUSED: &str = "echo 'fixture.invalid: Permission denied (publickey).' >&2\nexit 255\n";

/// #604: `git log` on an unborn HEAD exits 128, so `run_checked` turned "this
/// repository has no commits yet" into the same `Err` an unreachable host
/// produces — while the documented contract was `Ok(None)`. Remote Browse then
/// showed a freshly created repository as an error.
#[test]
fn repo_summary_separates_an_empty_repository_from_an_unreachable_host() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let unborn = root.path().join("unborn");
    let populated = root.path().join("populated");
    let bin = root.path().join("bin");
    for dir in [&unborn, &populated, &bin] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&unborn, &["init", "-q", "-b", "master"]);
    git(&populated, &["init", "-q", "-b", "main"]);
    git(&populated, &["config", "core.hooksPath", "/dev/null"]);
    git(&populated, &["config", "commit.gpgSign", "false"]);
    std::fs::write(populated.join("file"), "base\n").unwrap();
    git(&populated, &["add", "file"]);
    git(&populated, &["commit", "-q", "-m", "the first commit"]);

    let restore = RestorePath(std::env::var_os("PATH"));
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(
        &restore.0.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let host = kagi_domain::remote::RemoteHost::parse("fixture.invalid").unwrap();

    install_ssh(&bin, SSH_PASSTHROUGH);

    // A repository with no commits: git fails, the answer is still "no HEAD".
    assert_eq!(
        kagi::remote::repo_summary(&host, unborn.to_str().unwrap()),
        Ok(None),
        "an unborn HEAD must read as an empty repository, not an error"
    );

    // A normal repository still reports its HEAD.
    let summary = kagi::remote::repo_summary(&host, populated.to_str().unwrap())
        .expect("a reachable host must not error")
        .expect("a repository with a commit has a HEAD summary");
    assert_eq!(summary.branch.as_deref(), Some("main"));
    assert_eq!(summary.summary, "the first commit");
    assert!(!summary.head_short.is_empty(), "HEAD short hash present");

    // An unreachable host is still an error — leniency must not swallow it.
    install_ssh(&bin, SSH_REFUSED);
    let err = kagi::remote::repo_summary(&host, populated.to_str().unwrap())
        .expect_err("a refused transport must stay an error");
    assert!(
        err.to_string().contains("Permission denied"),
        "the transport failure must reach the user: {err}"
    );
}

#[path = "support/isolated.rs"]
mod test_support;
