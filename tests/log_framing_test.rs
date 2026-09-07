//! Integration test for the `git log` wire framing (issue #508, ADR-0186).
//!
//! Builds a **real** repository whose commit messages embed the ASCII field /
//! record separators the old framing used, runs the real `git` binary with the
//! real [`LOG_FORMAT`], and feeds the bytes to the real pure parser. Merely
//! *reading* such a repository used to forge extra graph rows — one commit
//! parsed as two — so this asserts the row count against
//! `git rev-list --count` and every field of every row.
//!
//! All writes are confined to a `TempDir`; no existing repository is touched.

use std::path::{Path, PathBuf};
use std::process::Command;

use kagi_domain::remote_snapshot::{parse_commits, LOG_FORMAT};
use tempfile::TempDir;

/// The exact bytes an attacker embeds in a commit message: an ASCII record
/// separator followed by a complete, well-formed record in the *old* wire
/// format, ending in a plausible summary.
const FORGED: &str = "\u{1e}0000000000000000000000000000000000000000\u{1f}\u{1f}Eve\u{1f}\
                      eve@example.com\u{1f}1700000000\u{1f}Eve\u{1f}eve@example.com\u{1f}\
                      1700000000\u{1f}forged row\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git failed to start");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(dir: &Path, file: &str, body: &str, message: &str) {
    std::fs::write(dir.join(file), body).expect("write");
    git(dir, &["add", file]);
    let msg_path = dir.join("msg.tmp");
    std::fs::write(&msg_path, message).expect("write message");
    git(dir, &["commit", "-q", "-F", "msg.tmp"]);
    std::fs::remove_file(&msg_path).expect("rm message");
}

fn init(tmp: &TempDir) -> PathBuf {
    let d = tmp.path().to_path_buf();
    git(&d, &["init", "-q", "-b", "main", "."]);
    git(&d, &["config", "user.name", "Test"]);
    git(&d, &["config", "user.email", "test@example.com"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    d
}

/// Run the real `git log` with the real format and parse it.
fn parsed(dir: &Path) -> Vec<kagi_domain::commit::Commit> {
    let fmt = format!("--pretty=format:{LOG_FORMAT}");
    let stdout = git(
        dir,
        &["log", "--topo-order", fmt.as_str(), "--branches", "--tags"],
    );
    parse_commits(&stdout)
}

#[test]
fn separator_bytes_in_commit_message_cannot_forge_a_row() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = init(&tmp);

    commit(&dir, "a.txt", "one\n", "first\n");
    let evil = format!("evil subject\n\nbody{FORGED}end\n");
    commit(&dir, "a.txt", "two\n", &evil);

    let real_count: usize = git(&dir, &["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .expect("rev-list count");
    assert_eq!(real_count, 2, "fixture should hold exactly two commits");

    let commits = parsed(&dir);
    assert_eq!(
        commits.len(),
        real_count,
        "parsed rows must match git's real history"
    );

    // No row may carry the forged identity, oid or summary.
    assert!(
        !commits.iter().any(|c| c.id.0.starts_with("000000")
            || c.author.name == "Eve"
            || c.summary == "forged row"),
        "a forged row reached the graph: {commits:#?}"
    );

    // The crafted commit keeps every field, and its body round-trips whole.
    let head = &commits[0];
    assert_eq!(head.summary, "evil subject");
    assert_eq!(head.author.name, "Test");
    assert_eq!(head.author.email, "test@example.com");
    assert_eq!(head.committer.name, "Test");
    assert!(head.author.time > 0);
    assert_eq!(head.message, evil, "commit body must round-trip verbatim");
    assert_eq!(head.parents.len(), 1);

    assert_eq!(commits[1].summary, "first");
    assert!(commits[1].parents.is_empty());
    assert_eq!(head.parents[0], commits[1].id);
}

/// Ordinary history — merge, empty body, non-ASCII, CR — still parses.
#[test]
fn ordinary_history_still_parses() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = init(&tmp);

    commit(&dir, "a.txt", "one\n", "base\n");
    git(&dir, &["checkout", "-q", "-b", "side"]);
    commit(&dir, "b.txt", "side\n", "日本語の件名\n\n本文\r\n");
    git(&dir, &["checkout", "-q", "main"]);
    commit(&dir, "c.txt", "main\n", "main work\n");
    git(
        &dir,
        &["merge", "-q", "--no-ff", "-m", "Merge side", "side"],
    );
    // A commit with an empty message body.
    std::fs::write(dir.join("d.txt"), "d\n").expect("write");
    git(&dir, &["add", "d.txt"]);
    git(&dir, &["commit", "-q", "--allow-empty-message", "-m", ""]);

    let real_count: usize = git(&dir, &["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .expect("rev-list count");
    let commits = parsed(&dir);
    assert_eq!(commits.len(), real_count);

    let merge = commits
        .iter()
        .find(|c| c.summary == "Merge side")
        .expect("merge commit present");
    assert_eq!(merge.parents.len(), 2);

    assert!(commits.iter().any(|c| c.summary == "日本語の件名"));
    assert!(commits.iter().any(|c| c.summary.is_empty()));
    assert!(commits.iter().all(|c| c.id.0.len() >= 40));
}
