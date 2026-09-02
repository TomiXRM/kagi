//! Worktree creation pipeline tests (T-CM-023/T-CM-024).

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::{CommonNote, PlanNote, WorktreeNote};
use kagi_git::{
    ops::{
        execute_create_worktree, execute_open_worktree_for_branch, plan_create_worktree,
        plan_open_worktree_for_branch, preflight_check, validate_worktree_path,
    },
    CommitId,
};

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

fn build_repo(tmp: &TempDir) -> Repository {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);

    Repository::open(d).expect("failed to open repo")
}

fn head_commit_id(repo: &Repository) -> CommitId {
    CommitId(
        repo.head()
            .expect("head")
            .target()
            .expect("head target")
            .to_string(),
    )
}

#[test]
fn create_worktree_success_creates_branch_and_linked_repo() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-feature");

    let plan = plan_create_worktree(&repo, "wt-feature", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );

    preflight_check(&repo, &plan).expect("preflight");
    execute_create_worktree(&repo, "wt-feature", &path, &at).expect("execute_create_worktree");

    assert!(path.join("README.md").exists());
    assert!(repo
        .find_branch("wt-feature", git2::BranchType::Local)
        .is_ok());
    let linked = Repository::open(&path).expect("open linked worktree");
    assert_eq!(linked.head().unwrap().shorthand().ok(), Some("wt-feature"));
}

#[test]
fn create_worktree_path_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("exists");
    std::fs::create_dir(&path).unwrap();

    let plan =
        plan_create_worktree(&repo, "wt-collision", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("already exists")),
        "expected path collision blocker, got {:?}",
        plan.blockers
    );
}

#[test]
fn create_worktree_branch_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-main");

    let plan = plan_create_worktree(&repo, "main", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("already exists")),
        "expected branch collision blocker, got {:?}",
        plan.blockers
    );
}

#[test]
fn create_worktree_preflight_detects_head_move() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-preflight");

    let plan =
        plan_create_worktree(&repo, "wt-preflight", &path, &at).expect("plan_create_worktree");
    assert!(plan.blockers.is_empty());

    write_file(repo_tmp.path(), "second.txt", "second\n");
    git(repo_tmp.path(), &["add", "second.txt"]);
    git(repo_tmp.path(), &["commit", "-qm", "second commit"]);

    let moved_repo = Repository::open(repo_tmp.path()).unwrap();
    assert!(
        preflight_check(&moved_repo, &plan).is_err(),
        "preflight should reject a moved HEAD"
    );
}

#[test]
fn validate_worktree_path_rejects_repo_inside_and_accepts_japanese_path() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo_root = repo_tmp.path();

    let inside = validate_worktree_path(repo_root, "inside-wt");
    assert!(inside.is_err(), "repo-internal path should be rejected");

    let japanese = worktrees_tmp.path().join("作業ツリー");
    let normalized =
        validate_worktree_path(repo_root, &japanese).expect("Japanese path should validate");
    assert_eq!(
        normalized,
        std::fs::canonicalize(worktrees_tmp.path())
            .unwrap()
            .join("作業ツリー")
    );
}

// ────────────────────────────────────────────────────────────
// unlock-worktree triple
// ────────────────────────────────────────────────────────────

/// Add a linked worktree named `name` on a fresh branch and return its path.
fn add_worktree(repo_dir: &Path, name: &str) -> std::path::PathBuf {
    let wt_path = repo_dir.join(name);
    git(
        repo_dir,
        &["worktree", "add", "-b", name, wt_path.to_str().unwrap()],
    );
    wt_path
}

#[test]
fn unlock_plan_surfaces_lock_reason_and_execute_unlocks() {
    let tmp = TempDir::new().expect("tempdir");
    let repo = build_repo(&tmp);
    let d = tmp.path();
    let wt_path = add_worktree(d, "wt-locked");
    git(
        d,
        &[
            "worktree",
            "lock",
            "--reason",
            "agent still running",
            wt_path.to_str().unwrap(),
        ],
    );

    let plan = kagi_git::ops::plan_unlock_worktree(&repo, "wt-locked").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert!(!plan.destructive);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("Locked with reason")
                && w.message_en().contains("agent still running")),
        "warning must show the recorded reason: {:?}",
        plan.warnings
    );

    kagi_git::ops::execute_unlock_worktree(&repo, &plan, "wt-locked").expect("execute");
    let wt = repo.find_worktree("wt-locked").expect("worktree");
    assert!(matches!(
        wt.is_locked(),
        Ok(git2::WorktreeLockStatus::Unlocked)
    ));
}

