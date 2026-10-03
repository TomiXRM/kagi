//! A repository inside a linked worktree's ignored content (#951).
//!
//! Removing a linked worktree deletes its directory recursively, ignored
//! content included (#934). A `.git` below it marks someone's repository:
//! a main checkout moved there (with `--separate-git-dir` and no
//! `core.worktree`, the common directory cannot prove where the main checkout
//! is), an independent clone, or a submodule's checkout. Removal refuses any
//! of them rather than deleting a repository silently.
//!
//! Only ignored directories are walked: anything else that could hold a
//! repository (untracked content, gitlinks) already blocks the removal as dirt
//! or as submodule content. The walk never follows a symlink (a symlink named
//! `.git` counts as found; a symlinked directory is not entered) and stops at
//! the first `.git` it finds.
use super::*;
use kagi_domain::plan_note::WorktreeNote;
use std::path::{Path, PathBuf};

/// The first `.git` — file, directory or symlink — inside one of the linked
/// worktree's ignored directories. Unreadable directories fail closed.
pub(crate) fn repository_in_ignored_content(
    wt: &git2::Worktree,
) -> Result<Option<PathBuf>, GitError> {
    let fail = |e: &dyn std::fmt::Display| {
        GitError::Other(format!("cannot inspect ignored worktree content: {e}"))
    };
    let wt_repo = Repository::open_from_worktree(wt).map_err(|e| fail(&e))?;
    let mut opts = git2::StatusOptions::new();
    opts.include_ignored(true)
        .recurse_ignored_dirs(false)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true);
    let statuses = wt_repo.statuses(Some(&mut opts)).map_err(|e| fail(&e))?;
    for entry in statuses.iter().filter(|entry| entry.status().is_ignored()) {
        let relative = entry.path_bytes();
        // Git reports an ignored directory as one entry ending in '/'.
        let Some(relative) = relative.strip_suffix(b"/") else {
            continue;
        };
        let dir = wt
            .path()
            .join(relative_path(relative).map_err(|e| fail(&e))?);
        if let Some(found) = first_dot_git(&dir).map_err(|e| fail(&e))? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn relative_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn relative_path(bytes: &[u8]) -> Result<PathBuf, String> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| "a path that is not UTF-8".to_string())
}

/// Depth-first, without following symlinks; the first `.git` entry wins.
fn first_dot_git(root: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                return Ok(Some(entry.path()));
            }
            // DirEntry::file_type does not follow a symlink.
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(None)
}

/// The plan's blocker, when the linked worktree holds a repository.
pub(crate) fn nested_repository_blocker(wt: &git2::Worktree) -> Result<Option<PlanNote>, GitError> {
    Ok(repository_in_ignored_content(wt)?.map(|path| {
        PlanNote::Worktree(WorktreeNote::RemoveContainsRepository {
            path: path.display().to_string(),
        })
    }))
}

/// Re-check before hooks and immediately before deletion: a repository may
/// have appeared under ignored content since the plan.
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
