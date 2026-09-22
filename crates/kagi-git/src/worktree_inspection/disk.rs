//! Worktree occupancy traversal (#633) — `du`'s arithmetic, in-process.
//!
//! Platform-independent walk; the per-entry allocation and hardlink identity
//! come from [`platform`] (`disk_unix.rs` / `disk_windows.rs`).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::{cancelled, CANCELLED};

#[cfg(unix)]
#[path = "disk_unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "disk_windows.rs"]
mod platform;
#[cfg(not(any(unix, windows)))]
compile_error!("worktree occupancy needs a platform allocation API (unix or windows)");

/// Directory name whose subtree is reported separately in the breakdown.
const TARGET_DIR_NAME: &str = "target";

/// How much disk a worktree occupies, measured the way `du` measures it.
///
/// **Occupancy, not reclaimable bytes.** Removing the worktree frees less than
/// this wherever storage is shared: APFS clones and reflink copies share
/// extents with files that survive, and a file hardlinked from outside the
/// worktree keeps its blocks. It answers "where is the space", which is the
/// question #633 asks — never "you will get exactly this back".
///
/// Exact in a narrower sense: it is the true sum of what the filesystem
/// reports as allocated for every entry reached, with nothing skipped,
/// estimated, or filtered. A traversal that cannot reach everything reports an
/// error instead of a smaller number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorktreeDiskUsage {
    /// Whole worktree: tracked files, ignored build output, and the Git
    /// administrative data inside it. `target_bytes` is a subset.
    pub allocated_bytes: u64,
    /// The part of `allocated_bytes` under directories named `target`.
    ///
    /// A **name-based breakdown only**. It locates Cargo build output in the
    /// usual layout so a large row is explainable; it is not a classification
    /// of those bytes as disposable, and no safety verdict reads it.
    pub target_bytes: u64,
}

/// Identity of a multiply-linked entry: volume plus file id, the pair `du`
/// uses to count shared bytes once.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct LinkKey(pub u64, pub u128);

/// What the traversal needs from one entry, per platform.
pub(super) struct EntryFacts {
    /// Bytes this entry occupies on the physical storage — `st_blocks * 512`
    /// on Unix, and the same contract on Windows, never a logical length or
    /// the size of an allocated range that compression left partly empty.
    pub allocated: u64,
    /// Present only when the entry has more than one link, so its bytes are
    /// reachable under several names and must be counted once.
    pub link: Option<LinkKey>,
}

/// The Windows "how many bytes does this entry physically occupy" decision.
///
/// It lives here rather than in `disk_windows.rs` because it is the part worth
/// pinning and the only part that needs no Windows runtime: `disk_windows.rs`
/// holds the two `GetFileInformationByHandleEx` calls, this holds what their
/// answers mean.
///
/// `FILE_STANDARD_INFO.AllocationSize` is the size of the allocated *range*,
/// not necessarily the physical storage behind it: a compressed file can occupy
/// less. `FILE_COMPRESSION_INFO.CompressedFileSize` instead reports "the number
/// of bytes actually allocated on the underlying physical storage", a multiple
/// of the cluster size and never above `AllocationSize`
/// ([MS-FSA §2.1.5.11.7](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/484bc722-6440-49a0-982a-9ba27cddf513)).
/// For an ordinary uncompressed file the two agree.
///
/// When the compression query cannot be answered the entry has no known
/// physical size, and this reports that. Falling back to `AllocationSize`
/// would put the number this module refuses to estimate back into the total
/// under a different name.
#[cfg(any(windows, test))]
pub(super) fn windows_physical_bytes(
    path: &Path,
    directory: bool,
    allocation: i64,
    compressed: impl FnOnce() -> Result<i64, String>,
) -> Result<u64, String> {
    if directory {
        // A directory stream is not compressible and the query leaves
        // `CompressedFileSize` at zero for one (MS-FSA §2.1.5.11.7 sets only
        // `CompressionState` for a DirectoryStream), so asking would report
        // every directory as occupying nothing. Its own allocation is the
        // only physical answer Windows offers for it.
        return nonnegative_bytes(path, allocation, "allocation size");
    }
    let compressed = nonnegative_bytes(path, compressed()?, "compressed file size")?;
    let allocation = nonnegative_bytes(path, allocation, "allocation size")?;
    if compressed > allocation {
        // The specification makes `CompressedFileSize <= AllocationSize` a
        // MUST. A filesystem that breaks it has told us two incompatible
        // things about one entry and neither can be summed.
        return Err(format!(
            "{}: compressed file size {compressed} exceeds allocation size {allocation}",
            path.display()
        ));
    }
    Ok(compressed)
}

