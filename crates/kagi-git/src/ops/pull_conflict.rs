//! What a pull would collide with — the plan-time preview and the execute-time
//! guard, computed once (#625).
//!
//! Invariant (#625 / ADR-0192): plan and execute must both call the shared
//! conflict calculation here; duplicated logic can label Stash & Pull safe,
//! then discover a conflict only while restoring the stash after confirmation.
//!
//! Split out of `ops/pull.rs` (which was already over the 800-line target) on a
//! feature boundary: everything here answers "would this pull run into what the
//! user has locally?", and nothing here mutates the repository.
//!
//! The overlap that matters is *not* commit-to-commit. A fast-forward pull
//! cannot conflict between commits, so [`predict_merge_conflict`] correctly
//! reports nothing for one; what conflicts is the **working-tree content**
//! against the incoming content, which surfaces when the auto-stash is restored
//! after the pull. [`pull_dirty_overlap`] is that calculation, and both the
//! plan (before the user confirms) and execute (as a refusal) read it from
//! here, so the two can never drift apart.

use super::remote_common::resolve_upstream_oid;
use super::*;

/// Dirty paths that the tree change from `old_tree` to `new_tree` also touches.
///
/// Sorted and deduplicated, rendered for display. A rename contributes **both**
/// of its paths on the dirty side, so a pull that renames a file the user is
/// editing under either name is caught.
pub(super) fn pull_dirty_overlap(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<String>, GitError> {
    let status = working_tree_status(repo)?;
    if status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty() {
        return Ok(Vec::new());
    }

    let mut dirty_paths: std::collections::HashSet<PathBuf> =
        status.untracked.iter().cloned().collect();
    for file in status.staged.iter().chain(status.unstaged.iter()) {
        dirty_paths.insert(file.path.clone());
        if let ChangeKind::Renamed { from } = &file.change {
            dirty_paths.insert(from.clone());
        }
    }

    let mut overlapping: Vec<String> = pull_changed_paths_between_trees(repo, old_tree, new_tree)?
        .into_iter()
        .filter(|path| dirty_paths.contains(path))
        .map(|path| path.display().to_string())
        .collect();
    overlapping.sort();
    overlapping.dedup();
    Ok(overlapping)
}

/// Execute-time refusal: the pull must not overwrite what the user is editing.
pub(super) fn ensure_pull_does_not_touch_dirty_paths(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<(), GitError> {
    let overlapping = pull_dirty_overlap(repo, old_tree, new_tree)?;
    if overlapping.is_empty() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "pull would overwrite dirty path(s): {}. Stash or commit those paths, then pull again.",
            overlapping.join(", ")
        )))
    }
}

/// What the auto-stash restore would do to the paths a pull touches (#625).
///
/// Two lists, because "changed on both sides" is not "conflicts": if the
/// upstream edited the first line and the user the last, `git stash pop`
/// merges them cleanly, and warning about it would be a prediction that does
/// not come true. Only [`Self::conflicting`] is asserted.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RestorePrediction {
    /// The three-way content merge was run in memory and **fails**.
    pub conflicting: Vec<String>,
    /// Overlapping, but not decidable ahead of time: binary content, a mode
    /// change, a delete against an edit, a path added on both sides, or a blob
    /// kagi could not read. Reported as *may* conflict.
    pub unpredictable: Vec<String>,
}

