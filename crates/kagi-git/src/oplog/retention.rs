//! Explicit entry retirement. No age/cap sweep: backups live as long as receipts.
use super::{entry_to_json, log_file_path, reading::Reader, OpLogEntry};
use crate::{ops::backup, GitError};
use git2::{Oid, Repository};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::PathBuf;

/// Frozen preview for explicit confirmation; execution checks log and ref drift.
/// Constructed only by Backend, never by callers supplying cleanup ref names.
#[derive(Debug)]
pub struct ForgetOplogPlan {
    pub(super) path: PathBuf,
    common_dir: PathBuf,
    before: String,
    retained: String,
    entry: OpLogEntry,
    refs: Vec<(String, Oid)>,
    next_id: u64,
}
impl ForgetOplogPlan {
    pub fn entry(&self) -> &OpLogEntry {
        &self.entry
    }
    /// Roots whose recovery lifetime ends with this entry (shared roots remain).
    pub fn backup_refs(&self) -> impl Iterator<Item = &str> {
        self.refs.iter().map(|(name, _)| name.as_str())
    }
}

pub(super) fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("oplog retention: {error}"))
}

/// Stable sidecar lock survives atomic log replacement. Append shares it.
/// Fail closed if another writer owns it; do not block a GUI thread on a lock.
pub(super) fn lock(path: &std::path::Path) -> Result<File, GitError> {
    let file = open_lock(path)?;
    file.try_lock().map_err(io)?;
    Ok(file)
}

/// Concurrent normal appends must not lose records just because a short write
/// is in progress. Bound waiting so a stalled external owner cannot hang us.
pub(super) fn append_lock(path: &std::path::Path) -> Result<File, GitError> {
    use std::time::{Duration, Instant};
    let file = open_lock(path)?;
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(io(error)),
        }
    }
}

fn open_lock(path: &std::path::Path) -> Result<File, GitError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("jsonl.lock"))
        .map_err(io)
}

