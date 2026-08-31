//! Read-only preview of the conflicts a PR would produce (ADR-0145).
//!
//! GitHub reports `mergeable: CONFLICTING` and nothing more — not which files,
//! not what the conflict looks like. That is enough to warn with and useless
//! for deciding what to do about it, which is the gap this closes.
//!
//! The merge is computed in memory: no ref moves, no index or working-tree
//! file is touched, and no merge state is entered. It answers "what would
//! happen", not "start dealing with it".
//!
//! One exception, stated because the module would otherwise be claiming more
//! than it does: the both-added case writes an **unreferenced empty blob** to
//! the object database. `merge_file_from_index` needs a readable ancestor and
//! there is none, and a repository in which no file has ever been empty does
//! not contain `e69de29…` to point at. Nothing references the blob and
//! `git gc` collects it; it is invisible to every command a user runs.

use super::*;

/// One conflicting file in a would-be merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrConflictFile {
    pub path: PathBuf,
    /// The merged text with conflict markers, exactly as git would leave it in
    /// the working tree. Parse with `kagi_domain::resolution::HunkModel`.
    pub marker_text: String,
    /// Set when a side deleted the file: there is no text to show, only the
    /// fact of it. (`None` for an ordinary content conflict.)
    pub kind: PrConflictKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrConflictKind {
    /// Both sides changed the contents.
    BothModified,
    /// One side deleted the file while the other changed it.
    DeleteModify,
    /// Both sides added a file at the same path.
    BothAdded,
    /// A binary file changed on both sides. There is no text to show, and
    /// asking libgit2 for merged content would be worse than useless: it
    /// produces none, and `git2` reads the resulting null pointer as a slice —
    /// a hard abort that takes the process down, not a catchable panic.
    Binary,
}

/// Compute the conflicts merging `head` into `base` would produce.
///
/// `Ok(vec![])` means the merge is clean — which is a real answer, not an
/// error: GitHub's `mergeable` can be stale, and this is computed from the
/// objects actually present.
pub fn pr_conflict_preview(
    repo: &Repository,
    base: &CommitId,
    head: &CommitId,
) -> Result<Vec<PrConflictFile>, GitError> {
    let oid = |c: &CommitId| {
        git2::Oid::from_str(&c.0)
            .map_err(|e| GitError::Other(format!("commit id parse failed: {}", e.message())))
    };
    let base_commit = repo
        .find_commit(oid(base)?)
        .map_err(|e| GitError::Other(format!("base commit lookup failed: {}", e.message())))?;
    let head_commit = repo
        .find_commit(oid(head)?)
        .map_err(|e| GitError::Other(format!("head commit lookup failed: {}", e.message())))?;

    let index = repo
        .merge_commits(&base_commit, &head_commit, None)
        .map_err(|e| GitError::Other(format!("merge_commits in-memory failed: {}", e.message())))?;
    if !index.has_conflicts() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts failed: {}", e.message())))?;
    for c in conflicts.flatten() {
        // `ours` is the base side (the PR's target), `theirs` the PR's head —
        // `merge_commits(base, head)` fixes that order.
        let path_of = |e: &Option<git2::IndexEntry>| {
            e.as_ref()
                .and_then(|e| std::str::from_utf8(&e.path).ok())
                .map(PathBuf::from)
        };
        let Some(path) = path_of(&c.our).or_else(|| path_of(&c.their)).or_else(|| {
            c.ancestor
                .as_ref()
                .and_then(|e| std::str::from_utf8(&e.path).ok())
                .map(PathBuf::from)
        }) else {
            continue;
        };

        let (kind, marker_text) = match (&c.ancestor, &c.our, &c.their) {
            // Binary first: the text path below must never see one.
            (_, Some(our), Some(their)) if is_binary(repo, our.id) || is_binary(repo, their.id) => {
                (PrConflictKind::Binary, String::new())
            }
            (ancestor, Some(our), Some(their)) => {
                // An ordinary content conflict: ask libgit2 for exactly the
                // text it would have written into the working tree.
                //
                // Both-added has no ancestor, and `IndexEntry` is not `Clone`,
                // so synthesise one at the empty blob — which is what "did not
                // exist on either side" means to a three-way merge.
                let synthetic;
                let ancestor = match ancestor {
                    Some(a) => a,
                    None => {
                        synthetic = empty_ancestor_like(repo, our)?;
                        &synthetic
                    }
                };
                let mut opts = git2::MergeFileOptions::new();
                opts.our_label("base").their_label("PR");
                let text = repo
                    .merge_file_from_index(ancestor, our, their, Some(&mut opts))
                    .ok()
                    .map(|r| String::from_utf8_lossy(r.content()).into_owned())
                    .unwrap_or_default();
                let kind = if c.ancestor.is_none() {
                    PrConflictKind::BothAdded
                } else {
                    PrConflictKind::BothModified
                };
                (kind, text)
            }
            // One side is gone: there is no three-way text to render.
            _ => (PrConflictKind::DeleteModify, String::new()),
        };

        out.push(PrConflictFile {
            path,
            marker_text,
            kind,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    Ok(out)
}

/// An index entry for `path` pointing at the empty blob — the stand-in for a
/// file that existed on neither side.
///
/// The blob is **written**, not just named: a repository where nothing is empty
/// does not contain `e69de29…`, and `merge_file_from_index` fails with "object
/// not found" on an id it cannot read. Writing it is a no-op if it is already
/// there, and adds an unreferenced empty blob if it is not.
fn empty_ancestor_like(
    repo: &Repository,
    like: &git2::IndexEntry,
) -> Result<git2::IndexEntry, GitError> {
    let empty = repo
        .blob(b"")
        .map_err(|e| GitError::Other(format!("empty blob write failed: {}", e.message())))?;
    Ok(git2::IndexEntry {
        id: empty,
        mode: like.mode,
        path: like.path.clone(),
        ctime: git2::IndexTime::new(0, 0),
        mtime: git2::IndexTime::new(0, 0),
        dev: 0,
        ino: 0,
        uid: 0,
        gid: 0,
        file_size: 0,
        flags: 0,
        flags_extended: 0,
    })
}

/// Whether `oid` is a blob git would call binary.
///
/// Guards `merge_file_from_index`: libgit2 returns no merged buffer for a
/// binary conflict, and `MergeFileResult::content()` passes that null pointer
/// to `slice::from_raw_parts`, which aborts the process. Checking the inputs
/// is the only way to avoid it — the result cannot be inspected without
/// calling the accessor that does the aborting.
fn is_binary(repo: &Repository, oid: git2::Oid) -> bool {
    repo.find_blob(oid).map(|b| b.is_binary()).unwrap_or(false)
}
