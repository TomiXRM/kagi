//! Security tests for `run_git` hardening — issues #290 (git-config /
//! environment hardening) and #291 (argument injection via repo-supplied
//! remote/ref names).
//!
//! Every test that asserts "the attack did not fire" first runs the **same**
//! attack through bare `git` (the positive control) and asserts it *does* fire.
//! Without that, removing the guard would leave a test that still passes.
//!
//! No network access: remotes are local paths / bare repos in a `TempDir`.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::cli::{check_operand, run_git};
use kagi_git::ops::{fetch_remote, fetch_remote_branch};

fn git(dir: &Path, args: &[&str]) {
    let status = raw_git(dir, args);
    assert!(status, "git {} failed", args.join(" "));
}

/// Bare `git`, no hardening — the positive control for every attack below.
fn raw_git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("HOME", dir)
        .status()
        .expect("git failed to start")
        .success()
}

/// A `#!/bin/sh` script that touches `marker` and then fails.
fn marker_script(path: &Path, marker: &Path) -> String {
    std::fs::write(
        path,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_str().unwrap().to_string()
}

/// A single-commit repo at `tmp/repo`.
fn fixture(tmp: &TempDir) -> PathBuf {
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "."]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "base"]);
    repo
}

// ────────────────────────────────────────────────────────────
// #290 — core.fsmonitor
// ────────────────────────────────────────────────────────────

/// A repo-local `core.fsmonitor` is executed by `git status`. `run_git` must
/// neutralise it with `-c core.fsmonitor=`.
///
/// Mutation check: drop `"-c", "core.fsmonitor="` from `HARDENING_ARGS` and the
/// final assertion fires ("fsmonitor ran under run_git").
#[test]
fn fsmonitor_does_not_run_under_run_git() {
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    let marker = tmp.path().join("PWNED_FSM");
    let script = marker_script(&tmp.path().join("fsm.sh"), &marker);
    git(&repo, &["config", "core.fsmonitor", &script]);

    // Positive control: bare `git status` runs it.
    let _ = raw_git(&repo, &["status", "--porcelain=v1"]);
    assert!(
        marker.exists(),
        "fixture is inert: bare git status did not run core.fsmonitor"
    );
    std::fs::remove_file(&marker).unwrap();

    // The real thing: file_history's status args, through run_git.
    let out = run_git(
        &repo,
        &["-c", "core.quotePath=false", "status", "--porcelain=v1"],
    )
    .expect("run_git status");
    assert_eq!(out.status, 0, "status failed: {}", out.stderr);
    assert!(!marker.exists(), "fsmonitor ran under run_git");
}

// ────────────────────────────────────────────────────────────
// #290 — core.sshCommand
// ────────────────────────────────────────────────────────────

/// A repo-local `core.sshCommand` is executed by `git fetch` over an ssh
/// remote. The fetch itself fails (the host does not exist) — the assertion is
/// on the marker.
///
/// Mutation check: make `repo_local_overrides` return `Vec::new()` and the
/// final assertion fires ("sshCommand ran under run_git").
#[test]
fn ssh_command_does_not_run_under_run_git() {
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    let marker = tmp.path().join("PWNED_SSH");
    let script = marker_script(&tmp.path().join("ssh.sh"), &marker);
    git(&repo, &["config", "core.sshCommand", &script]);
    git(
        &repo,
        &["remote", "add", "evil", "git@kagi.invalid:x/y.git"],
    );

    // Positive control: bare `git fetch` runs it.
    let _ = raw_git(&repo, &["fetch", "evil"]);
    assert!(
        marker.exists(),
        "fixture is inert: bare git fetch did not run core.sshCommand"
    );
    std::fs::remove_file(&marker).unwrap();

    let _ = run_git(&repo, &["fetch", "--", "evil"]).expect("run_git fetch");
    assert!(!marker.exists(), "sshCommand ran under run_git");

    // …and the override really is the mechanism: git reports the neutered value.
    let seen = run_git(&repo, &["config", "--get", "core.sshCommand"]).unwrap();
    assert_eq!(seen.stdout.trim(), "ssh");
}

// ────────────────────────────────────────────────────────────
// #291 — remote name that is really a flag
// ────────────────────────────────────────────────────────────

