//! Security tests for `run_git` hardening — issues #290 (git-config /
//! environment hardening) and #291 (argument injection via repo-supplied
//! remote/ref names).
//!
//! Every test that asserts "the attack did not fire" first runs the **same**
//! attack through bare `git` (the positive control) and asserts it *does* fire.
//! Without that, removing the guard would leave a test that still passes.
//!
//! No network access: remotes are local paths / bare repos in a `TempDir`.

#[path = "support/backend_ops.rs"]
mod backend_ops;
use backend_ops::{execute_stash_push, fetch_remote, fetch_remote_branch};
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::cli::{check_operand, run_git};

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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
// #647 — dynamic merge drivers
// ────────────────────────────────────────────────────────────

/// A repo-local `merge.<name>.driver` is selected by `.gitattributes` and runs
/// arbitrary code during `merge-tree --write-tree`. `run_git` must scan the
/// untrusted config level and disable every discovered driver name.
///
/// Mutation check: make `merge_driver_name` always return `None`; the final
/// assertion fires because the marker script runs under `run_git`.
#[test]
fn merge_driver_does_not_run_under_run_git() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    std::fs::write(repo.join(".gitattributes"), "f.txt merge=evil\n").unwrap();
    std::fs::write(repo.join("f.txt"), "base\n").unwrap();
    git(&repo, &["add", ".gitattributes", "f.txt"]);
    git(&repo, &["commit", "-qm", "add merge driver fixture"]);

    git(&repo, &["checkout", "-qb", "topic"]);
    std::fs::write(repo.join("f.txt"), "topic\n").unwrap();
    git(&repo, &["commit", "-am", "topic"]);
    git(&repo, &["checkout", "main"]);
    std::fs::write(repo.join("f.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-am", "main"]);

    let marker = tmp.path().join("PWNED_MERGE_DRIVER");
    let script = marker_script(&tmp.path().join("merge-driver.sh"), &marker);
    git(&repo, &["config", "merge.evil.driver", &script]);

    // Positive control: the repository-selected driver runs through bare Git.
    let raw = Command::new("git")
        .args(["merge-tree", "--write-tree", "topic", "main"])
        .current_dir(&repo)
        .output()
        .expect("bare git merge-tree");
    assert_eq!(raw.status.code(), Some(1), "bare merge-tree must conflict");
    assert!(
        marker.exists(),
        "fixture is inert: bare git merge-tree did not run merge.evil.driver"
    );
    std::fs::remove_file(&marker).unwrap();

    let out = run_git(&repo, &["merge-tree", "--write-tree", "topic", "main"])
        .expect("run_git merge-tree");
    assert_eq!(out.status, 1, "merge-tree must still report the conflict");
    assert!(!marker.exists(), "merge.evil.driver ran under run_git");

    // The empty `-c merge.evil.driver=` override, not an incidental Git
    // behavior difference, is the mechanism preventing execution.
    let seen = run_git(&repo, &["config", "--get", "merge.evil.driver"]).unwrap();
    assert_eq!(seen.stdout.trim(), "");
}

/// Config inspection is a hard precondition: an unreadable local config must
/// return an error before `run_git` can start a child process.
#[test]
fn malformed_local_config_refuses_run_git() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    std::fs::write(repo.join(".git/config"), "[merge\n").unwrap();

    assert!(
        run_git(&repo, &["status", "--porcelain=v1"]).is_err(),
        "run_git must not execute after repository config inspection fails"
    );
}

// ────────────────────────────────────────────────────────────
// #649 — dynamic filter and diff drivers
// ────────────────────────────────────────────────────────────

