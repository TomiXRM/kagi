//! Path normalization + symlink-safe containment shared by the worktree ops.
//!
//! These are the single source of truth for "does this path stay inside the
//! repo / worktree boundary". `validate_worktree_path_keyed` (user-entered
//! worktree paths), the `.kagi/worktree.toml` copy/symlink steps (issue #392),
//! and the `.worktreeinclude` copy (issue #419) all route through the same
//! checks, so containment is implemented once. Mirrors the symlink-safety of
//! `remove_worktree_dir_checked`.

use std::path::{Component, Path, PathBuf};

use super::GitError;
use git2::Repository;

/// Marker file kagi drops into a linked worktree's admin dir
/// (`$GIT_DIR/worktrees/<name>/`) at creation time, so bulk operations (prune)
/// can tell a kagi-created worktree from one added by hand (`git worktree add`).
/// Writing into git's own admin namespace is deliberate and precedented — it is
/// kagi's private metadata dir (issue #372 item 1, §5 open question resolved).
const KAGI_CREATED_MARKER: &str = ".kagi-created";

/// The admin directory git keeps for the linked worktree `name`
/// (`$GIT_DIR/worktrees/<name>/`). `commondir()` always points at the main
/// `.git`, so this is correct whether `repo` is the main repo or a linked one.
fn worktree_admin_dir(repo: &Repository, name: &str) -> PathBuf {
    repo.commondir().join("worktrees").join(name)
}

/// Drop the `.kagi-created` marker into `name`'s admin dir. Best-effort: the
/// worktree already exists when this runs, so a write hiccup must never undo it
/// (an unmarked worktree is simply treated as hand-added — the safe default).
pub(crate) fn mark_kagi_created(repo: &Repository, name: &str) {
    let _ = std::fs::write(
        worktree_admin_dir(repo, name).join(KAGI_CREATED_MARKER),
        b"",
    );
}

/// Whether the linked worktree `name` carries kagi's creation marker. The
/// admin dir survives even when the working directory is gone (the prunable
/// case), so this is readable exactly when scoping bulk prune needs it.
pub(crate) fn is_kagi_created(repo: &Repository, name: &str) -> bool {
    worktree_admin_dir(repo, name)
        .join(KAGI_CREATED_MARKER)
        .exists()
}

/// Lexically normalize a path without requiring the final path to exist.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// Canonicalize the longest existing prefix of `path` (resolving symlinks) and
/// re-append the components that don't exist yet. Lets worktree containment be
/// checked even when the target's parent directory hasn't been created.
pub(crate) fn canonicalize_nearest_existing(path: &Path) -> std::io::Result<PathBuf> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path;
    loop {
        match std::fs::canonicalize(cur) {
            Ok(mut real) => {
                for part in tail.iter().rev() {
                    real.push(part);
                }
                return Ok(real);
            }
            Err(e) => match (cur.file_name(), cur.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name.to_os_string());
                    cur = parent;
                }
                _ => return Err(e),
            },
        }
    }
}

/// Reject a config-supplied relative path before it is ever joined: absolute
/// paths and `..` / root / drive-prefix components. Purely lexical and cheap —
/// [`resolve_contained`] is the real (symlink-aware) guard, but this refuses the
/// obvious escapes up front with a precise message. Shared by the worktree
/// copy/symlink steps and `.worktreeinclude` copy (issues #392 / #419).
pub(crate) fn reject_escaping_relative(kind: &str, rel: &str) -> Result<(), GitError> {
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(GitError::Other(format!(
            "{kind} '{rel}' must be a path inside the repository, not an absolute path"
        )));
    }
    if p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(GitError::Other(format!(
            "{kind} '{rel}' must not contain '..' or a filesystem root/prefix"
        )));
    }
    Ok(())
}

/// Resolve `candidate` symlink-safely (longest existing prefix canonicalized,
/// non-existent tail re-appended) and confirm the real location stays inside
/// `boundary` (which must already be canonical). Refuses any path that escapes
/// the tree — via an absolute join, a `..`, or a symlinked parent — and never
/// follows a symlink out of it. Creates nothing. The shared containment guard
/// for worktree copy/symlink steps and `.worktreeinclude` (issues #392 / #419).
pub(crate) fn resolve_contained(boundary: &Path, candidate: &Path) -> Result<PathBuf, GitError> {
    let real = canonicalize_nearest_existing(candidate)
        .map_err(|e| GitError::Other(format!("cannot resolve '{}': {e}", candidate.display())))?;
    if real.starts_with(boundary) {
        Ok(real)
    } else {
        Err(GitError::Other(format!(
            "refusing '{}': it resolves to '{}', outside the boundary '{}'",
            candidate.display(),
            real.display(),
            boundary.display()
        )))
    }
}