#[test]
fn unlock_plan_without_reason_notes_none_recorded() {
    let tmp = TempDir::new().expect("tempdir");
    let repo = build_repo(&tmp);
    let d = tmp.path();
    let wt_path = add_worktree(d, "wt-locked-bare");
    git(d, &["worktree", "lock", wt_path.to_str().unwrap()]);

    let plan = kagi_git::ops::plan_unlock_worktree(&repo, "wt-locked-bare").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("(no reason recorded)")),
        "warning must note the missing reason: {:?}",
        plan.warnings
    );
}

#[test]
fn unlock_unlocked_worktree_is_blocked() {
    let tmp = TempDir::new().expect("tempdir");
    let repo = build_repo(&tmp);
    add_worktree(tmp.path(), "wt-free");

    let plan = kagi_git::ops::plan_unlock_worktree(&repo, "wt-free").expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("already unlocked")),
        "unlocked worktree must be a blocker: {:?}",
        plan.blockers
    );
    // Execute (simulating a stale confirm) must refuse too.
    assert!(kagi_git::ops::execute_unlock_worktree(&repo, &plan, "wt-free").is_err());
}

#[test]
fn unlock_missing_worktree_is_blocked() {
    let tmp = TempDir::new().expect("tempdir");
    let repo = build_repo(&tmp);

    let plan = kagi_git::ops::plan_unlock_worktree(&repo, "no-such").expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("does not exist")),
        "missing worktree must be a blocker: {:?}",
        plan.blockers
    );
}

/// A worktree whose `locked` metadata cannot be read is neither "locked" nor
/// "unlocked" — kagi must not guess, it must block. Making `locked` a
/// directory is the cheapest way to make libgit2's read of it fail.
#[test]
fn unlock_plan_reports_an_unreadable_lock_state() {
    let tmp = TempDir::new().expect("tempdir");
    let repo = build_repo(&tmp);
    add_worktree(tmp.path(), "wt-broken");

    std::fs::create_dir(tmp.path().join(".git/worktrees/wt-broken/locked"))
        .expect("create locked dir");

    let plan = kagi_git::ops::plan_unlock_worktree(&repo, "wt-broken").expect("plan");
    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Worktree(WorktreeNote::LockStateUnreadable { name, .. }) if name == "wt-broken"
        )),
        "expected WorktreeNote::LockStateUnreadable, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// open-worktree-for-an-existing-branch triple
// ────────────────────────────────────────────────────────────

/// `plan_open_worktree_for_branch` resolves the name through the same
/// `resolve_branch_commit` fallback every op uses, so a tag resolves to a
/// commit and never reaches `find_branch(.., Local)`. "Open a worktree for
/// this branch" on a name that is not a local branch must be a blocker.
#[test]
fn open_worktree_for_a_name_that_is_not_a_local_branch_is_blocked() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    git(repo_tmp.path(), &["tag", "v1"]);

    let plan = plan_open_worktree_for_branch(&repo, "v1", worktrees_tmp.path().join("wt-v1"))
        .expect("plan_open_worktree_for_branch");

    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Common(CommonNote::BranchMissing { name, in_repo: true }) if name == "v1"
        )),
        "expected CommonNote::BranchMissing for the tag name, got: {:?}",
        plan.blockers
    );
}

/// Git allows one worktree per branch. Opening a second worktree on a branch
/// that is already checked out elsewhere must block, and name the worktree
/// that holds it so the user can go there instead.
#[test]
fn open_worktree_for_a_branch_checked_out_elsewhere_is_blocked() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let first = add_worktree(repo_tmp.path(), "taken");

    let plan =
        plan_open_worktree_for_branch(&repo, "taken", worktrees_tmp.path().join("taken-again"))
            .expect("plan_open_worktree_for_branch");

    let hit = plan.blockers.iter().find_map(|b| match b {
        PlanNote::Worktree(WorktreeNote::BranchInOtherWorktree { branch, path }) => {
            Some((branch, path))
        }
        _ => None,
    });
    let (branch, path) = hit.unwrap_or_else(|| {
        panic!(
            "expected WorktreeNote::BranchInOtherWorktree, got: {:?}",
            plan.blockers
        )
    });
    assert_eq!(branch, "taken");
    assert!(
        path.contains("taken"),
        "the blocker must name the worktree holding the branch ({}), got {}",
        first.display(),
        path
    );
}