/// Rebase can invoke a repository-selected filter process. The shared scanner
/// must disable the process and its `required` flag before starting Git.
///
/// Mutation check: omit either `filter.<name>.process=` or
/// `filter.<name>.required=false`; the hardened rebase fails or creates the
/// marker, and the final assertions catch the mutation.
#[test]
fn rebase_does_not_run_filter_process() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    std::fs::write(
        repo.join(".gitattributes"),
        "filtered.txt filter=RebaseProbe\n",
    )
    .unwrap();
    std::fs::write(repo.join("filtered.txt"), "base\n").unwrap();
    git(&repo, &["add", ".gitattributes", "filtered.txt"]);
    git(&repo, &["commit", "-qm", "filter fixture"]);
    git(&repo, &["checkout", "-qb", "topic"]);
    std::fs::write(repo.join("filtered.txt"), "topic\n").unwrap();
    git(&repo, &["commit", "-am", "topic"]);
    git(&repo, &["checkout", "main"]);
    std::fs::write(repo.join("main.txt"), "main\n").unwrap();
    git(&repo, &["add", "main.txt"]);
    git(&repo, &["commit", "-qm", "main"]);
    git(&repo, &["checkout", "topic"]);

    let hardened = tmp.path().join("hardened-rebase");
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            repo.to_str().unwrap(),
            hardened.to_str().unwrap(),
        ],
    );
    git(&hardened, &["branch", "main", "origin/main"]);
    git(&hardened, &["config", "user.name", "Test"]);
    git(&hardened, &["config", "user.email", "test@example.com"]);
    git(&hardened, &["config", "commit.gpgsign", "false"]);
    let raw_marker = tmp.path().join("PWNED_REBASE_RAW");
    let raw_script = marker_script(&tmp.path().join("rebase-raw.sh"), &raw_marker);
    git(
        &repo,
        &["config", "filter.RebaseProbe.process", &raw_script],
    );
    git(&repo, &["config", "filter.RebaseProbe.required", "true"]);
    git(&repo, &["config", "merge.renormalize", "false"]);

    // Positive control: bare rebase starts the configured filter process.
    let _ = raw_git(&repo, &["rebase", "main"]);
    assert!(
        raw_marker.exists(),
        "fixture is inert: bare git rebase did not run the filter process"
    );

    let marker = tmp.path().join("PWNED_REBASE_HARDENED");
    let script = marker_script(&tmp.path().join("rebase-hardened.sh"), &marker);
    git(
        &hardened,
        &["config", "filter.RebaseProbe.process", &script],
    );
    git(
        &hardened,
        &["config", "filter.RebaseProbe.required", "true"],
    );
    git(&hardened, &["config", "merge.renormalize", "false"]);
    let out = run_git(&hardened, &["rebase", "--", "main"]).expect("run_git rebase");
    assert_eq!(out.status, 0, "hardened rebase failed: {}", out.stderr);
    assert!(!marker.exists(), "filter process ran under run_git rebase");
    let seen = run_git(
        &hardened,
        &["config", "--get", "filter.RebaseProbe.required"],
    )
    .unwrap();
    assert_eq!(seen.stdout.trim(), "false");
    let renormalize = run_git(&hardened, &["config", "--get", "merge.renormalize"]).unwrap();
    assert_eq!(
        renormalize.stdout.trim(),
        "false",
        "run_git must not override merge.renormalize"
    );
}

/// Both executable diff driver forms are selected by `.gitattributes`.
///
/// Mutation check: removing either dynamic suffix from the scanner makes the
/// corresponding marker assertion fail; the config reads pin the empty
/// overrides rather than accepting an incidental lack of diff output.
#[test]
fn diff_command_and_textconv_do_not_run_under_run_git() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    std::fs::write(
        repo.join(".gitattributes"),
        "command.txt diff=CommandProbe\ntextconv.txt diff=TextconvProbe\n",
    )
    .unwrap();
    std::fs::write(repo.join("command.txt"), "base\n").unwrap();
    std::fs::write(repo.join("textconv.txt"), "base\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "diff fixtures"]);
    std::fs::write(repo.join("command.txt"), "changed\n").unwrap();
    std::fs::write(repo.join("textconv.txt"), "changed\n").unwrap();

    for (driver, suffix, path) in [
        ("CommandProbe", "command", "command.txt"),
        ("TextconvProbe", "textconv", "textconv.txt"),
    ] {
        let marker = tmp.path().join(format!("PWNED_DIFF_{suffix}"));
        let script = marker_script(&tmp.path().join(format!("diff-{suffix}.sh")), &marker);
        let key = format!("diff.{driver}.{suffix}");
        git(&repo, &["config", &key, &script]);
        let args = match suffix {
            "command" => vec!["diff", "--ext-diff", "--", path],
            "textconv" => vec!["diff", "--textconv", "--", path],
            _ => unreachable!(),
        };

        // Positive control: bare diff selects this exact configured driver.
        let _ = raw_git(&repo, &args);
        assert!(
            marker.exists(),
            "fixture is inert: bare git diff did not run {key}"
        );
        std::fs::remove_file(&marker).unwrap();

        let _ = run_git(&repo, &args).expect("run_git diff");
        assert!(!marker.exists(), "{key} ran under run_git");
        let seen = run_git(&repo, &["config", "--get", &key]).unwrap();
        assert_eq!(seen.stdout.trim(), "", "{key} was not overridden");
    }
}

