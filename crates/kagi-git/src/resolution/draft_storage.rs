//! ADR-0191's same-directory write/sync/rename convention, with a draft lock.
use super::*;
use crate::advisory_lock::AdvisoryLock;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{self, Write};

#[derive(Debug, Clone, Default)]
pub(super) struct Snapshot {
    pub hash: Option<[u8; 32]>,
    pub files: BTreeMap<PathBuf, Baseline>,
}

#[derive(Debug, Clone)]
pub(super) struct Baseline {
    pub hash: [u8; 32],
    pub state: Option<hunk_state::HunkState>,
}

pub(super) fn file_hash(path: &Path, file: &FileResolution) -> [u8; 32] {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    // FileRecord contains only infallible scalar/sequence projections.
    serde_json::to_writer(&mut writer, &file_record(path, file))
        .expect("draft record serialization into a content digest");
    writer.0.finalize().into()
}

impl Snapshot {
    pub fn capture(buffer: &ResolutionBuffer, hash: Option<[u8; 32]>) -> Self {
        Self {
            hash,
            files: buffer
                .files
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        Baseline {
                            hash: file_hash(path, file),
                            state: file.hunk_state.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

pub(super) struct Disk {
    pub buffer: ResolutionBuffer,
    pub content: String,
    pub hash: [u8; 32],
}

pub(super) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn error(path: &Path, cause: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("conflicts: draft {}: {cause}", path.display()))
}

/// A missing primary may be the interval after a legacy interrupted replacement;
/// try the backup too. Only verified corruption permits fallback on read failure;
/// filesystem errors must not hide a newer primary behind an older backup.
pub(super) fn read(repo: &Path, path: &Path) -> Result<Option<Disk>, GitError> {
    let (absent, good) = match std::fs::read(path) {
        Ok(bytes) => (
            false,
            String::from_utf8(bytes)
                .ok()
                .and_then(|text| parse_buffer_json(repo, &text).map(|buffer| (text, buffer))),
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (true, None),
        Err(e) => return Err(error(path, e)),
    };
    let (content, buffer) = match good {
        Some(good) => good,
        None => {
            let previous = sibling(path, ".prev");
            match std::fs::read_to_string(&previous) {
                Ok(text) => {
                    let buffer = parse_buffer_json(repo, &text).ok_or_else(|| {
                        error(&previous, "invalid draft JSON; original preserved")
                    })?;
                    (text, buffer)
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound && absent => return Ok(None),
                Err(e) => return Err(error(path, format!("cannot recover document: {e}"))),
            }
        }
    };
    let hash = Sha256::digest(content.as_bytes()).into();
    *buffer
        .disk_snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Arc::new(Snapshot::capture(&buffer, Some(hash)));
    Ok(Some(Disk {
        buffer,
        content,
        hash,
    }))
}

/// Restore-time upgrade recovery only: ordinary writers still refuse corrupt
/// input. Re-read under the same writer lock before moving any damaged bytes.
pub(super) fn read_for_restore(
    repo: &Path,
    path: &Path,
) -> Result<(Option<Disk>, Vec<DraftRestoreWarning>), GitError> {
    if let Ok(disk) = read(repo, path) {
        return Ok((disk, Vec::new()));
    }
    let _lock = lock(path)?;
    let cause = match read(repo, path) {
        Ok(disk) => return Ok((disk, Vec::new())),
        Err(error) => error,
    };
    let previous = sibling(path, ".prev");
    let mut warnings = Vec::new();
    for source in [path, previous.as_path()] {
        let bytes = match std::fs::read(source) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(error(source, e)),
        };
        // A permission/read failure is not evidence of damaged JSON. Only
        // positively invalid documents are quarantined, never a good backup.
        if std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| parse_buffer_json(repo, text))
            .is_some()
        {
            return Err(cause);
        }
        let aside = rescue(source)?;
        let entry = crate::oplog::OpLogEntry::new(
            "conflict-draft-restore",
            repo.display().to_string(),
            crate::ops::StateSummary {
                head: "unobserved".into(),
                dirty: "unobserved".into(),
            },
            crate::oplog::OpOutcome::Failed {
                error: format!(
                    "Draft could not be read; the damaged file was moved aside as {}",
                    aside.display()
                ),
            },
        )
        .with_nothing_moved();
        let recording_error = match crate::backend::recording::finalize(entry) {
            crate::backend::recording::Recording::Failed { error, .. } => Some(error.to_string()),
            _ => None,
        };
        warnings.push(DraftRestoreWarning {
            rescued_path: aside,
            recording_error,
        });
    }
    if warnings.is_empty() {
        return Err(cause);
    }
    Ok((None, warnings))
}

/// ADR-0191 settings/store.rs via ui-core/src/atomic_file.rs::rescue: reserve
/// `.corrupt[.N]` exclusively, then rename over only our empty reservation.
fn rescue(path: &Path) -> Result<PathBuf, GitError> {
    let base = path.file_name().unwrap_or_default();
    for n in 0..100u32 {
        let mut name = base.to_os_string();
        name.push(if n == 0 {
            ".corrupt".to_string()
        } else {
            format!(".corrupt.{n}")
        });
        let aside = path.with_file_name(name);
        match OpenOptions::new().write(true).create_new(true).open(&aside) {
            Ok(_) => match std::fs::rename(path, &aside) {
                Ok(()) => return Ok(aside),
                Err(e) => {
                    let _ = std::fs::remove_file(&aside);
                    return Err(error(path, e));
                }
            },
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(error(&aside, e)),
        }
    }
    Err(error(path, "all reserved .corrupt names are occupied"))
}

/// Never unlink the sidecar: every process must lock the same inode. The guard
/// explicitly unlocks on drop; process death also releases the OS advisory lock.
pub(super) fn lock(path: &Path) -> Result<AdvisoryLock, GitError> {
    let lock_path = sibling(path, ".lock");
    crate::advisory_lock::lock_exclusive(&lock_path, std::time::Duration::from_secs(2)).map_err(
        |e| {
            if e.kind() == io::ErrorKind::WouldBlock {
                error(&lock_path, "timed out after 2 s waiting for draft writer")
            } else {
                error(&lock_path, e)
            }
        },
    )
}

#[cfg(all(test, unix))]
#[test]
fn draft_lock_releases_while_unrelated_fork_is_alive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("buffer.json");
    let owner = lock(&path).unwrap();
    let admission = crate::advisory_lock::tests::with_unrelated_fork(|| {
        drop(owner);
        crate::advisory_lock::lock_exclusive(&sibling(&path, ".lock"), std::time::Duration::ZERO)
    });
    drop(
        admission.expect("draft owner must unlock buffer.json.lock before an unrelated fork execs"),
    );
}

/// Deliberately duplicated across the crate boundary: the byte-for-byte algorithm
/// used by settings/store.rs via ui-core/src/atomic_file.rs (ADR-0191). The Git
/// backend cannot depend on UI core. Keep temp naming and durability identical.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
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

pub(super) fn replace(path: &Path, bytes: &[u8], disk: Option<&Disk>) -> Result<(), GitError> {
    // Back up only a decoded good document, never the torn primary. Both swaps
    // are atomic: a crash at either point leaves at least one complete document.
    let previous = sibling(path, ".prev");
    write_atomic(
        &previous,
        disk.map_or(bytes, |disk| disk.content.as_bytes()),
    )
    .map_err(|e| error(&previous, e))?;
    write_atomic(path, bytes).map_err(|e| error(path, e))
}

impl ResolutionBuffer {
    pub(super) fn save_draft_document(&self, path: &Path) -> Result<(), GitError> {
        let _lock = lock(path)?;
        // Never hold the memory-baseline mutex across file I/O: UI capture and
        // preservation clones only copy this immutable Arc.
        let snapshot = self
            .disk_snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let disk = read(&self.repo_path, path)?;
        // Content CAS, not stat/mtime: a writer that committed since load forces
        // a fresh per-file/per-hunk merge under the interprocess lock.
        let changed = disk.as_ref().map(|disk| disk.hash) != snapshot.hash;
        let json = self
            .autosave_json(disk.as_ref().map(|disk| &disk.buffer), &snapshot, changed)
            .map_err(|e| error(path, e))?;
        replace(path, json.as_bytes(), disk.as_ref())?;
        *self.disk_snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(
            Snapshot::capture(self, Some(Sha256::digest(json.as_bytes()).into())),
        );
        Ok(())
    }
}
