//! Unix per-entry allocation and hardlink identity (#633).
//!
//! Everything needed is already in the `lstat` the traversal performed, so
//! this costs no extra syscall.

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::{EntryFacts, LinkKey};

/// `st_blocks` is defined in 512-byte units regardless of the filesystem's own
/// block size, so this is the same arithmetic `du` performs.
const BLOCK_BYTES: u64 = 512;

pub(super) fn entry_facts(path: &Path, meta: &Metadata) -> Result<EntryFacts, String> {
    let allocated = meta
        .blocks()
        .checked_mul(BLOCK_BYTES)
        .ok_or_else(|| format!("{}: block count overflows a byte count", path.display()))?;
    // A directory's link count is subdirectory bookkeeping, not sharing.
    let link =
        (!meta.is_dir() && meta.nlink() > 1).then(|| LinkKey(meta.dev(), u128::from(meta.ino())));
    Ok(EntryFacts { allocated, link })
}