/// Clean, smudge, and long-running process filters are independently
/// executable, and `required=true` turns an empty command into a failure.
///
/// Mutation check: each driver has its own positive control and marker, while
/// successful hardened commands plus explicit `required=false` reads catch
/// removal of any one override.
#[test]
fn filter_clean_smudge_and_process_do_not_run_under_run_git() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = fixture(&tmp);
    std::fs::write(
        repo.join(".gitattributes"),
        "clean.txt filter=CleanProbe\nsmudge.txt filter=SmudgeProbe\nprocess.txt filter=ProcessProbe\n",
    )
    .unwrap();
    for path in ["clean.txt", "smudge.txt", "process.txt"] {
        std::fs::write(repo.join(path), "base\n").unwrap();
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "filter fixtures"]);

    for (name, suffix) in [
        ("CleanProbe", "clean"),
        ("SmudgeProbe", "smudge"),
        ("ProcessProbe", "process"),
    ] {
        let marker = tmp.path().join(format!("PWNED_FILTER_{suffix}"));
        let script = marker_script(&tmp.path().join(format!("filter-{suffix}.sh")), &marker);
        git(
            &repo,
            &["config", &format!("filter.{name}.{suffix}"), &script],
        );
        git(
            &repo,
            &["config", &format!("filter.{name}.required"), "true"],
        );

        let path = format!("{suffix}.txt");
        if suffix == "smudge" {
            std::fs::remove_file(repo.join(&path)).unwrap();
        } else {
            std::fs::write(repo.join(&path), "changed\n").unwrap();
        }
        let args = if suffix == "smudge" {
            vec!["checkout-index", "-f", "--", path.as_str()]
        } else {
            vec!["add", "--", path.as_str()]
        };

        // Positive control: bare Git starts this exact filter implementation.
        let _ = raw_git(&repo, &args);
        assert!(
            marker.exists(),
            "fixture is inert: bare git did not run filter.{name}.{suffix}"
        );
        std::fs::remove_file(&marker).unwrap();
        if suffix == "smudge" {
            let _ = std::fs::remove_file(repo.join(&path));
        }

        let out = run_git(&repo, &args).expect("run_git filter operation");
        assert_eq!(
            out.status, 0,
            "hardened filter operation failed: {}",
            out.stderr
        );
        assert!(!marker.exists(), "filter.{name}.{suffix} ran under run_git");
        let required = format!("filter.{name}.required");
        let seen = run_git(&repo, &["config", "--get", &required]).unwrap();
        assert_eq!(seen.stdout.trim(), "false", "{required} was not overridden");
    }
}

/// Stash push retains its filter defense after its private config scan is
/// removed because execution delegates to the hardened `run_git`.
///
/// Mutation check: bypassing `run_git` in stash push or removing the shared
/// clean-filter override creates the marker; a bare `stash create` proves the
/// same repository configuration is executable first.
#[test]
fn stash_push_uses_shared_filter_hardening() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo_dir = fixture(&tmp);
    std::fs::write(repo_dir.join(".gitattributes"), "a.txt filter=StashProbe\n").unwrap();
    git(&repo_dir, &["add", ".gitattributes"]);
    git(&repo_dir, &["commit", "-qm", "stash filter fixture"]);
    let marker = tmp.path().join("PWNED_STASH_FILTER");
    let script = marker_script(&tmp.path().join("stash-filter.sh"), &marker);
    git(&repo_dir, &["config", "filter.StashProbe.clean", &script]);
    git(&repo_dir, &["config", "filter.StashProbe.required", "true"]);
    std::fs::write(repo_dir.join("a.txt"), "stash me\n").unwrap();

    // Positive control: bare stash plumbing runs the configured clean filter.
    let _ = raw_git(&repo_dir, &["stash", "create"]);
    assert!(
        marker.exists(),
        "fixture is inert: bare git stash create did not run the clean filter"
    );
    std::fs::remove_file(&marker).unwrap();

    let mut repo = Repository::open(&repo_dir).unwrap();
    execute_stash_push(&mut repo, Some("hardened"), false).expect("hardened stash push");
    assert!(!marker.exists(), "stash push ran filter.StashProbe.clean");
    let seen = run_git(
        &repo_dir,
        &["config", "--get", "filter.StashProbe.required"],
    )
    .unwrap();
    assert_eq!(seen.stdout.trim(), "false");
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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

#[path = "support/isolated.rs"]
mod test_support;