/// Plan-time preview (#625): what restoring the auto-stash would do to the
/// paths the pull is about to change in the working tree.
///
/// The comparison is `HEAD` against the **post-pull** content, because that is
/// what the stash is restored on top of:
///
/// | pull shape | post-pull content |
/// |---|---|
/// | fast-forward | the upstream tree |
/// | diverged | the in-memory merge of HEAD and upstream |
///
/// Using the raw upstream tree for a diverged branch was wrong in both
/// directions (#626 review): it reported paths only *local* commits changed —
/// the reverse delta — and it compared the working tree against content the
/// pull never installs, so a local first-line commit, an upstream last-line
/// commit and a further first-line edit here were called a conflict when the
/// real pull and restore finish clean. Diffing HEAD against the post-pull
/// content needs no merge base and is right for both shapes.
///
/// Nothing is written: the diverged case reads the merged **index** git2 builds
/// in memory (`diff_tree_to_index`, stage-0 blobs), never a tree. A plan that
/// wrote a loose object would wake the watcher and reload the confirmation this
/// preview exists to fill (ADR-0192).
///
/// Local knowledge, like [`predict_merge_conflict`] beside it — `plan_pull`
/// does not fetch. The UI closes that freshness gap by fetching before it opens
/// the dirty-pull confirmation (ADR-0192).
///
/// An unresolvable upstream, an unborn HEAD, an upstream equal to HEAD, or a
/// commit-level merge conflict (the pull itself then refuses and nothing lands,
/// so the restore is onto an unchanged tree) all mean "nothing incoming": an
/// empty prediction rather than an error, because a preview that cannot be
/// computed must not fail the plan.
pub(super) fn plan_pull_restore_conflicts(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<RestorePrediction, GitError> {
    let Some(head_oid) = repo.head().ok().and_then(|head| head.target()) else {
        return Ok(RestorePrediction::default());
    };
    let Ok(upstream_oid) = resolve_upstream_oid(repo, branch_name, remote_name) else {
        return Ok(RestorePrediction::default());
    };
    if head_oid == upstream_oid {
        return Ok(RestorePrediction::default());
    }
    let tree_of = |oid: git2::Oid| -> Result<git2::Tree<'_>, GitError> {
        repo.find_commit(oid)
            .and_then(|commit| commit.tree())
            .map_err(|e| {
                GitError::Other(format!(
                    "pull restore preview: cannot read tree of {oid}: {}",
                    e.message()
                ))
            })
    };
    let head_tree = tree_of(head_oid)?;
    let fast_forward = repo
        .graph_descendant_of(upstream_oid, head_oid)
        .unwrap_or(false);
    let incoming = if fast_forward {
        Incoming::Tree(tree_of(upstream_oid)?)
    } else {
        let (Ok(head_commit), Ok(upstream_commit)) =
            (repo.find_commit(head_oid), repo.find_commit(upstream_oid))
        else {
            return Ok(RestorePrediction::default());
        };
        match repo.merge_commits(&head_commit, &upstream_commit, None) {
            // A commit-level conflict means the pull refuses before writing
            // anything, so the stash is restored onto an unchanged HEAD.
            Ok(index) if index.has_conflicts() => return Ok(RestorePrediction::default()),
            Ok(index) => Incoming::Merged(index),
            Err(_) => return Ok(RestorePrediction::default()),
        }
    };

    let dirty = dirty_paths(repo)?;
    if dirty.is_empty() {
        return Ok(RestorePrediction::default());
    }
    let mut overlap: Vec<String> = incoming
        .changed_paths(repo, &head_tree)?
        .into_iter()
        .filter(|path| dirty.contains(path))
        .map(|path| path.display().to_string())
        .collect();
    overlap.sort();
    overlap.dedup();

    let Some(workdir) = repo.workdir().map(|dir| dir.to_path_buf()) else {
        // Bare repository: nothing is restorable, so nothing is predictable.
        return Ok(RestorePrediction {
            conflicting: Vec::new(),
            unpredictable: overlap,
        });
    };

    let mut prediction = RestorePrediction::default();
    for path in overlap {
        match restore_verdict(repo, &head_tree, &incoming, &workdir, &path) {
            Verdict::Clean => {}
            Verdict::Conflicts => prediction.conflicting.push(path),
            Verdict::Unknown => prediction.unpredictable.push(path),
        }
    }
    Ok(prediction)
}

/// The content the pull will leave in the working tree.
enum Incoming<'repo> {
    /// Fast-forward: exactly the upstream tree.
    Tree(git2::Tree<'repo>),
    /// Diverged: the merge git2 built in memory. Never written out.
    Merged(git2::Index),
}

impl Incoming<'_> {
    /// Paths this pull changes relative to `head_tree`.
    fn changed_paths(
        &self,
        repo: &Repository,
        head_tree: &git2::Tree<'_>,
    ) -> Result<Vec<PathBuf>, GitError> {
        let diff = match self {
            Incoming::Tree(tree) => repo.diff_tree_to_tree(Some(head_tree), Some(tree), None),
            Incoming::Merged(index) => repo.diff_tree_to_index(Some(head_tree), Some(index), None),
        }
        .map_err(|e| {
            GitError::Other(format!(
                "pull restore preview: diff failed: {}",
                e.message()
            ))
        })?;
        let mut paths = Vec::new();
        for delta in diff.deltas() {
            if let Some(path) = delta.old_file().path() {
                paths.push(path.to_path_buf());
            }
            if let Some(path) = delta.new_file().path() {
                paths.push(path.to_path_buf());
            }
        }
        Ok(paths)
    }

    /// `(content, filemode)` this pull will install at `path`, if it is a
    /// mergeable text blob.
    fn blob_at(&self, repo: &Repository, path: &str) -> Option<(Vec<u8>, i32)> {
        let (oid, mode) = match self {
            Incoming::Tree(tree) => {
                let entry = tree.get_path(Path::new(path)).ok()?;
                (entry.id(), entry.filemode())
            }
            Incoming::Merged(index) => {
                let entry = index.get_path(Path::new(path), 0)?;
                (entry.id, entry.mode as i32)
            }
        };
        let blob = repo.find_blob(oid).ok()?;
        (!blob.is_binary()).then(|| (blob.content().to_vec(), mode))
    }
}

/// Every path the working tree has changed, renames counted under both names.
fn dirty_paths(repo: &Repository) -> Result<std::collections::HashSet<PathBuf>, GitError> {
    let status = working_tree_status(repo)?;
    let mut paths: std::collections::HashSet<PathBuf> = status.untracked.iter().cloned().collect();
    for file in status.staged.iter().chain(status.unstaged.iter()) {
        paths.insert(file.path.clone());
        if let ChangeKind::Renamed { from } = &file.change {
            paths.insert(from.clone());
        }
    }
    Ok(paths)
}