/// The execute side of "open a worktree for this existing branch": a real
/// directory on disk, checked out on the branch, holding the branch's own
/// content — and no new branch invented.
#[test]
fn execute_open_worktree_for_branch_checks_out_the_existing_branch() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let d = repo_tmp.path();
    let repo = build_repo(&repo_tmp);

    git(d, &["checkout", "-q", "-b", "existing"]);
    write_file(d, "only-on-branch.txt", "branch content\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "branch commit"]);
    let branch_tip = repo
        .find_branch("existing", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    git(d, &["checkout", "-q", "main"]);

    let path = worktrees_tmp.path().join("wt-existing");
    let plan = plan_open_worktree_for_branch(&repo, "existing", &path).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    execute_open_worktree_for_branch(&repo, "existing", &path)
        .expect("execute_open_worktree_for_branch");

    assert!(path.is_dir(), "worktree directory must exist");
    assert_eq!(
        std::fs::read_to_string(path.join("only-on-branch.txt")).expect("branch file in worktree"),
        "branch content\n"
    );
    let linked = Repository::open(&path).expect("open linked worktree");
    assert_eq!(linked.head().unwrap().shorthand().ok(), Some("existing"));
    assert_eq!(
        linked.head().unwrap().target().unwrap(),
        branch_tip,
        "the worktree must sit on the existing branch tip, not a new branch"
    );
}

// ────────────────────────────────────────────────────────────
// issue #339 — .worktreeinclude copies gitignored files into a new worktree
// ────────────────────────────────────────────────────────────

/// Repo with `.gitignore` + `.worktreeinclude` both listing `.env`, and a
/// present, gitignored `.env`.
fn build_repo_with_worktreeinclude(tmp: &TempDir) -> Repository {
    let d = tmp.path();
    let repo = build_repo(tmp);
    write_file(d, ".gitignore", ".env\n");
    write_file(d, ".worktreeinclude", ".env\n");
    git(d, &["add", ".gitignore", ".worktreeinclude"]);
    git(d, &["commit", "-qm", "add ignore + include"]);
    write_file(d, ".env", "TOKEN=abc\n");
    repo
}

#[test]
fn worktreeinclude_copies_ignored_env() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo_with_worktreeinclude(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt");

    let plan = plan_create_worktree(&repo, "wt", &path, &at).expect("plan");
    let listed = plan.warnings.iter().any(
        |w| matches!(w, PlanNote::Worktree(WorktreeNote::IncludeCopy { count, .. }) if *count >= 1),
    );
    assert!(
        listed,
        "plan must list the include copy: {:?}",
        plan.warnings
    );

    execute_create_worktree(&repo, "wt", &path, &at).expect("execute");
    assert_eq!(
        std::fs::read_to_string(path.join(".env")).expect(".env copied"),
        "TOKEN=abc\n"
    );
}

#[test]
fn worktreeinclude_does_not_copy_tracked_file() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let d = repo_tmp.path();
    let repo = build_repo(&repo_tmp);
    // config.toml is TRACKED but also matches .worktreeinclude.
    write_file(d, ".worktreeinclude", "config.toml\n");
    write_file(d, "config.toml", "tracked=1\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "track config"]);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt");

    let plan = plan_create_worktree(&repo, "wt", &path, &at).expect("plan");
    assert!(
        !plan
            .warnings
            .iter()
            .any(|w| matches!(w, PlanNote::Worktree(WorktreeNote::IncludeCopy { .. }))),
        "tracked files must not appear in the copy set: {:?}",
        plan.warnings
    );
}

#[test]
fn worktreeinclude_does_not_copy_non_ignored_file() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let d = repo_tmp.path();
    let repo = build_repo(&repo_tmp);
    write_file(d, ".worktreeinclude", "notes.txt\n");
    git(d, &["add", ".worktreeinclude"]);
    git(d, &["commit", "-qm", "include"]);
    // notes.txt is untracked but NOT gitignored.
    write_file(d, "notes.txt", "hi\n");
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt");

    execute_create_worktree(&repo, "wt", &path, &at).expect("execute");
    assert!(
        !path.join("notes.txt").exists(),
        "a matched but non-ignored file must not be copied"
    );
}

