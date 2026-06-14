//! Integration tests for Smart Commit Message backend (T-COMMIT-015, ADR-0044).
//!
//! Each test builds a small Git repository in a `tempfile::TempDir` using the
//! `git` CLI, then exercises the staged-diff collection and rule-based / offline
//! fallback paths.  No real network / Ollama is touched — the LLM HTTP path is
//! exercised only in the in-module unit tests via `KAGI_OFFLINE`.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    collect_staged_diff, collect_staged_files, generate_message, rule_based, ChangeKind, GenError,
    GenInput, Lang, MessageBackend, Style,
};

// ────────────────────────────────────────────────────────────
// Test helpers
// ────────────────────────────────────────────────────────────

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
        .expect("git command failed to start");
    assert!(
        status.success(),
        "git {} exited with {:?}",
        args.join(" "),
        status.code()
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Repository::open(dir).expect("failed to open repo")
}

fn input(lang: Lang, style: Style) -> GenInput {
    GenInput {
        diff: String::new(),
        lang,
        style,
    }
}

// ────────────────────────────────────────────────────────────
// collect_staged_diff — staged ONLY (no unstaged leakage)
// ────────────────────────────────────────────────────────────

#[test]
fn collect_staged_diff_is_empty_when_nothing_staged() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    assert_eq!(collect_staged_diff(&repo), "");
    assert!(collect_staged_files(&repo).is_empty());
}

#[test]
fn collect_staged_diff_includes_staged_excludes_unstaged() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Stage one new file.
    write_file(dir, "staged.txt", "STAGED_CONTENT_MARKER\n");
    git(dir, &["add", "staged.txt"]);

    // Create another change that is NOT staged.
    write_file(dir, "unstaged.txt", "UNSTAGED_CONTENT_MARKER\n");
    // (left untracked, not added)

    // And modify a tracked file without staging it.
    write_file(dir, "base.txt", "base\nUNSTAGED_MODIFICATION_MARKER\n");

    let diff = collect_staged_diff(&repo);
    assert!(
        diff.contains("STAGED_CONTENT_MARKER"),
        "staged content missing from diff:\n{diff}"
    );
    assert!(
        !diff.contains("UNSTAGED_CONTENT_MARKER"),
        "untracked content leaked into staged diff:\n{diff}"
    );
    assert!(
        !diff.contains("UNSTAGED_MODIFICATION_MARKER"),
        "unstaged modification leaked into staged diff:\n{diff}"
    );

    // The staged file set reflects only the staged add.
    let files = collect_staged_files(&repo);
    assert_eq!(
        files.len(),
        1,
        "expected exactly one staged file: {files:?}"
    );
    assert_eq!(files[0].path, Path::new("staged.txt"));
    assert_eq!(files[0].change, ChangeKind::Added);
}

#[test]
fn collect_staged_diff_partial_stage_only_sees_staged_hunk() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Modify base.txt and stage it, then add a further unstaged modification.
    write_file(dir, "base.txt", "base\nSTAGED_LINE\n");
    git(dir, &["add", "base.txt"]);
    write_file(dir, "base.txt", "base\nSTAGED_LINE\nEXTRA_UNSTAGED_LINE\n");

    let diff = collect_staged_diff(&repo);
    assert!(diff.contains("STAGED_LINE"), "diff:\n{diff}");
    assert!(
        !diff.contains("EXTRA_UNSTAGED_LINE"),
        "unstaged hunk leaked:\n{diff}"
    );
}

// ────────────────────────────────────────────────────────────
// rule_based against a real staged set
// ────────────────────────────────────────────────────────────

#[test]
fn rule_based_from_real_staged_files_is_nonempty() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "feature.rs", "fn x() {}\n");
    git(dir, &["add", "feature.rs"]);

    let files = collect_staged_files(&repo);
    let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
    assert!(!msg.is_empty());
    assert!(msg.starts_with("feat"), "got: {msg}");
}

// ────────────────────────────────────────────────────────────
// offline fallback: Ollama backend Errs, rule-based stays usable
// ────────────────────────────────────────────────────────────

#[test]
fn ollama_offline_errs_and_rule_based_recovers() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    std::fs::create_dir_all(dir.join("src")).expect("mkdir src");
    write_file(dir, "src/mod.rs", "// stuff\n");
    git(dir, &["add", "src/mod.rs"]);

    let files = collect_staged_files(&repo);
    let gi = GenInput {
        diff: collect_staged_diff(&repo),
        lang: Lang::En,
        style: Style::ConventionalCommits,
    };

    // Force offline so the Ollama path never touches the network.
    let prev = std::env::var("KAGI_OFFLINE").ok();
    std::env::set_var("KAGI_OFFLINE", "1");

    let ollama = generate_message(
        &MessageBackend::Ollama {
            host: "localhost:11434".to_string(),
            model: "gemma".to_string(),
        },
        &gi,
        &files,
    );
    assert_eq!(ollama, Err(GenError::Offline));

    // The caller's fallback — rule-based — must still produce a draft.
    let fallback = generate_message(&MessageBackend::RuleBased, &gi, &files)
        .expect("rule-based is infallible");
    assert!(!fallback.is_empty());

    match prev {
        Some(v) => std::env::set_var("KAGI_OFFLINE", v),
        None => std::env::remove_var("KAGI_OFFLINE"),
    }
}