/// Windows reports sizes as signed; a negative one is a broken answer, never a
/// huge file.
#[cfg(any(windows, test))]
fn nonnegative_bytes(path: &Path, value: i64, what: &str) -> Result<u64, String> {
    u64::try_from(value).map_err(|_| format!("{}: negative {what} ({value})", path.display()))
}

/// Sum the allocation of everything under `root`, splitting out `target/`.
///
/// - `.gitignore` is not consulted: ignored build output is the whole point.
/// - Symlinks are never followed, so the walk stays inside the worktree.
/// - Multiply-linked entries count once per worktree.
/// - Any failure is fatal to the number. An unreadable directory or an entry
///   that disappears mid-scan means the sum is no longer the whole tree, and a
///   quietly smaller total is worse than no total.
pub(super) fn scan(root: &Path, cancel: &AtomicBool) -> Result<WorktreeDiskUsage, String> {
    let root_meta = std::fs::symlink_metadata(root)
        .map_err(|error| format!("{}: unreadable: {error}", root.display()))?;
    if root_meta.file_type().is_symlink() {
        return Err(format!(
            "{}: worktree root is a symlink; refusing to traverse it",
            root.display()
        ));
    }
    if !root_meta.is_dir() {
        return Err(format!(
            "{}: worktree root is not a directory",
            root.display()
        ));
    }

    // The root's own allocation counts; the root is never treated as build
    // output, even if the worktree itself happens to be named `target`.
    let mut allocated = platform::entry_facts(root, &root_meta)?.allocated;
    let mut target = 0u64;
    let mut multiply_linked: HashSet<LinkKey> = HashSet::new();
    let mut stack: Vec<(PathBuf, bool)> = vec![(root.to_path_buf(), false)];

    while let Some((dir, in_target)) = stack.pop() {
        if cancelled(cancel) {
            return Err(CANCELLED.to_string());
        }
        let entries = std::fs::read_dir(&dir)
            .map_err(|error| format!("{}: unreadable directory: {error}", dir.display()))?;
        for entry in entries {
            // Polled per entry: a relaxed load is noise next to the `stat` it
            // rides with, and a warm `target/` must stop the moment the UI
            // abandons the scan.
            if cancelled(cancel) {
                return Err(CANCELLED.to_string());
            }
            let entry = entry.map_err(|error| {
                format!("{}: unreadable directory entry: {error}", dir.display())
            })?;
            let path = entry.path();
            // `DirEntry::metadata` does not traverse symlinks.
            let meta = entry
                .metadata()
                .map_err(|error| format!("{}: unreadable: {error}", path.display()))?;
            let facts = platform::entry_facts(&path, &meta)?;
            let descends_into_target = in_target
                || (meta.is_dir() && entry.file_name().as_os_str() == OsStr::new(TARGET_DIR_NAME));

            let first_sighting = facts.link.is_none_or(|key| multiply_linked.insert(key));
            if first_sighting {
                allocated = add(allocated, facts.allocated, &path)?;
                if descends_into_target {
                    target = add(target, facts.allocated, &path)?;
                }
            }

            if meta.is_dir() {
                stack.push((path, descends_into_target));
            }
        }
    }

    Ok(WorktreeDiskUsage {
        allocated_bytes: allocated,
        target_bytes: target,
    })
}

fn add(total: u64, bytes: u64, path: &Path) -> Result<u64, String> {
    total
        .checked_add(bytes)
        .ok_or_else(|| format!("{}: worktree size overflows a byte count", path.display()))
}

#[cfg(test)]
#[path = "disk_tests.rs"]
mod tests;