#[cfg(unix)]
#[test]
fn worktreeinclude_skips_symlinks() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let d = repo_tmp.path();
    let repo = build_repo(&repo_tmp);
    write_file(d, ".gitignore", "link\n");
    write_file(d, ".worktreeinclude", "link\n");
    git(d, &["add", ".gitignore", ".worktreeinclude"]);
    git(d, &["commit", "-qm", "include link"]);
    std::os::unix::fs::symlink("README.md", d.join("link")).unwrap();
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt");

    let plan = plan_create_worktree(&repo, "wt", &path, &at).expect("plan");
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Worktree(WorktreeNote::IncludeSkippedSymlinks { count }) if *count == 1
        )),
        "plan must note the skipped symlink: {:?}",
        plan.warnings
    );

    execute_create_worktree(&repo, "wt", &path, &at).expect("execute");
    assert!(
        !path.join("link").exists(),
        "matched symlinks must not be copied"
    );
}

// ────────────────────────────────────────────────────────────
// issue #340 — worktree lifecycle: remove / lock / prune / repair
// ────────────────────────────────────────────────────────────

use kagi_git::ops::{
    execute_lock_worktree, execute_prune_worktrees, execute_remove_worktree,
    execute_repair_worktrees, plan_lock_worktree, plan_prune_worktrees, plan_remove_worktree,
    plan_repair_worktrees,
};

/// Worktree-only remove leaves the branch (§6). `delete_branch=false`.
#[test]
fn remove_worktree_only_keeps_branch() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    let wt = add_worktree(tmp.path(), "wt-rm");

    let plan = plan_remove_worktree(&repo, "wt-rm", false).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let backups = execute_remove_worktree(&repo, &plan, "wt-rm", false).expect("execute");
    assert!(backups.is_empty(), "clean worktree needs no backup");
    assert!(!wt.exists(), "worktree dir must be gone");
    assert!(
        repo.find_branch("wt-rm", git2::BranchType::Local).is_ok(),
        "branch must survive a worktree-only remove"
    );
    assert!(repo.find_worktree("wt-rm").is_err(), "admin entry gone");
}

/// Also-delete-branch removes both the worktree and its (merged) branch.
#[test]
fn remove_worktree_also_deletes_branch_when_asked() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    add_worktree(tmp.path(), "wt-rm2");

    let plan = plan_remove_worktree(&repo, "wt-rm2", true).expect("plan");
    execute_remove_worktree(&repo, &plan, "wt-rm2", true).expect("execute");
    assert!(
        repo.find_branch("wt-rm2", git2::BranchType::Local).is_err(),
        "branch must be deleted when delete_branch=true"
    );
}

/// A dirty worktree is a remove blocker (no --force). Mutation-verify: execute
/// also refuses while the blocker stands (reverting the guard lets it through).
#[test]
fn remove_dirty_worktree_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    let wt = add_worktree(tmp.path(), "wt-dirty");
    write_file(&wt, "scratch.txt", "uncommitted work\n"); // untracked = dirty

    let plan = plan_remove_worktree(&repo, "wt-dirty", false).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| matches!(b, PlanNote::Worktree(WorktreeNote::RemoveDirty { .. }))),
        "dirty worktree must be a blocker: {:?}",
        plan.blockers
    );
    assert!(
        execute_remove_worktree(&repo, &plan, "wt-dirty", false).is_err(),
        "execute must refuse a plan carrying blockers"
    );
    assert!(wt.exists(), "the dirty worktree must be left untouched");
}

/// The main worktree is never removable (§6). Mutation-verify: execute refuses.
#[test]
fn remove_main_worktree_is_always_refused() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);

    let plan = plan_remove_worktree(&repo, "main", false).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| matches!(b, PlanNote::Worktree(WorktreeNote::RemoveMainRefused))),
        "main worktree removal must be refused: {:?}",
        plan.blockers
    );
    assert!(
        execute_remove_worktree(&repo, &plan, "main", false).is_err(),
        "execute must refuse to remove the main worktree"
    );
}

/// `lock --reason` records the reason in `git worktree list --porcelain` (§6).
#[test]
fn lock_worktree_reason_appears_in_porcelain() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    add_worktree(tmp.path(), "wt-lock");

    let plan = plan_lock_worktree(&repo, "wt-lock", Some("agent running")).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_lock_worktree(&repo, &plan, "wt-lock", Some("agent running")).expect("execute");

    let out = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(tmp.path())
        .output()
        .expect("git worktree list");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("locked agent running"),
        "porcelain must show the lock reason, got:\n{}",
        text
    );

    // Re-locking is a no-op blocker.
    let plan2 = plan_lock_worktree(&repo, "wt-lock", Some("again")).expect("plan2");
    assert!(
        plan2
            .blockers
            .iter()
            .any(|b| matches!(b, PlanNote::Worktree(WorktreeNote::AlreadyLocked { .. }))),
        "an already-locked worktree must block: {:?}",
        plan2.blockers
    );
}

