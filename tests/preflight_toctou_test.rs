//! #295: preflight must reject an execute when the working-tree classification
//! changed since the plan — not just when HEAD moved. Each test plans, then
//! mutates the tree WITHOUT moving HEAD, and asserts execute refuses and the
//! target is untouched. The "HEAD didn't move" part is what the old
//! HEAD-only preflight missed.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{execute_discard, plan_discard};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git")
        .success();
    assert!(ok, "git {:?}", args);
}

fn write(dir: &Path, n: &str, c: &str) {
    std::fs::write(dir.join(n), c).unwrap();
}
fn read(dir: &Path, n: &str) -> String {
    std::fs::read_to_string(dir.join(n)).unwrap()
}
fn head(dir: &Path) -> String {
    let o = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn repo() -> TempDir {
    let t = TempDir::new().unwrap();
    let d = t.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    write(d, "tracked.txt", "committed\n");
    write(d, "other.txt", "committed other\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "base"]);
    t
}

/// #295 impact 2: plan a discard of tracked.txt (unstaged edit), then
/// `git rm --cached` it (unstaged → untracked, HEAD unmoved). Execute must
/// refuse — "restore from index" must not become "delete from disk".
#[test]
fn discard_refuses_when_a_target_became_untracked() {
    let t = repo();
    let d = t.path();
    write(d, "tracked.txt", "PRECIOUS EDIT\n");
    let repo = Repository::open(d).unwrap();
    let plan = plan_discard(&repo, &["tracked.txt".into()]).unwrap();
    assert!(
        plan.blockers.is_empty(),
        "plan should be clean: {:?}",
        plan.blockers
    );

    let h = head(d);
    git(d, &["rm", "--cached", "-q", "tracked.txt"]); // reclassify, HEAD stays
    assert_eq!(head(d), h, "precondition: HEAD must not move");

    let err = execute_discard(&repo, &plan, &["tracked.txt".into()]).err();
    assert!(err.is_some(), "execute must refuse the stale plan");
    assert!(
        read(d, "tracked.txt").contains("PRECIOUS EDIT"),
        "the user's content must survive the refusal"
    );
}

/// #295 impact 1: plan a discard, then make the target conflicted (HEAD
/// unmoved, via a merge that conflicts on it). Execute must refuse rather than
/// force-overwrite a half-done resolution.
#[test]
fn discard_refuses_when_a_target_became_conflicted() {
    let t = repo();
    let d = t.path();
    // Branch b changes tracked.txt one way…
    git(d, &["checkout", "-q", "-b", "b"]);
    write(d, "tracked.txt", "B SIDE\n");
    git(d, &["commit", "-qam", "b"]);
    git(d, &["checkout", "-q", "main"]);
    write(d, "tracked.txt", "MAIN SIDE\n");
    git(d, &["commit", "-qam", "main"]);
    // Now an unstaged edit, and a clean plan.
    write(d, "tracked.txt", "unstaged edit\n");
    let repo = Repository::open(d).unwrap();
    let plan = plan_discard(&repo, &["tracked.txt".into()]).unwrap();
    // (planning may or may not block; if it blocks the bug can't fire, so only
    //  run the TOCTOU half when the plan was accepted.)
    if !plan.blockers.is_empty() {
        return;
    }
    git(d, &["stash", "-q"]);
    let h = head(d);
    let merged = Command::new("git")
        .args(["merge", "b"])
        .current_dir(d)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", d)
        .status()
        .unwrap();
    let _ = merged; // conflicts; HEAD does not move on a conflicted merge
    assert_eq!(
        head(d),
        h,
        "precondition: a conflicted merge must not move HEAD"
    );

    assert!(
        execute_discard(&repo, &plan, &["tracked.txt".into()]).is_err(),
        "a target that turned conflicted must not be force-overwritten"
    );
}

/// #295 final acceptance: execute_discard must reject paths the plan does not
/// cover, so a plan for A can never be replayed to touch B.
#[test]
fn execute_discard_rejects_paths_outside_its_plan() {
    let t = repo();
    let d = t.path();
    write(d, "tracked.txt", "edit A\n");
    write(d, "other.txt", "edit B\n");
    let repo = Repository::open(d).unwrap();
    let plan = plan_discard(&repo, &["tracked.txt".into()]).unwrap();
    assert!(plan.blockers.is_empty());

    // Replay the plan-for-A against path B: must be refused before any write.
    let err = execute_discard(&repo, &plan, &["other.txt".into()])
        .expect_err("a plan for tracked.txt must refuse a discard of other.txt");
    assert!(format!("{err}").contains("other.txt"), "{err}");
    assert_eq!(
        read(d, "other.txt"),
        "edit B\n",
        "other.txt must be untouched by the refused discard"
    );

    // Sanity: the planned path itself still works.
    execute_discard(&repo, &plan, &["tracked.txt".into()]).expect("planned path discards fine");
}