pub(crate) fn plan(repo: &Repository, entry: &OpLogEntry) -> Result<ForgetOplogPlan, GitError> {
    let path = log_file_path()?.ok_or_else(|| io("missing log path"))?;
    let _lock = lock(&path)?;
    let before = std::fs::read_to_string(&path).map_err(io)?;
    let owner = Repository::discover(&entry.repo).map_err(io)?;
    let common_dir = std::fs::canonicalize(repo.commondir()).map_err(io)?;
    if std::fs::canonicalize(owner.commondir()).map_err(io)? != common_dir {
        return Err(io("entry belongs to another repository"));
    }
    let (retained, remaining) = without_entry(&before, entry)?;
    let next_id = remaining
        .iter()
        .chain(std::iter::once(entry))
        .map(|entry| entry.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let mut refs = Vec::new();
    for name in &entry.backup_refs {
        backup::validate_reference(name)?;
        if remaining
            .iter()
            .any(|other| other.backup_refs.contains(name))
            || refs.iter().any(|(saved, _)| saved == name)
        {
            continue;
        }
        let reference = repo.find_reference(name).map_err(io)?;
        let oid = reference
            .target()
            .ok_or_else(|| io("backup ref is symbolic"))?;
        repo.find_blob(oid).map_err(io)?;
        refs.push((name.clone(), oid));
    }
    Ok(ForgetOplogPlan {
        path,
        common_dir,
        before,
        retained,
        next_id,
        entry: entry.clone(),
        refs,
    })
}

fn without_entry(content: &str, entry: &OpLogEntry) -> Result<(String, Vec<OpLogEntry>), GitError> {
    let mut retained = String::new();
    let mut remaining = Vec::new();
    let mut found = 0;
    let mut reader = Reader::default();
    for line in content.split_inclusive('\n') {
        if line.trim().is_empty() {
            retained.push_str(line);
            continue;
        }
        // Ownership comes from the same validated top-level JSON as the reader.
        let (parsed, legacy) = reader.parse(line)?;
        if entry_to_json(&parsed) == entry_to_json(entry) {
            found += 1;
        } else {
            if legacy {
                // Freeze surviving identities before deleting an earlier legacy line.
                let mut value: serde_json::Value = serde_json::from_str(line).map_err(io)?;
                value["id"] = serde_json::json!(parsed.id);
                value["parent"] = serde_json::json!(parsed.parent);
                retained.push_str(&serde_json::to_string(&value).map_err(io)?);
                retained.push('\n');
            } else {
                retained.push_str(line);
            }
            remaining.push(parsed);
        }
    }
    if found != 1 {
        return Err(io("entry is missing or ambiguous; re-plan"));
    }
    Ok((retained, remaining))
}

/// Log replacement precedes ref deletion. Any failure leaks recovery rather
/// than leaving a retained receipt with missing bytes. `retired` marks Partial.
pub(crate) fn execute(
    repo: &Repository,
    plan: &ForgetOplogPlan,
    retired: &mut bool,
) -> Result<(), GitError> {
    if std::fs::canonicalize(repo.commondir()).map_err(io)? != plan.common_dir
        || log_file_path()?.as_ref() != Some(&plan.path)
    {
        return Err(io("repository/log identity changed; re-plan"));
    }
    let mut lock = lock(&plan.path)?;
    execute_locked(repo, plan, retired, &mut lock)
}

// The caller owns the stable log lock for this entire log/ref transaction.
pub(super) fn execute_locked(
    repo: &Repository,
    plan: &ForgetOplogPlan,
    retired: &mut bool,
    lock: &mut File,
) -> Result<(), GitError> {
    if std::fs::read_to_string(&plan.path).map_err(io)? != plan.before {
        return Err(io("oplog changed after confirmation; re-plan"));
    }
    let mut transaction = repo.transaction().map_err(io)?;
    for (name, oid) in &plan.refs {
        transaction.lock_ref(name).map_err(io)?;
        if repo.find_reference(name).map_err(io)?.target() != Some(*oid) {
            return Err(io("backup reference changed after confirmation; re-plan"));
        }
        transaction.remove(name).map_err(io)?;
    }
    // Preserve sequence monotonicity even when retiring the newest/only entry.
    reserve_id(lock, plan.next_id)?;
    let parent = plan.path.parent().ok_or_else(|| io("log has no parent"))?;
    let mut replacement = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    replacement
        .write_all(plan.retained.as_bytes())
        .map_err(io)?;
    replacement.as_file().sync_all().map_err(io)?;
    replacement.persist(&plan.path).map_err(io)?;
    *retired = true;
    // Persist the directory entry before releasing a reachability root.
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(io)?;
    transaction.commit().map_err(io)?;
    for (name, _) in &plan.refs {
        match repo.find_reference(name) {
            Err(error) if error.code() == git2::ErrorCode::NotFound => (),
            _ => return Err(io("backup ref cleanup could not be verified")),
        }
    }
    Ok(())
}

/// Called while holding the same log lock as retirement: a waiting append
/// cannot resurrect ownership of a ref already retired by the preceding writer.
pub(super) fn validate_append_roots(entry: &OpLogEntry) -> Result<(), GitError> {
    if entry.backup_refs.is_empty() {
        return Ok(());
    }
    let repo = Repository::discover(&entry.repo).map_err(io)?;
    for name in &entry.backup_refs {
        backup::validate_reference(name)?;
        let reference = repo.find_reference(name).map_err(io)?;
        let oid = reference
            .target()
            .ok_or_else(|| io("backup ref is symbolic"))?;
        repo.find_blob(oid).map_err(io)?;
    }
    Ok(())
}

/// The stable lock file also holds the next sequence id. Reserve before append
/// or retirement; failed writes may leave gaps, never reuse a retired identity.
pub(super) fn reserve_id(lock: &mut File, floor: u64) -> Result<u64, GitError> {
    lock.rewind().map_err(io)?;
    let mut previous = String::new();
    lock.read_to_string(&mut previous).map_err(io)?;
    let next = if previous.trim().is_empty() {
        0
    } else {
        previous.trim().parse::<u64>().map_err(io)?
    };
    let assigned = next.max(floor);
    let following = assigned
        .checked_add(1)
        .ok_or_else(|| io("oplog sequence exhausted"))?;
    lock.rewind().map_err(io)?;
    let bytes = following.to_string();
    lock.write_all(bytes.as_bytes()).map_err(io)?;
    lock.set_len(bytes.len() as u64).map_err(io)?;
    lock.sync_all().map_err(io)?;
    Ok(assigned)
}