/// A hand-deleted worktree directory is detected as prunable and pruned (§6).
#[test]
fn hand_deleted_worktree_is_prunable_and_pruned() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    let wt = add_worktree(tmp.path(), "wt-orphan");

    std::fs::remove_dir_all(&wt).expect("hand-delete the worktree dir");

    let plan = plan_prune_worktrees(&repo).expect("plan");
    assert!(
        plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Worktree(WorktreeNote::PrunePreview { count, .. }) if *count >= 1
        )),
        "the hand-deleted worktree must show as prunable: {:?} / {:?}",
        plan.warnings,
        plan.blockers
    );

    let pruned = execute_prune_worktrees(&repo, &plan).expect("execute");
    assert_eq!(pruned, 1);
    assert!(
        repo.find_worktree("wt-orphan").is_err(),
        "admin entry pruned"
    );
}

/// Nothing prunable → a no-op blocker (empty dry-run preview).
#[test]
fn prune_with_nothing_to_prune_is_a_blocker() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    add_worktree(tmp.path(), "wt-live"); // present, not prunable

    let plan = plan_prune_worktrees(&repo).expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| matches!(b, PlanNote::Worktree(WorktreeNote::PruneNothing))),
        "nothing-to-prune must be a blocker: {:?}",
        plan.blockers
    );
}

/// Moving the main worktree breaks the linked worktree's `.git` link; `repair`
/// run from the moved main restores it (§6, case 1).
#[test]
fn repair_restores_links_after_moving_main() {
    let base = TempDir::new().unwrap();
    let main1 = base.path().join("main");
    std::fs::create_dir(&main1).unwrap();
    git(&main1, &["init", "-q", "-b", "main", "."]);
    git(&main1, &["config", "user.name", "Test"]);
    git(&main1, &["config", "user.email", "test@example.com"]);
    git(&main1, &["config", "commit.gpgsign", "false"]);
    write_file(&main1, "README.md", "# test\n");
    git(&main1, &["add", "README.md"]);
    git(&main1, &["commit", "-qm", "initial"]);

    let wt = base.path().join("wt-repair");
    git(
        &main1,
        &["worktree", "add", "-b", "feat", wt.to_str().unwrap()],
    );

    // Move the main worktree — the linked worktree's .git link now dangles.
    let main2 = base.path().join("main-moved");
    std::fs::rename(&main1, &main2).unwrap();
    assert!(
        Repository::open(&wt).is_err(),
        "the linked worktree must be broken after the main moved"
    );

    let repo = Repository::open(&main2).expect("open moved main");
    let plan = plan_repair_worktrees(&repo).expect("plan");
    assert!(plan.blockers.is_empty());
    execute_repair_worktrees(&repo, &plan).expect("execute repair");

    let linked = Repository::open(&wt).expect("linked worktree must resolve after repair");
    assert_eq!(linked.head().unwrap().shorthand().ok(), Some("feat"));
}

/// The containment-checked delete (issue #340) refuses to delete the main
/// worktree even via the branch-delete path — proving the branch.rs:756
/// integration closed the unbounded delete. A branch checked out in the main
/// worktree is HEAD, so delete-branch blocks anyway; here we assert the
/// remove path never touches the repo root.
#[test]
fn remove_never_deletes_repo_root() {
    let tmp = TempDir::new().unwrap();
    let repo = build_repo(&tmp);
    // "main" resolves to no admin entry → refused before any fs touch.
    let plan = plan_remove_worktree(&repo, "main", true).expect("plan");
    let _ = execute_remove_worktree(&repo, &plan, "main", true);
    assert!(
        tmp.path().join("README.md").exists(),
        "the repository root must never be deleted"
    );
}

#[test]
fn no_worktreeinclude_leaves_plan_unchanged() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt");

    let plan = plan_create_worktree(&repo, "wt", &path, &at).expect("plan");
    assert!(
        !plan.warnings.iter().any(|w| matches!(
            w,
            PlanNote::Worktree(
                WorktreeNote::IncludeCopy { .. }
                    | WorktreeNote::IncludeSkippedSymlinks { .. }
                    | WorktreeNote::IncludeOverCap { .. }
            )
        )),
        "no .worktreeinclude → no include notes: {:?}",
        plan.warnings
    );
}
