//! What a pull would collide with — plan-time preview and execute-time guard
//! with one definition of locally dirty paths (#625 / ADR-0192).
//!
//! Invariant: [`dirty_paths`] is the only definition of local dirt. Preview and
//! execute both compare it with HEAD → post-pull content, including directory /
//! file collisions. Restore content conflicts are separate from direct-pull
//! overwrite protection and from committed-history merge conflicts.
//!
//! Direct checkout refuses overlapping local paths even when a three-way stash
//! restore could merge them. Only those paths require explicit local-change
//! protection; unrelated edits and staged blobs stay in place.

use super::remote_common::resolve_configured_upstream_oid;
use super::*;

// Use path components, not string prefixes: `dir` collides with `dir/file`,
// but `dir` and `directory` are unrelated. Return the local paths being saved.
fn overlapping_dirty_paths(
    dirty: &std::collections::HashSet<PathBuf>,
    incoming: &[PathBuf],
) -> Vec<String> {
    let mut overlapping: Vec<String> = dirty
        .iter()
        .filter(|local| {
            incoming
                .iter()
                .any(|path| local.starts_with(path) || path.starts_with(local))
        })
        .map(|path| path.display().to_string())
        .collect();
    overlapping.sort();
    overlapping.dedup();
    overlapping
}

/// Update only the actual incoming delta. A full-tree checkout can replace
/// unrelated staged entries with HEAD blobs; restricting literal paths retains
/// their path/OID/mode, including staged additions and deletions.
pub(super) fn checkout_pull_tree(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<(), GitError> {
    let paths = pull_changed_paths_between_trees(repo, old_tree, new_tree)?;
    // An empty pathspec means "all paths", not "nothing".
    if paths.is_empty() {
        return Ok(());
    }
    let overlapping = overlapping_dirty_paths(&dirty_paths(repo)?, &paths);
    if !overlapping.is_empty() {
        return Err(GitError::Other(format!(
            "pull would overwrite dirty path(s): {}. Stash or commit those paths, then pull again.",
            overlapping.join(", ")
        )));
    }
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.safe().disable_pathspec_match(true);
    for path in &paths {
        // Unix checkout accepts the original bytes. Other platforms must not
        // substitute a lossy display path (CheckoutBuilder would panic).
        #[cfg(not(unix))]
        if path.to_str().is_none() {
            return Err(GitError::Other(
                "Pull cannot safely express an incoming checkout path on this platform".into(),
            ));
        }
        checkout.path(path.as_path());
    }
    repo.checkout_tree(new_tree.as_object(), Some(&mut checkout))
        .map_err(|error| {
            GitError::Other(format!("checkout_tree (pull) failed: {}", error.message()))
        })
}

/// Direct-pull protection and explicit-stash restore advice are separate:
/// overlap requires protecting local work even if its content could merge.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RestorePrediction {
    /// A committed-history conflict. Stashing cannot resolve this.
    pub merge_conflict: bool,
    /// Direct checkout would touch these local paths, even if stash restore
    /// could merge their content cleanly.
    pub overlapping: Vec<String>,
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
/// This is local knowledge; the caller fetches before planning. Failed object
/// reads fail the plan closed. A history conflict is reported independently
/// from local overwrite and restore-content risks.
pub(super) fn plan_pull_restore_conflicts(
    repo: &Repository,
    branch_name: &str,
) -> Result<RestorePrediction, GitError> {
    let head_oid = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(|error| GitError::Other(error.to_string()))?
        .id();
    let upstream_oid = resolve_configured_upstream_oid(repo, branch_name)?;
    if head_oid == upstream_oid
        || repo
            .graph_descendant_of(head_oid, upstream_oid)
            .map_err(|error| GitError::Other(error.to_string()))?
    {
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
        .map_err(|error| GitError::Other(error.to_string()))?;
    let incoming = if fast_forward {
        Incoming::Tree(tree_of(upstream_oid)?)
    } else {
        let head_commit = repo
            .find_commit(head_oid)
            .map_err(|error| GitError::Other(error.to_string()))?;
        let upstream_commit = repo
            .find_commit(upstream_oid)
            .map_err(|error| GitError::Other(error.to_string()))?;
        match repo.merge_commits(&head_commit, &upstream_commit, None) {
            Ok(index) if index.has_conflicts() => {
                return Ok(RestorePrediction {
                    merge_conflict: true,
                    ..RestorePrediction::default()
                });
            }
            Ok(index) => Incoming::Merged(index),
            Err(error) => return Err(GitError::Other(error.to_string())),
        }
    };

    let dirty = dirty_paths(repo)?;
    if dirty.is_empty() {
        return Ok(RestorePrediction::default());
    }
    let overlap = overlapping_dirty_paths(&dirty, &incoming.changed_paths(repo, &head_tree)?);

    let Some(workdir) = repo.workdir().map(|dir| dir.to_path_buf()) else {
        // Bare repository: nothing is restorable, so nothing is predictable.
        return Ok(RestorePrediction {
            overlapping: overlap.clone(),
            unpredictable: overlap,
            ..RestorePrediction::default()
        });
    };

    let mut prediction = RestorePrediction {
        overlapping: overlap,
        ..RestorePrediction::default()
    };
    for path in &prediction.overlapping {
        match restore_verdict(repo, &head_tree, &incoming, &workdir, path) {
            Verdict::Clean => {}
            Verdict::Conflicts => prediction.conflicting.push(path.clone()),
            Verdict::Unknown => prediction.unpredictable.push(path.clone()),
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
