//! `collect_squash_links` — the whole-repo scan behind the graph's ghost
//! connectors (ADR-0139).

use std::process::Command;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// main with a squash-merged `feat` and a genuinely unmerged `orphan`.
fn setup() -> TempDir {
    let td = TempDir::new().unwrap();
    let p = td.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "a\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "root"]);

    git(p, &["checkout", "-qb", "feat"]);
    std::fs::write(p.join("b.txt"), "b\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "feat 1"]);
    std::fs::write(p.join("c.txt"), "c\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "feat 2"]);

    git(p, &["checkout", "-qb", "orphan", "main"]);
    std::fs::write(p.join("z.txt"), "z\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "orphan work"]);

    git(p, &["checkout", "-q", "main"]);
    git(p, &["merge", "--squash", "feat"]);
    git(p, &["commit", "-qm", "squash: feat"]);
    td
}

#[test]
fn links_a_squash_merged_branch_to_the_commit_that_replayed_it() {
    let td = setup();
    let repo = git2::Repository::open(td.path()).unwrap();
    let links = kagi_git::ops::collect_squash_links(&repo).unwrap();

    let feat: Vec<_> = links.iter().filter(|l| l.branch == "feat").collect();
    assert_eq!(feat.len(), 1, "feat should be linked once: {:?}", links);
    let squash_head = git(td.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        feat[0].squash, squash_head,
        "should point at the squash commit"
    );
    assert_eq!(feat[0].tip, git(td.path(), &["rev-parse", "feat"]));

    // The guard that keeps this from becoming "every stale branch is merged".
    assert!(
        !links.iter().any(|l| l.branch == "orphan"),
        "an unmerged branch must not be linked: {:?}",
        links
    );
}
