//! Small JSON-file primitives shared by kagi's own files under `~/.kagi`
//! (`settings.json` #491, `pr-viewed/*.json` #351): atomic replacement and
//! moving a corrupt file aside instead of overwriting it.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Move a corrupt file aside, under a name **reserved exclusively**
/// (`<name>.corrupt`, `<name>.corrupt.1`, …) so a second corruption never
/// overwrites the first rescue (and two processes never fight over one name).
/// Returns where it went, or `None` if the original could not be moved — in
/// which case the caller must not write. Failures are logged as
/// `<tag>: rescue …`.
pub(crate) fn rescue(path: &Path, tag: &str) -> Option<PathBuf> {
    let base = path.file_name()?.to_os_string();
    for n in 0..100u32 {
        let mut name = base.clone();
        name.push(if n == 0 {
            ".corrupt".to_string()
        } else {
            format!(".corrupt.{n}")
        });
        let aside = path.with_file_name(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&aside)
        {
            // The reservation is an empty file we just created, so renaming the
            // original over it cannot destroy anyone else's rescue.
            Ok(_) => {
                return match std::fs::rename(path, &aside) {
                    Ok(()) => Some(aside),
                    Err(e) => {
                        klog!("{tag}: rescue rename failed: {e}");
                        let _ = std::fs::remove_file(&aside);
                        None
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                klog!("{tag}: rescue reservation failed: {e}");
                return None;
            }
        }
    }
    None
}

/// Replace `path` with `bytes` via a same-directory temp file + rename. The
/// rename is atomic on every platform kagi ships on; the preceding `sync_all`
/// makes the *contents* durable before the swap. This is crash-atomic, not a
/// power-loss guarantee — the containing directory is not fsync'd.
///
/// The temp file inherits the mode of the file it replaces, so a file the
/// user tightened (`chmod 600`) does not come back at the default umask after
/// the next write. With no existing file there is nothing to carry over and
/// the platform default stands — kagi does not invent a mode.
///
/// (The `.corrupt` rescue needs no equivalent: its reserved file is *replaced*
/// by renaming the original onto it, so the rescue keeps the original's inode
/// and therefore its own mode.)
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(name);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if written.is_ok() {
        if let Ok(md) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, md.permissions());
        }
    }
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
