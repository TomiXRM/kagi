//! Low-level safe checkout oracle; the public Backend refuses dirt earlier.
use super::*;
use std::process::Command;
use tempfile::TempDir;

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
        "git {} exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
    Repository::open(dir).expect("open repo")
}

fn conflicting_merge_repo() -> (TempDir, Repository) {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "same.txt", "base\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "same base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "same.txt", "feature\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "same.txt", "main\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "main change"]);

    (tmp, repo)
}

#[test]
fn merge_into_conflict_keeps_unrelated_dirty_file() {
    let (tmp, repo) = conflicting_merge_repo();
    let dir = tmp.path();

    write_file(dir, "base.txt", "UNSAVED USER WORK\n");

    let files = execute_merge_into_conflict(&repo, "feature").expect("merge into conflict");
    assert!(
        files.iter().any(|f| f == "same.txt"),
        "expected same.txt conflicted, got {:?}",
        files
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("base.txt")).unwrap(),
        "UNSAVED USER WORK\n",
        "safe-mode merge checkout must not discard uncommitted work"
    );
}
