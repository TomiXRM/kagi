//! Windows per-entry physical usage and hardlink identity (#633).
//!
//! `std`'s Windows metadata exposes only the logical file size, and reporting
//! that as occupancy would misreport every compressed, sparse, or
//! cluster-padded file. The real answers come from one attributes-only handle
//! per entry:
//!
//! - `FILE_COMPRESSION_INFO.CompressedFileSize` — bytes actually on the
//!   physical storage, the counterpart of `st_blocks * 512`. Queried for
//!   files; a directory has no such stream and falls back to its own
//!   `FILE_STANDARD_INFO.AllocationSize`. Which value means what, and what an
//!   unanswerable query means, is [`super::windows_physical_bytes`].
//! - `FILE_STANDARD_INFO.NumberOfLinks` + `FILE_ID_INFO` — the volume/file-id
//!   pair that counts a hardlinked file once, the counterpart of `(dev, ino)`.
//!
//! The handle is opened through `std`'s `OpenOptions` (so it is closed by
//! `Drop`) with `FILE_READ_ATTRIBUTES` only, sharing everything, and
//! `FILE_FLAG_OPEN_REPARSE_POINT` so a symlink is never followed.
//! `FILE_FLAG_BACKUP_SEMANTICS` is what makes a directory openable at all.
//!
//! Every query rides that one handle. `GetCompressedFileSizeW` would answer
//! the same question from a path, but it reopens by name after the guarded
//! open — following reparse points and whatever the name points at by then —
//! so it would trade the no-follow guarantee for the number it returns.

use std::fs::{File, Metadata, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FileCompressionInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx,
    FILE_COMPRESSION_INFO, FILE_ID_INFO, FILE_STANDARD_INFO,
};

use super::{windows_physical_bytes, EntryFacts, LinkKey};

const FILE_READ_ATTRIBUTES: u32 = 0x0080;
const FILE_SHARE_ALL: u32 = 0x0000_0001 | 0x0000_0002 | 0x0000_0004;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

pub(super) fn entry_facts(path: &Path, _meta: &Metadata) -> Result<EntryFacts, String> {
    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("{}: cannot read attributes: {error}", path.display()))?;

    let standard: FILE_STANDARD_INFO = by_handle(&file, FileStandardInfo, path, "standard info")?;
    // The compression query is made only where it answers, and its failure is
    // carried into the decision rather than swallowed here.
    let allocated = windows_physical_bytes(
        path,
        standard.Directory,
        standard.AllocationSize,
        || -> Result<i64, String> {
            let compression: FILE_COMPRESSION_INFO =
                by_handle(&file, FileCompressionInfo, path, "compression info")?;
            Ok(compression.CompressedFileSize)
        },
    )?;

    // Directories are never hardlinked; only files carry shared allocation.
    let link = if standard.NumberOfLinks > 1 && !standard.Directory {
        let id: FILE_ID_INFO = by_handle(&file, FileIdInfo, path, "file id")?;
        Some(LinkKey(
            id.VolumeSerialNumber,
            u128::from_le_bytes(id.FileId.Identifier),
        ))
    } else {
        None
    };

    Ok(EntryFacts { allocated, link })
}

/// One `GetFileInformationByHandleEx` call into a zeroed `#[repr(C)]` value.
///
/// A filesystem that cannot answer a class — neither `FILE_COMPRESSION_INFO`
/// nor `FILE_ID_INFO` is universal across FAT and the network redirectors — is
/// a failure, not a zero. A zeroed compression answer would count a file as
/// occupying nothing, and a missing file id would let hardlink deduplication
/// silently double-count.
fn by_handle<T: Default>(file: &File, class: i32, path: &Path, what: &str) -> Result<T, String> {
    let mut info = T::default();
    let size = u32::try_from(std::mem::size_of::<T>())
        .map_err(|_| format!("{}: {what} buffer too large", path.display()))?;
    // SAFETY: `info` is a live, writable `#[repr(C)]` value of exactly `size`
    // bytes matching `class`, and `file` owns the handle for this call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            class,
            (&mut info as *mut T).cast(),
            size,
        )
    };
    if ok == 0 {
        return Err(format!(
            "{}: {what} unavailable: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(info)
}
