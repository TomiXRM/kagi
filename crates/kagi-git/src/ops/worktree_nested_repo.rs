//! A repository inside the linked worktree being removed (#951).
//!
//! Removing a linked worktree deletes its directory recursively (#934). A
//! `.git` below its root marks someone's repository: a main checkout moved
//! there (with `--separate-git-dir` and no `core.worktree`, the common
//! directory cannot prove where the main checkout is), an independent clone,
//! or a submodule's checkout. Removal refuses any of them rather than
//! deleting a repository silently.
//!
//! The whole directory is walked, tracked, untracked and ignored alike: Git's
//! status cannot be the starting point, as a repository inside a directory
//! that also holds a tracked file is reported by neither its directory nor
//! its `.git` (#958 review). The recursive delete walks the same tree, so
//! this does not raise its cost's bound. Only the root's own `.git` (the
//! linked worktree's) is not a find. The walk never follows a symlink (a
//! symlink named `.git` counts as found; a symlinked directory is not
//! entered) and stops at the first `.git` it finds. The name is matched
//! without regard to ASCII case on every OS: on a case-insensitive filesystem
//! (macOS, Windows) Git opens `.GIT` as `.git`; elsewhere refusing it as well
//! errs on the safe side.
use super::*;
use kagi_domain::plan_note::WorktreeNote;
use std::path::{Path, PathBuf};

/// The first `.git` — file, directory or symlink — below the linked
/// worktree's root. Unreadable directories fail closed.
pub(crate) fn repository_in_worktree(wt: &git2::Worktree) -> Result<Option<PathBuf>, GitError> {
    first_nested_dot_git(wt.path())
        .map_err(|e| GitError::Other(format!("cannot inspect worktree content: {e}")))
}

fn is_dot_git(name: &std::ffi::OsStr) -> bool {
    name.eq_ignore_ascii_case(".git")
}

/// Depth-first, without following symlinks; the first `.git` entry (in any
/// ASCII case) below `root` wins. `root`'s own `.git` is skipped.
fn first_nested_dot_git(root: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut pending = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        // DirEntry::file_type does not follow a symlink.
        if !is_dot_git(&entry.file_name()) && entry.file_type()?.is_dir() {
            pending.push(entry.path());
        }
    }
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if is_dot_git(&entry.file_name()) {
                return Ok(Some(entry.path()));
            }
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(None)
}

/// The plan's blocker, when the linked worktree holds a repository.
pub(crate) fn nested_repository_blocker(wt: &git2::Worktree) -> Result<Option<PlanNote>, GitError> {
    Ok(repository_in_worktree(wt)?.map(|path| {
        PlanNote::Worktree(WorktreeNote::RemoveContainsRepository {
            path: path.display().to_string(),
        })
    }))
}

/// Re-check before hooks and immediately before deletion: a repository may
/// have appeared in the target since the plan.
pub(crate) fn preflight_remove_nested_repositories(
    repo: &Repository,
    name: &str,
) -> Result<(), GitError> {
    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("cannot inspect removal target: {e}")))?;
    if let Some(blocker) = nested_repository_blocker(&wt)? {
        return Err(GitError::Blocked(Box::new(blocker)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@e")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@e")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// A trusted pre-remove command can create a repository inside ignored
    /// content after every earlier check passed. The check right before the
    /// recursive delete stops it, and the repository survives.
    #[test]
    fn pre_remove_repository_survives_final_deletion_guard() {
        if std::env::var_os("KAGI_REMOVE_NESTED_REPO_UNIT_CHILD").is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "ops::worktree_nested_repo::tests::pre_remove_repository_survives_final_deletion_guard",
                    "--nocapture",
                ])
                .env("KAGI_REMOVE_NESTED_REPO_UNIT_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        use crate::ops::{
            execute_remove_worktree, load_worktree_config, plan_remove_worktree,
            trust_worktree_config,
        };
        let store = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", store.path());
        for key in ["KAGI_OPEN_REPO", "KAGI_MENU_DUMP", "KAGI_SELECT_FIRST"] {
            std::env::remove_var(key);
        }
        let td = tempfile::tempdir().unwrap();
        let main = td.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join(".gitignore"), "vendor/\n").unwrap();
        std::fs::create_dir(main.join(".kagi")).unwrap();
        std::fs::write(
            main.join(".kagi/worktree.toml"),
            "[[pre_remove]]\ntype = \"command\"\nrun = \"git init -q vendor/lib\"\n",
        )
        .unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "base"]);
        let target = td.path().join("target");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "target",
                target.to_str().unwrap(),
            ],
        );
        std::fs::create_dir(target.join("vendor")).unwrap();
        std::fs::write(target.join("vendor/seed"), "ignored seed\n").unwrap();
        let cfg = load_worktree_config(&target).unwrap().unwrap();
        trust_worktree_config(&cfg).unwrap();
        let repo = Repository::open(&main).unwrap();
        let plan = plan_remove_worktree(&repo, "target", false).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        let error = execute_remove_worktree(&repo, &plan, "target", false)
            .expect_err("a repository created by pre_remove must stop deletion");
        assert!(
            matches!(
                &error,
                GitError::Blocked(note)
                    if matches!(
                        **note,
                        PlanNote::Worktree(WorktreeNote::RemoveContainsRepository { .. })
                    )
            ),
            "{error:?}"
        );
        assert!(
            target.join("vendor/lib/.git").exists(),
            "the repository survives"
        );
        std::env::remove_var("KAGI_LOG_DIR");
    }
}
