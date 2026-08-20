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

/// The safety regression this feature shipped with. `git patch-id` strips
/// whitespace, so a branch whose only difference from the squash commit is
/// indentation used to be reported as squash-merged — in Python that is a
/// behaviour change, and `git branch -d` refuses to delete it. kagi must not
/// be looser than git on an irreversible delete.
#[test]
fn a_whitespace_only_difference_is_not_a_squash_merge() {
    let td = TempDir::new().unwrap();
    let p = td.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("m.py"), "def f(a):\n    if a:\n        return 1\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "root"]);

    // The branch indents the new line INSIDE the `if`.
    git(p, &["checkout", "-qb", "ws"]);
    std::fs::write(
        p.join("m.py"),
        "def f(a):\n    if a:\n        return 1\n        return 2\n",
    )
    .unwrap();
    git(p, &["commit", "-qam", "add return 2"]);

    // main gets the same line at a DIFFERENT indent — different behaviour,
    // identical patch-id.
    git(p, &["checkout", "-q", "main"]);
    std::fs::write(
        p.join("m.py"),
        "def f(a):\n    if a:\n        return 1\n    return 2\n",
    )
    .unwrap();
    git(p, &["commit", "-qam", "add return 2 (outside the if)"]);

    let repo = git2::Repository::open(p).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap().id();
    let tip = repo
        .find_branch("ws", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();

    // Precondition: patch-id alone cannot tell these apart.
    let pid = |from: git2::Oid, to: git2::Oid| {
        let a = repo.find_commit(from).unwrap().tree().unwrap();
        let b = repo.find_commit(to).unwrap().tree().unwrap();
        repo.diff_tree_to_tree(Some(&a), Some(&b), None)
            .unwrap()
            .patchid(None)
            .unwrap()
    };
    let base = repo.merge_base(head, tip).unwrap();
    let head_parent = repo.find_commit(head).unwrap().parent(0).unwrap().id();
    assert_eq!(
        pid(base, tip),
        pid(head_parent, head),
        "precondition: patch-id must collide, or this test proves nothing"
    );

    assert_eq!(
        kagi_git::ops::squash_merged_as(&repo, tip, head),
        None,
        "a whitespace-only difference must not unblock the delete"
    );
    assert!(
        !kagi_git::ops::collect_squash_links(&repo)
            .unwrap()
            .iter()
            .any(|l| l.branch == "ws"),
        "and must not draw a ghost connector either"
    );
}

/// An empty diff hashes to one fixed patch-id, so without a guard every
/// net-zero branch matches every `--allow-empty` commit — and each other.
#[test]
fn a_net_zero_branch_is_not_squash_merged_by_an_empty_commit() {
    let td = TempDir::new().unwrap();
    let p = td.path();
    git(p, &["init", "-q", "-b", "main"]);
    std::fs::write(p.join("a.txt"), "a\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "root"]);

    for (branch, file) in [("nulldiff", "tmp.txt"), ("nulldiff2", "other.txt")] {
        git(p, &["checkout", "-qb", branch, "main"]);
        std::fs::write(p.join(file), "x\n").unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-qm", "add"]);
        std::fs::remove_file(p.join(file)).unwrap();
        git(p, &["commit", "-qam", "and remove it again"]);
    }

    git(p, &["checkout", "-q", "main"]);
    git(p, &["commit", "-q", "--allow-empty", "-m", "retrigger ci"]);

    let repo = git2::Repository::open(p).unwrap();
    let links = kagi_git::ops::collect_squash_links(&repo).unwrap();
    assert!(
        links.is_empty(),
        "an empty diff must not be a universal collider: {:?}",
        links
    );
}
