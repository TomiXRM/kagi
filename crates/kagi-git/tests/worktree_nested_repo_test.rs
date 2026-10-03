//! Remove refuses a linked worktree whose ignored content holds a repository
//! (#951): a main checkout moved there, an independent clone. The walk does
//! not follow symlinks, and an ignored directory without `.git` is still
//! removed.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::plan_remove_worktree;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_succeeds, init_repo, write_file};

use std::path::{Path, PathBuf};

use git2::Repository;
use kagi_domain::plan_note::{PlanNote, WorktreeNote};
use tempfile::TempDir;

/// A repository whose `vendor/` is ignored, with linked worktrees `target`
/// (to be removed) and `other` (the tab Remove runs from).
fn repo_with_linked(main: &Path, dirs: &Path) -> (PathBuf, PathBuf) {
    init_repo(main, "main");
    write_file(main, ".gitignore", "vendor/\n");
    git(main, &["add", ".gitignore"]);
    git(main, &["commit", "-qm", "ignore vendor"]);
    let target = dirs.join("target");
    let other = dirs.join("other");
    for (path, branch) in [(&target, "target"), (&other, "other")] {
        git(
            main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                branch,
                path.to_str().unwrap(),
            ],
        );
    }
    (target, other)
}

fn repository_blocker(blockers: &[PlanNote]) -> Option<&str> {
    blockers.iter().find_map(|note| match note {
        PlanNote::Worktree(WorktreeNote::RemoveContainsRepository { path }) => Some(path.as_str()),
        _ => None,
    })
}

/// Paths compare after resolving `/var` → `/private/var` and the like.
fn same_file(path: impl AsRef<Path>) -> PathBuf {
    path.as_ref().canonicalize().unwrap()
}

/// `--separate-git-dir` without `core.worktree`: the common directory cannot
/// say where the main checkout is. Moved into the target's ignored `vendor/`,
/// it would have been deleted with the target; now the plan refuses with the
/// path, and so does the recorded run, and the main checkout survives.
#[test]
fn a_moved_main_checkout_inside_ignored_content_is_refused() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let base = TempDir::new().unwrap();
    let main = base.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let (target, other) = repo_with_linked(&main, base.path());
    let common = base.path().join("common.git");
    git(
        &main,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            common.to_str().unwrap(),
            main.to_str().unwrap(),
        ],
    );
    // No core.worktree (`--unset` fails harmlessly when there is none).
    git_succeeds(&main, &["config", "--unset", "core.worktree"]);
    std::fs::create_dir(target.join("vendor")).unwrap();
    let moved = target.join("vendor/main");
    std::fs::rename(&main, &moved).unwrap();
    git(&other, &["worktree", "repair"]);
    // Without core.worktree the common directory does not name the main
    // checkout (libgit2 guesses its parent directory), so the existing
    // containment checks cannot see the moved checkout.
    let guessed = Repository::open(&common)
        .unwrap()
        .workdir()
        .map(Path::to_path_buf);
    assert_ne!(
        guessed.and_then(|p| p.canonicalize().ok()),
        Some(moved.canonicalize().unwrap())
    );

    let repo = Repository::open(&other).unwrap();
    let plan = plan_remove_worktree(&repo, "target", false).unwrap();
    let blocker = repository_blocker(&plan.blockers).expect("refused");
    assert_eq!(same_file(blocker), same_file(moved.join(".git")));
    assert_eq!(plan.blockers.len(), 1, "{:?}", plan.blockers);

    let recorded = kagi_git::Backend::plan_recorded_remove(&other, "target", false).unwrap();
    let report =
        kagi_git::Backend::run_recorded_remove(&recorded, kagi_git::oplog::Actor::Human, None);
    assert!(matches!(
        report.blocker,
        Some(PlanNote::Worktree(
            WorktreeNote::RemoveContainsRepository { .. }
        ))
    ));
    assert!(
        moved.join(".gitignore").exists(),
        "the main checkout survives"
    );
}

/// An independent clone under ignored content is a repository too.
#[test]
fn an_independent_repository_inside_ignored_content_is_refused() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let base = TempDir::new().unwrap();
    let main = base.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let (target, _) = repo_with_linked(&main, base.path());
    let lib = target.join("vendor/deep/lib");
    std::fs::create_dir_all(&lib).unwrap();
    init_repo(&lib, "main");
    let plan = plan_remove_worktree(&Repository::open(&main).unwrap(), "target", false).unwrap();
    assert_eq!(
        repository_blocker(&plan.blockers).map(same_file),
        Some(same_file(lib.join(".git")))
    );
}

/// A repository that appears after confirmation stops the recorded run before
/// anything is deleted.
#[test]
fn a_repository_that_appears_after_the_plan_stops_the_removal() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let base = TempDir::new().unwrap();
    let main = base.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let (target, _) = repo_with_linked(&main, base.path());
    std::fs::create_dir(target.join("vendor")).unwrap();
    write_file(&target, "vendor/seed", "ignored\n");
    let plan = kagi_git::Backend::plan_recorded_remove(&main, "target", false).unwrap();
    assert!(
        plan.preview.blockers.is_empty(),
        "{:?}",
        plan.preview.blockers
    );
    init_repo(&target.join("vendor"), "main");
    let report = kagi_git::Backend::run_recorded_remove(&plan, kagi_git::oplog::Actor::Human, None);
    assert!(matches!(
        report.blocker,
        Some(PlanNote::Worktree(
            WorktreeNote::RemoveContainsRepository { .. }
        ))
    ));
    assert!(target.join("vendor/seed").exists(), "nothing was deleted");
}

/// The walk does not follow symlinks: a link to a repository is not entered
/// (and the recursive delete does not follow it either), and an ignored
/// directory without `.git` is removed as before.
#[cfg(unix)]
#[test]
fn ignored_content_without_a_repository_is_still_removed() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let base = TempDir::new().unwrap();
    let main = base.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let (target, _) = repo_with_linked(&main, base.path());
    std::fs::create_dir_all(target.join("vendor/build")).unwrap();
    write_file(&target, "vendor/build/out.o", "ignored\n");
    let elsewhere = base.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    init_repo(&elsewhere, "main");
    std::os::unix::fs::symlink(&elsewhere, target.join("vendor/link")).unwrap();
    let plan = kagi_git::Backend::plan_recorded_remove(&main, "target", false).unwrap();
    assert!(
        plan.preview.blockers.is_empty(),
        "{:?}",
        plan.preview.blockers
    );
    let report = kagi_git::Backend::run_recorded_remove(&plan, kagi_git::oplog::Actor::Human, None);
    assert!(report.blocker.is_none(), "{:?}", report.blocker);
    assert!(!target.exists(), "the linked worktree is removed");
    assert!(
        elsewhere.join(".git").exists(),
        "the symlink was not followed"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
