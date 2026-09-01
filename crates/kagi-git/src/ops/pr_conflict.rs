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

/// One conflicting file in a would-be merge — **without** its text.
///
/// The list and the text are separate calls on purpose. On a real PR this was
/// 537 conflicting files whose marker text came to 50 MB, of which the user
/// reads one file: generating all of it up front cost 3.4 s of the 5.8 s wait
/// and held the other 49 MB for nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrConflictFile {
    pub path: PathBuf,
    pub kind: PrConflictKind,
}

/// How big a single file's marker text may get before it is refused.
///
/// The largest file in that same PR was 3.5 MB of markers. Turning that into
/// diff rows is a long freeze and an unreadable wall; the tab exists to answer
/// "what clashes", and past this size the honest answer is "more than this
/// view can show you".
pub const MAX_MARKER_BYTES: usize = 512 * 1024;

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

/// List the files merging `head` into `base` would conflict on — paths and
/// kinds only, no text.
///
/// `Ok(vec![])` means the merge is clean, which is a real answer rather than an
/// error: GitHub's `mergeable` can be stale, and this is computed from the
/// objects actually present.
pub fn pr_conflict_files(
    repo: &Repository,
    base: &CommitId,
    head: &CommitId,
) -> Result<Vec<PrConflictFile>, GitError> {
    let index = merged_index(repo, base, head)?;
    if !index.has_conflicts() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for c in index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts failed: {}", e.message())))?
        .flatten()
    {
        let Some(path) = conflict_path(&c) else {
            continue;
        };
        let kind = match (&c.ancestor, &c.our, &c.their) {
            (_, Some(our), Some(their)) if is_binary(repo, our.id) || is_binary(repo, their.id) => {
                PrConflictKind::Binary
            }
            (None, Some(_), Some(_)) => PrConflictKind::BothAdded,
            (Some(_), Some(_), Some(_)) => PrConflictKind::BothModified,
            _ => PrConflictKind::DeleteModify,
        };
        out.push(PrConflictFile { path, kind });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    Ok(out)
}

/// The conflict markers for **one** file, as git would have written them.
///
/// `Ok(None)` when there is nothing to show: a deleted side, a binary file, or
/// a conflict larger than [`MAX_MARKER_BYTES`].
pub fn pr_conflict_text(
    repo: &Repository,
    base: &CommitId,
    head: &CommitId,
    path: &Path,
) -> Result<Option<String>, GitError> {
    let index = merged_index(repo, base, head)?;
    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts failed: {}", e.message())))?;
    for c in conflicts.flatten() {
        if conflict_path(&c).as_deref() != Some(path) {
            continue;
        }
        let (Some(our), Some(their)) = (&c.our, &c.their) else {
            return Ok(None);
        };
        // Never for a binary: libgit2 produces no buffer and `git2` reads the
        // resulting null pointer as a slice, which aborts the process.
        if is_binary(repo, our.id) || is_binary(repo, their.id) {
            return Ok(None);
        }
        let synthetic;
        let ancestor = match &c.ancestor {
            Some(a) => a,
            None => {
                synthetic = empty_ancestor_like(repo, our)?;
                &synthetic
            }
        };
        let mut opts = git2::MergeFileOptions::new();
        opts.our_label("base").their_label("PR");
        let Ok(result) = repo.merge_file_from_index(ancestor, our, their, Some(&mut opts)) else {
            return Ok(None);
        };
        let bytes = result.content();
        if bytes.len() > MAX_MARKER_BYTES {
            return Ok(None);
        }
        return Ok(Some(String::from_utf8_lossy(bytes).into_owned()));
    }
    Ok(None)
}

fn merged_index(
    repo: &Repository,
    base: &CommitId,
    head: &CommitId,
) -> Result<git2::Index, GitError> {
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
    repo.merge_commits(&base_commit, &head_commit, None)
        .map_err(|e| GitError::Other(format!("merge_commits in-memory failed: {}", e.message())))
}

/// `ours` is the base side, `theirs` the PR's — `merge_commits(base, head)`
/// fixes that order.
fn conflict_path(c: &git2::IndexConflict) -> Option<PathBuf> {
    let of = |e: &Option<git2::IndexEntry>| {
        e.as_ref()
            .and_then(|e| std::str::from_utf8(&e.path).ok())
            .map(PathBuf::from)
    };
    of(&c.our)
        .or_else(|| of(&c.their))
        .or_else(|| of(&c.ancestor))
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
