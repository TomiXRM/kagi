//! C2 regression probes for prediction CLI candidates (#627 P1).

use std::path::Path;
use std::process::Command;

use kagi_git::benchmark::fingerprint_repository;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn divergent_repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.name", "Prediction test"]);
    git(repo, &["config", "user.email", "prediction@example.test"]);
    std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["branch", "topic"]);
    std::fs::write(repo.join("main.txt"), "main\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "main"]);
    git(repo, &["checkout", "-q", "topic"]);
    std::fs::write(repo.join("topic.txt"), "topic\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "topic"]);
    git(repo, &["checkout", "-q", "main"]);
    root
}

#[test]
fn merge_tree_write_tree_changes_the_complete_repository_fingerprint() {
    let root = divergent_repository();
    let before = fingerprint_repository(root.path()).unwrap();

    git(
        root.path(),
        &["merge-tree", "--write-tree", "main", "topic"],
    );

    let after = fingerprint_repository(root.path()).unwrap();
    assert_ne!(
        before, after,
        "--write-tree must be rejected for prediction"
    );
}

#[test]
fn diff_tree_candidate_preserves_the_complete_repository_fingerprint() {
    let root = divergent_repository();
    let before = fingerprint_repository(root.path()).unwrap();

    git(root.path(), &["diff", "--no-ext-diff", "main", "topic"]);

    let after = fingerprint_repository(root.path()).unwrap();
    assert_eq!(
        before, after,
        "tree diff candidate must not write repository state"
    );
}

#[test]
fn cherry_pick_no_commit_changes_the_complete_repository_fingerprint() {
    let root = divergent_repository();
    let before = fingerprint_repository(root.path()).unwrap();

    git(root.path(), &["cherry-pick", "--no-commit", "topic"]);

    let after = fingerprint_repository(root.path()).unwrap();
    assert_ne!(
        before, after,
        "--no-commit must be rejected for prediction"
    );
}

#[test]
fn merge_file_stdout_candidate_preserves_the_complete_repository_fingerprint() {
    let root = divergent_repository();
    std::fs::write(root.path().join("ancestor"), "base\n").unwrap();
    std::fs::write(root.path().join("ours"), "base\n").unwrap();
    std::fs::write(root.path().join("theirs"), "theirs\n").unwrap();
    let before = fingerprint_repository(root.path()).unwrap();

    git(
        root.path(),
        &["merge-file", "-p", "ours", "ancestor", "theirs"],
    );

    let after = fingerprint_repository(root.path()).unwrap();
    assert_eq!(
        before, after,
        "merge-file stdout candidate must not write repository state"
    );
}

#[test]
fn all_diff_candidates_preserve_the_complete_repository_fingerprint() {
    for args in [
        vec!["diff", "--no-ext-diff", "main", "topic"],
        vec!["diff", "--no-ext-diff", "--cached"],
        vec!["diff", "--no-ext-diff"],
        vec!["diff", "--no-ext-diff", "HEAD"],
    ] {
        let root = divergent_repository();
        let before = fingerprint_repository(root.path()).unwrap();

        git(root.path(), &args);

        let after = fingerprint_repository(root.path()).unwrap();
        assert_eq!(before, after, "{args:?} must not write repository state");
    }
}