/// `.git/config` may name a remote `--upload-pack=<cmd>`; `git fetch <name>`
/// then runs `<cmd>`. Both the leading-dash reject and the `--` separator must
/// stop it, and the sole-remote auto-fetch path must reach the same verdict.
///
/// Mutation check A: delete the `check_operand` calls in `ops/fetch.rs` and the
/// `is_err()` assertions fire (the fetch returns `Ok`).
/// Mutation check B: additionally drop the `"--"` from both `fetch` arg lists
/// and the marker assertions fire (the injected command runs).
#[test]
fn dash_remote_name_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "--bare", "-b", "main", "."]);

    let marker = tmp.path().join("PWNED_CFG");
    let evil = format!("--upload-pack=touch '{}';git-upload-pack", marker.display());

    // The only remote in the repo, so `resolve_fetch_remote` returns it.
    let mut cfg = std::fs::OpenOptions::new()
        .append(true)
        .open(repo.join(".git/config"))
        .unwrap();
    use std::io::Write;
    writeln!(
        cfg,
        "[remote \"{}\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/evil/*",
        evil,
        src.display()
    )
    .unwrap();
    drop(cfg);

    // Positive control: bare `git fetch --prune <name>` executes it.
    let _ = raw_git(&repo, &["fetch", "--prune", &evil]);
    assert!(
        marker.exists(),
        "fixture is inert: bare git fetch did not honour the injected --upload-pack"
    );
    std::fs::remove_file(&marker).unwrap();

    let g = Repository::open(&repo).unwrap();

    // Auto-fetch path (`fetch_remote` → `resolve_fetch_remote` → sole remote).
    let err = fetch_remote(&g, &repo);
    assert!(err.is_err(), "fetch_remote accepted a flag-shaped remote");
    assert!(!marker.exists(), "fetch_remote executed the injection");

    // Branch-menu path.
    let err = fetch_remote_branch(&g, &repo, &evil, "main");
    assert!(
        err.is_err(),
        "fetch_remote_branch accepted a flag-shaped remote"
    );
    assert!(
        !marker.exists(),
        "fetch_remote_branch executed the injection"
    );

    // And the raw CLI layer is safe even without the validator, thanks to `--`.
    let _ = run_git(&repo, &["fetch", "--prune", "--", &evil]);
    assert!(!marker.exists(), "`--` did not neutralise the remote name");
}

/// The shared validator itself (`ops/branch.rs` and `ops/tag.rs` use the same
/// `is_flag_like` predicate for names kagi creates).
#[test]
fn check_operand_rejects_leading_dash_only() {
    assert!(check_operand("remote", "origin").is_ok());
    assert!(check_operand("branch", "feature/x").is_ok());
    assert!(check_operand("remote", "-x").is_err());
    assert!(check_operand("remote", "--upload-pack=evil").is_err());
}

// ────────────────────────────────────────────────────────────
// #290 — regression: hardening does not break the happy path
// ────────────────────────────────────────────────────────────

/// The hardening `-c` flags actually reach the child process, and a plain
/// local-path remote still fetches and pushes with them in place.
#[test]
fn hardening_is_applied_and_local_remotes_still_work() {
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    let bare = tmp.path().join("remote.git");
    git(
        tmp.path(),
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);

    for (key, want) in [
        ("core.fsmonitor", ""),
        ("core.hooksPath", "/dev/null"),
        ("core.askPass", ""),
        ("protocol.allow", "user"),
    ] {
        let out = run_git(&repo, &["config", "--get", key]).unwrap();
        assert_eq!(out.stdout.trim(), want, "{} not hardened", key);
    }

    // `credential.helper` is deliberately NOT cleared when the repo does not
    // set one — clearing it unconditionally kills the user's system/global
    // helper (osxkeychain), which is the whole reason this module shells out.
    let raw = Command::new("git")
        .args(["config", "--get", "credential.helper"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let hardened = run_git(&repo, &["config", "--get", "credential.helper"]).unwrap();
    assert_eq!(
        hardened.stdout.trim(),
        String::from_utf8_lossy(&raw.stdout).trim(),
        "credential.helper was altered without the repo setting one"
    );

    let push = run_git(&repo, &["push", "-u", "--", "origin", "main"]).unwrap();
    assert_eq!(push.status, 0, "push failed: {}", push.stderr);

    let g = Repository::open(&repo).unwrap();
    fetch_remote(&g, &repo).expect("fetch against a local-path remote");
    fetch_remote_branch(&g, &repo, "origin", "main").expect("fetch remote branch");
}