enum Verdict {
    /// The three-way merge succeeds: the restore will be silent.
    Clean,
    /// The three-way merge produces conflict markers.
    Conflicts,
    /// Not decidable without doing the restore.
    Unknown,
}

/// Predict one path's restore by running the same three-way content merge
/// `git stash pop` would.
///
/// The sides are the ones the restore actually merges: **ancestor** is the blob
/// at HEAD (what the user's edit was made against), **ours** is the content the
/// pull is about to put in the working tree ([`Incoming`] — the upstream blob
/// on a fast-forward, the merged one when the branch diverged), and **theirs**
/// is the bytes currently on disk.
///
/// [`git2::merge_file`] does this entirely in memory: it writes no loose
/// object and touches no ref, which a plan must not do — a written object
/// would wake the FS watcher and reload the confirmation this preview exists
/// to fill (ADR-0192).
fn restore_verdict(
    repo: &Repository,
    head_tree: &git2::Tree<'_>,
    incoming: &Incoming<'_>,
    workdir: &Path,
    path: &str,
) -> Verdict {
    let head_blob = |tree: &git2::Tree<'_>| -> Option<(Vec<u8>, i32)> {
        let entry = tree.get_path(Path::new(path)).ok()?;
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return None;
        }
        let blob = repo.find_blob(entry.id()).ok()?;
        if blob.is_binary() {
            return None;
        }
        Some((blob.content().to_vec(), entry.filemode()))
    };
    // A path missing on either side is an add/add or a delete against an edit:
    // `git stash pop` refuses those instead of merging content, so they are
    // reported as possible rather than asserted.
    let (Some((ancestor, head_mode)), Some((ours, incoming_mode))) =
        (head_blob(head_tree), incoming.blob_at(repo, path))
    else {
        return Verdict::Unknown;
    };
    if head_mode != incoming_mode {
        return Verdict::Unknown;
    }
    // The local side changes mode and type too, and a content merge says
    // nothing about either: a `chmod +x` here against an upstream text edit is
    // not something this can call clean, and a path that became a symlink or a
    // directory is not a content merge at all.
    let file = workdir.join(path);
    let Ok(meta) = std::fs::symlink_metadata(&file) else {
        return Verdict::Unknown;
    };
    if !meta.file_type().is_file() {
        return Verdict::Unknown;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let local_executable = meta.permissions().mode() & 0o111 != 0;
        let head_executable = head_mode & 0o111 != 0;
        if local_executable != head_executable {
            return Verdict::Unknown;
        }
    }
    let Ok(theirs) = std::fs::read(&file) else {
        return Verdict::Unknown;
    };
    if theirs.contains(&0) {
        return Verdict::Unknown;
    }
    fn side<'a>(content: &'a [u8], path: &Path) -> git2::MergeFileInput<'a> {
        let mut input = git2::MergeFileInput::new();
        input.content(content).path(path);
        input
    }
    let path_arg = Path::new(path);
    match git2::merge_file(
        &side(&ancestor, path_arg),
        &side(&ours, path_arg),
        &side(&theirs, path_arg),
        None,
    ) {
        Ok(result) if result.is_automergeable() => Verdict::Clean,
        Ok(_) => Verdict::Conflicts,
        Err(_) => Verdict::Unknown,
    }
}

/// Every path either side of each delta between two trees.
pub(super) fn pull_changed_paths_between_trees(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<PathBuf>, GitError> {
    let diff = repo
        .diff_tree_to_tree(Some(old_tree), Some(new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for pull safety failed: {}",
                e.message()
            ))
        })?;

    let mut paths = Vec::new();
    for delta in diff.deltas() {
        if let Some(path) = delta.old_file().path() {
            paths.push(path.to_path_buf());
        }
        if let Some(path) = delta.new_file().path() {
            paths.push(path.to_path_buf());
        }
    }
    Ok(paths)
}

/// Attempt an in-memory merge with the current upstream tip to predict conflicts.
///
/// Returns `Ok(true)` if a conflict is predicted, `Ok(false)` if the merge
/// would be clean (or fast-forward), or `Err(...)` if the prediction itself
/// failed (non-fatal — caller ignores and treats as no warning).
pub(super) fn predict_merge_conflict(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<bool, GitError> {
    let head_oid = repo.head().ok().and_then(|r| r.target());
    let upstream_oid = resolve_upstream_oid(repo, branch_name, remote_name).ok();

    let (head_oid, upstream_oid) = match (head_oid, upstream_oid) {
        (Some(h), Some(u)) => (h, u),
        _ => return Ok(false),
    };

    // If already fast-forward or up-to-date, no conflict possible.
    if head_oid == upstream_oid {
        return Ok(false);
    }
    if repo
        .graph_descendant_of(head_oid, upstream_oid)
        .unwrap_or(false)
        || repo
            .graph_descendant_of(upstream_oid, head_oid)
            .unwrap_or(false)
    {
        return Ok(false);
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let upstream_commit = repo
        .find_commit(upstream_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let index = repo
        .merge_commits(&head_commit, &upstream_commit, None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    Ok(index.has_conflicts())
}
