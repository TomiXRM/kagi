//! Regression: opening kagi AT a linked worktree must flag that worktree as
//! `is_current` and report each worktree's own branch/HEAD — the graph's WIP
//! row labels and attached/detached 🌲 badges come from these.

use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .status()
        .expect("git");
    assert!(st.success(), "git {:?} failed", args);
}

#[test]
fn snapshot_from_a_linked_worktree_marks_it_current_with_its_own_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let td = tempfile::tempdir().unwrap();
    let main = td.path().join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "master"]);
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(&main, &["branch", "feat"]);
    // Claude Code style: the worktree lives INSIDE the main repo's directory.
    let wt = main.join(".claude").join("worktrees").join("feat");
    std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
    git(
        &main,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "feat"],
    );

    let mut b = kagi_git::Backend::open(&wt).expect("open worktree");
    let snap = b.snapshot(100).expect("snapshot");
    let by_branch: Vec<(Option<String>, bool, bool)> = snap
        .worktrees
        .iter()
        .map(|w| (w.branch.clone(), w.is_current, w.is_main))
        .collect();
    let feat = snap
        .worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some("feat"))
        .unwrap_or_else(|| panic!("no feat worktree in {:?}", by_branch));
    assert!(
        feat.is_current,
        "linked worktree must be current: {:?}",
        by_branch
    );
    assert!(!feat.is_main);
    let master = snap
        .worktrees
        .iter()
        .find(|w| w.is_main)
        .expect("main worktree listed");
    assert_eq!(master.branch.as_deref(), Some("master"));
    assert!(
        !master.is_current,
        "main must NOT be current here: {:?}",
        by_branch
    );
    // HEAD of the opened repo is the worktree's branch.
    assert!(
        matches!(&snap.head, kagi_git::Head::Attached { branch, .. } if branch == "feat"),
        "head = {:?}",
        snap.head
    );
}

#[test]
fn detached_worktree_reports_no_branch_and_keeps_its_head_when_clean_or_dirty() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let td = tempfile::tempdir().unwrap();
    let main = td.path().join("main");
    let detached = td.path().join("detached");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            detached.to_str().unwrap(),
            "HEAD",
        ],
    );
    git(&detached, &["checkout", "-q", "--orphan", "detached-only"]);
    std::fs::write(detached.join("a.txt"), "unreachable\n").unwrap();
    std::fs::write(detached.join("only.txt"), "detached\n").unwrap();
    git(&detached, &["add", "."]);
    git(&detached, &["commit", "-qm", "unreachable detached root"]);
    git(&detached, &["checkout", "-q", "--detach", "HEAD"]);
    git(&main, &["branch", "-D", "detached-only"]);
    let detached = detached.canonicalize().expect("canonical detached path");

    let mut backend = kagi_git::Backend::open(&main).expect("open main worktree");
    // A zero ordinary-history budget still pins registered detached roots.
    let clean = backend.snapshot(0).expect("clean snapshot");
    let detached_clean = clean
        .worktrees
        .iter()
        .find(|worktree| worktree.path == detached)
        .expect("detached worktree listed while clean");
    assert_eq!(detached_clean.branch, None, "detached HEAD is not a branch");
    let head = detached_clean
        .head
        .clone()
        .expect("clean detached HEAD is retained");
    assert_eq!(detached_clean.wip, None);
    assert_eq!(
        clean
            .commits
            .iter()
            .map(|commit| &commit.id)
            .collect::<Vec<_>>(),
        vec![&head],
        "unreachable detached HEAD must be pinned beyond the history budget"
    );

    std::fs::write(detached.join("dirty.txt"), "dirty\n").unwrap();
    let dirty = backend.snapshot(100).expect("dirty snapshot");
    let detached_dirty = dirty
        .worktrees
        .iter()
        .find(|worktree| worktree.path == detached)
        .expect("detached worktree listed while dirty");
    assert_eq!(detached_dirty.branch, None);
    assert_eq!(detached_dirty.head.as_ref(), Some(&head));
    assert!(
        detached_dirty
            .wip
            .as_ref()
            .is_some_and(|wip| wip.is_dirty()),
        "dirty detached worktree keeps WIP state"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
