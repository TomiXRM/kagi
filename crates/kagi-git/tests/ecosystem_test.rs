//! Integration tests for the Code Ecosystem / hot-spot data layer (ADR-0119).
//!
//! Builds repos in tempdirs with `git init -b main` and asserts the read-only
//! `Backend::ecosystem` mining + `kagi_domain::hotspot::analyze` ranking on a
//! real `git log --numstat`.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use kagi_domain::activity::Granularity;
use kagi_git::hotspot::{repo_ecosystem, EcosystemRequest};
use kagi_git::{analyze_hotspots, Backend};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write(dir: &Path, name: &str, content: &str) {
    if let Some(parent) = dir.join(name).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(dir.join(name), content).unwrap();
}

/// A commit with a fixed author date so window logic is deterministic.
fn commit_at(dir: &Path, date: &str, msg: &str) {
    git(dir, &["add", "-A"]);
    let env_date = format!("GIT_AUTHOR_DATE={date} GIT_COMMITTER_DATE={date}");
    let status = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .unwrap_or_else(|_| panic!("commit failed ({env_date})"));
    assert!(status.success());
}

/// Commit authored by a specific person (distinct author email per call).
fn commit_as(dir: &Path, date: &str, name: &str, email: &str, msg: &str) {
    git(dir, &["add", "-A"]);
    let status = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("commit failed");
    assert!(status.success());
}

fn init(tmp: &TempDir) -> &Path {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    dir
}

#[test]
fn mines_churn_and_loc_then_ranks_hotspots() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);

    // `hot.rs`: big + changed 3 times. `cold.rs`: big but changed once.
    // `tiny.rs`: changed 3 times but only a few lines.
    write(dir, "hot.rs", &"x\n".repeat(200));
    write(dir, "cold.rs", &"y\n".repeat(200));
    write(dir, "tiny.rs", "a\n");
    commit_at(dir, "2026-01-01T00:00:00", "init");

    write(dir, "hot.rs", &"x\n".repeat(210));
    write(dir, "tiny.rs", "a\nb\n");
    commit_at(dir, "2026-01-02T00:00:00", "edit");

    write(dir, "hot.rs", &"x\n".repeat(220));
    write(dir, "tiny.rs", "a\nb\nc\n");
    commit_at(dir, "2026-01-03T00:00:00", "edit2");

    let raw = repo_ecosystem(&EcosystemRequest {
        repo_dir: dir.to_path_buf(),
        limit: 0,
        ignore_patterns: Vec::new(),
    })
    .expect("ecosystem");

    // Three commits mined; current LOC reflects the working tree.
    assert_eq!(raw.commits.len(), 3);
    assert_eq!(raw.loc.get("hot.rs"), Some(&220));
    assert_eq!(raw.loc.get("tiny.rs"), Some(&3));

    // Rank over all history: hot.rs wins (high churn AND high complexity).
    let now = 1_767_500_000; // a few days after the last commit
    let eco = analyze_hotspots(&raw, now, Granularity::All);
    assert_eq!(eco.files[0].path, "hot.rs");
    assert_eq!(eco.files[0].commits, 3);
    assert!(eco.files[0].risk >= eco.files.last().unwrap().risk);
    // cold.rs changed only once → lower churn than hot/tiny.
    let cold = eco.files.iter().find(|f| f.path == "cold.rs").unwrap();
    assert_eq!(cold.commits, 1);
}

#[test]
fn backend_facade_matches_free_function() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);
    write(dir, "a.txt", "one\ntwo\n");
    commit_at(dir, "2026-01-01T00:00:00", "init");

    let backend = Backend::open(dir).expect("open");
    let via_backend = backend.ecosystem(0, Vec::new()).expect("backend ecosystem");
    let via_free = repo_ecosystem(&EcosystemRequest {
        repo_dir: dir.to_path_buf(),
        limit: 0,
        ignore_patterns: Vec::new(),
    })
    .expect("free ecosystem");
    assert_eq!(via_backend, via_free);
    assert_eq!(via_backend.loc.get("a.txt"), Some(&2));
}

#[test]
fn mine_captures_distinct_authors_for_ownership() {
    use kagi_git::ownership;

    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);

    // shared.rs touched by alice (twice) and bob (once) → 3 commits, 2 authors.
    write(dir, "shared.rs", "1\n");
    commit_as(dir, "2026-01-01T00:00:00", "Alice", "alice@x", "a1");
    write(dir, "shared.rs", "1\n2\n");
    commit_as(dir, "2026-01-02T00:00:00", "Bob", "bob@x", "b1");
    write(dir, "shared.rs", "1\n2\n3\n");
    commit_as(dir, "2026-01-03T00:00:00", "Alice", "alice@x", "a2");

    let raw = repo_ecosystem(&EcosystemRequest {
        repo_dir: dir.to_path_buf(),
        limit: 0,
        ignore_patterns: Vec::new(),
    })
    .expect("ecosystem");

    // The mine must record the real, distinct author emails.
    let authors: Vec<&str> = raw.commits.iter().map(|c| c.author.as_str()).collect();
    assert!(authors.contains(&"alice@x"), "authors were {authors:?}");
    assert!(authors.contains(&"bob@x"), "authors were {authors:?}");

    let now = 1_767_500_000;
    let owners = ownership(&raw, now, Granularity::All, 10);
    let shared = owners.iter().find(|o| o.path == "shared.rs").unwrap();
    assert_eq!(shared.authors, 2, "expected 2 authors, got {shared:?}");
    assert_eq!(shared.primary_author, "alice@x");
    assert!((shared.primary_share - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn empty_repo_is_not_an_error() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);
    // No commits yet → git log exits non-zero; ensure we surface it as Err, not
    // a panic. (A fresh repo has no HEAD.)
    let res = repo_ecosystem(&EcosystemRequest {
        repo_dir: dir.to_path_buf(),
        limit: 0,
        ignore_patterns: Vec::new(),
    });
    assert!(res.is_err());
}
