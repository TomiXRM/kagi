//! `plan_push_tag` / `execute_push_tag` — publishing a local tag (ADR-0140).

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::ops::{execute_push_tag, plan_push_tag};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Repos {
    _tmp: TempDir,
    local: PathBuf,
    remote: PathBuf,
}

/// A local repo with an `origin` bare remote and one commit pushed.
fn setup() -> Repos {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "t"]);
    git(&local, &["config", "user.email", "t@e"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::write(local.join("a.txt"), "a\n").unwrap();
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);
    Repos {
        _tmp: tmp,
        local,
        remote,
    }
}

fn remote_tag_sha(remote: &Path, name: &str) -> String {
    git(remote, &["rev-parse", &format!("refs/tags/{}", name)])
}

#[test]
fn pushes_a_local_tag_to_the_remote() {
    let r = setup();
    git(&r.local, &["tag", "v1.0.0"]);
    let local_sha = git(&r.local, &["rev-parse", "v1.0.0"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_tag(&repo, "v1.0.0").unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(
        !plan.warnings.is_empty(),
        "publishing to a remote must be stated, not silent"
    );
    assert!(!plan.destructive);

    execute_push_tag(&r.local, "origin", "v1.0.0").expect("push");
    assert_eq!(remote_tag_sha(&r.remote, "v1.0.0"), local_sha);
}

#[test]
fn a_missing_tag_is_blocked() {
    let r = setup();
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_tag(&repo, "nope").unwrap();
    assert!(!plan.blockers.is_empty());
    assert!(plan.warnings.is_empty(), "a blocked plan promises nothing");
}

#[test]
fn a_repo_with_no_remote_is_blocked() {
    let td = TempDir::new().unwrap();
    let p = td.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "a\n").unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-qm", "base"]);
    git(p, &["tag", "v1.0.0"]);

    let repo = Repository::open(p).unwrap();
    let plan = plan_push_tag(&repo, "v1.0.0").unwrap();
    assert!(!plan.blockers.is_empty(), "nowhere to push to");
}

/// The safety property: kagi never force-pushes a tag, so a tag that already
/// exists on the remote at a different commit makes the remote refuse rather
/// than silently moving what everyone else has already fetched.
#[test]
fn a_moved_tag_is_refused_by_the_remote_not_forced() {
    let r = setup();
    git(&r.local, &["tag", "v1.0.0"]);
    let first_sha = git(&r.local, &["rev-parse", "v1.0.0"]);
    execute_push_tag(&r.local, "origin", "v1.0.0").expect("first push");

    // Move the tag locally onto a new commit.
    std::fs::write(r.local.join("b.txt"), "b\n").unwrap();
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "second"]);
    git(&r.local, &["tag", "-f", "v1.0.0"]);
    assert_ne!(git(&r.local, &["rev-parse", "v1.0.0"]), first_sha);

    let result = execute_push_tag(&r.local, "origin", "v1.0.0");
    assert!(result.is_err(), "the remote must refuse a moved tag");
    assert_eq!(
        remote_tag_sha(&r.remote, "v1.0.0"),
        first_sha,
        "the published tag must not move"
    );
}
