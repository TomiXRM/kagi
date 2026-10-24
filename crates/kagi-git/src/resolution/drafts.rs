//! Draft authority and recovery within the existing per-repository document.

use super::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub(super) struct RecoveryEntry {
    pub(super) path: PathBuf,
    pub(super) file: FileResolution,
}

/// Read-only access to preserved drafts; inspection never grants live authority.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryDraft<'a> {
    pub path: &'a Path,
    pub identity: Option<&'a ConflictRevision>,
    pub result: Option<&'a [ResolvedLine]>,
    pub raw_result: Option<RawResolution>,
}

/// Owned inspection data without live conflict sides, hunks or editing history.
/// Like the borrowed view, these preserved bodies grant no resolution authority.
#[derive(Debug)]
pub struct OwnedRecoveryDraft {
    pub path: PathBuf,
    pub identity: Option<ConflictRevision>,
    pub result: Option<Vec<ResolvedLine>>,
    pub raw_result: Option<RawResolution>,
}

pub(super) fn file_identity(
    git_dir: &Path,
    context: &ConflictRevision,
    path: &Path,
    conflict: &git2::IndexConflict,
) -> ConflictRevision {
    let mut hash = Sha256::new();
    let mut part = |bytes: &[u8]| {
        crate::backend::conflict_ops::conflict_observe::digest_part(&mut hash, bytes);
    };
    part(b"kagi-conflict-draft-v1");
    part(git_dir.as_os_str().as_encoded_bytes());
    part(context.as_str().as_bytes());
    part(path.as_os_str().as_encoded_bytes());
    for (stage, entry) in [
        (1u8, conflict.ancestor.as_ref()),
        (2u8, conflict.our.as_ref()),
        (3u8, conflict.their.as_ref()),
    ] {
        part(&[stage, u8::from(entry.is_some())]);
        if let Some(entry) = entry {
            part(&entry.path);
            part(entry.id.as_bytes());
            part(&entry.mode.to_be_bytes());
        }
    }
    ConflictRevision::from_fingerprint(hex::encode(hash.finalize()))
}

pub(super) fn parse_identity(value: Option<Value>) -> Option<ConflictRevision> {
    let value = value?;
    if value.get("version")?.as_u64()? != 1 {
        return None;
    }
    let fingerprint = value.get("fingerprint")?.as_str()?;
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(ConflictRevision::from_fingerprint(
        fingerprint.to_ascii_lowercase(),
    ))
}

pub(super) fn parse_path(bytes: Option<Value>, path: Option<Value>) -> Option<PathBuf> {
    if let Some(bytes) = bytes {
        let bytes: Vec<u8> = serde_json::from_value(bytes).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            return Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)));
        }
        #[cfg(not(unix))]
        {
            return String::from_utf8(bytes).ok().map(PathBuf::from);
        }
    }
    json_string(path).map(PathBuf::from)
}

fn same_identity(current: &FileResolution, saved: &FileResolution) -> bool {
    current.identity.is_some() && current.identity == saved.identity
}

pub(super) fn compatible(current: &FileResolution, saved: &FileResolution) -> bool {
    if saved.recovery_only || !same_identity(current, saved) {
        return false;
    }
    // Legacy source-unknown drafts remain inspectable, never re-authorized.
    if saved.worktree_fingerprint.is_none() && current.worktree_fingerprint.is_some() {
        return false;
    }
    if current.raw {
        saved.result.is_none()
            && saved.raw_result.is_none_or(|raw| {
                Some(raw) == current.current_raw || Some(raw) == current.incoming_raw
            })
    } else {
        saved.raw_result.is_none()
    }
}

fn has_draft(file: &FileResolution) -> bool {
    file.result.is_some() || file.raw_result.is_some()
}

pub(super) fn has_draft_state(file: &FileResolution) -> bool {
    file.draft_state || has_draft(file) || !file.undo.is_empty() || !file.redo.is_empty()
}

pub(super) fn same_draft(left: &FileResolution, right: &FileResolution) -> bool {
    left.recovery_only == right.recovery_only && same_draft_body(left, right)
}

fn same_draft_body(left: &FileResolution, right: &FileResolution) -> bool {
    same_draft_metadata(left, right)
        && left.result == right.result
        && left.hunk_state == right.hunk_state
}

pub(super) fn same_draft_metadata(left: &FileResolution, right: &FileResolution) -> bool {
    left.identity == right.identity
        && left.worktree_fingerprint == right.worktree_fingerprint
        && left.draft_state == right.draft_state
        && left.raw_result == right.raw_result
        && left.current_text == right.current_text
        && left.incoming_text == right.incoming_text
        && left.binary == right.binary
}

impl ResolutionBuffer {
    /// Known operation + this file's stages. Unrelated staged files are excluded.
    pub fn file_identity(&self, path: &Path) -> Option<&ConflictRevision> {
        self.files.get(path)?.identity.as_ref()
    }

    pub fn recovery_drafts(&self) -> impl Iterator<Item = RecoveryDraft<'_>> {
        self.recoveries
            .iter()
            .filter(|entry| has_draft(&entry.file))
            .map(|entry| RecoveryDraft {
                path: &entry.path,
                identity: entry.file.identity.as_ref(),
                result: entry.file.result.as_deref(),
                raw_result: entry.file.raw_result,
            })
    }

    /// Consume the buffer, moving only preserved inspection data to the caller.
    /// Current conflict state is dropped here; recovery editing state is dropped
    /// as each entry is consumed, without copying draft bodies or metadata.
    pub fn into_recovery_drafts(self) -> impl Iterator<Item = OwnedRecoveryDraft> {
        self.recoveries
            .into_iter()
            .filter(|entry| has_draft(&entry.file))
            .map(|entry| OwnedRecoveryDraft {
                path: entry.path,
                identity: entry.file.identity,
                result: entry.file.result,
                raw_result: entry.file.raw_result,
            })
    }

    pub(super) fn preserve_recovery(&mut self, path: PathBuf, file: FileResolution) {
        if !has_draft_state(&file) {
            return;
        }
        if let Some(index) = self
            .recoveries
            .iter()
            .position(|entry| entry.path == path && same_draft(&entry.file, &file))
        {
            self.recoveries.remove(index);
        }
        // Equality deduplicates content, not recency: a latest live undo may
        // equal an older null preceding a subsequently preserved body.
        self.recoveries.push(RecoveryEntry { path, file });
    }

    /// Consume the latest buffer. Only a known matching identity authorizes a
    /// live draft; even an undone-to-None state supersedes an older disk draft.
    /// All other bodies remain in the same document's recovery collection.
    pub fn retain_drafts(&mut self, previous: ResolutionBuffer) {
        let ResolutionBuffer {
            files,
            mut hunks,
            mut recoveries,
            disk_snapshot,
            draft_writer,
            ..
        } = previous;
        let mut matched = std::collections::BTreeSet::new();
        self.disk_snapshot = disk_snapshot;
        self.draft_writer = draft_writer;
        for (path, mut saved) in files {
            if let Some(current) = self
                .files
                .get_mut(&path)
                .filter(|current| compatible(current, &saved))
            {
                super::worktree_state::retain_provenance(current, &mut saved);
                if has_draft_state(&saved) {
                    current.replace_result(saved.result.take(), &mut self.marker_residue_count);
                    current.raw_result = saved.raw_result;
                    current.hunk_state = saved.hunk_state.take();
                    self.hunks.remove(&path);
                    current.draft_state = true;
                    current.editing_origin = saved.editing_origin.take();
                    current.live_editing_origin = saved.live_editing_origin;
                    current.pending_edit = saved.pending_edit;
                    current.undo = std::mem::take(&mut saved.undo);
                    current.redo = std::mem::take(&mut saved.redo);
                }
                if let Some(model) = hunks.remove(&path) {
                    self.hunks.insert(path.clone(), model);
                }
                if has_draft_state(current) {
                    matched.insert(path);
                }
            } else {
                self.hunks.remove(&path);
                recoveries.push(RecoveryEntry { path, file: saved });
            }
        }
        let mut preserved = Vec::new();
        for entry in recoveries.into_iter().rev() {
            // A current file record wins over an older recovery, including null.
            if !matched.contains(&entry.path) {
                if let Some(current) = self.files.get_mut(&entry.path).filter(|current| {
                    has_draft_state(&entry.file) && compatible(current, &entry.file)
                }) {
                    let mut saved = entry.file;
                    super::worktree_state::retain_provenance(current, &mut saved);
                    current.replace_result(saved.result.take(), &mut self.marker_residue_count);
                    current.raw_result = saved.raw_result;
                    current.hunk_state = saved.hunk_state.take();
                    self.hunks.remove(&entry.path);
                    current.draft_state = true;
                    current.editing_origin = saved.editing_origin.take();
                    current.live_editing_origin = saved.live_editing_origin;
                    current.pending_edit = saved.pending_edit;
                    current.undo = std::mem::take(&mut saved.undo);
                    current.redo = std::mem::take(&mut saved.redo);
                    matched.insert(entry.path);
                    continue;
                }
            }
            // Do not retain a redundant historical copy of the adopted draft.
            if !self
                .files
                .get(&entry.path)
                .is_some_and(|current| same_draft(current, &entry.file))
            {
                preserved.push(entry);
            }
        }
        // Keep history oldest-to-newest so the latest displaced live file wins
        // if the repository later returns to its exact known context.
        for entry in preserved.into_iter().rev() {
            self.preserve_recovery(entry.path, entry.file);
        }
    }

    /// Merge disk recoveries by reference. Only the immediately previous file
    /// save from this live editing owner may coalesce without known identity.
    /// Prior editors and preserved recoveries remain inspection-only history.
    pub(super) fn autosave_json(
        &self,
        disk: Option<&Self>,
        snapshot: &draft_storage::Snapshot,
        changed: bool,
    ) -> Result<String, serde_json::Error> {
        let merged = self.merge_draft_files(disk, snapshot, changed);
        let mut recoveries: Vec<(&Path, &FileResolution)> = Vec::new();
        let mut pending = Vec::new();
        for entry in &self.recoveries {
            let record = (entry.path.as_path(), &entry.file);
            if entry.file.pending_edit.is_pending() {
                pending.push(record);
            } else {
                // Missing historical bodies precede the later disk snapshot.
                recoveries.push(record);
            }
        }
        if let Some(disk) = disk {
            for (path, file, preserved) in disk
                .recoveries
                .iter()
                .map(|entry| (entry.path.as_path(), &entry.file, true))
                .chain(
                    disk.files
                        .iter()
                        .map(|(path, file)| (path.as_path(), file, false)),
                )
            {
                if !has_draft_state(file) {
                    continue;
                }
                // Invalidation is irreversible for this exact saved body. Do not
                // copy its older, still-authoritative disk counterpart alongside
                // the inspection-only recovery and let reload readopt that copy.
                if !file.recovery_only
                    && self.recoveries.iter().any(|entry| {
                        entry.path == path
                            && entry.file.recovery_only
                            && same_draft_body(&entry.file, file)
                    })
                {
                    continue;
                }
                let superseded = self.files.get(path).is_some_and(|current| {
                    merged.matches_record(path, current, file)
                        || (!preserved && merged.supersedes(path, current, file, snapshot))
                });
                if !superseded {
                    if let Some(index) = recoveries
                        .iter()
                        .position(|(old_path, old)| *old_path == path && same_draft(old, file))
                    {
                        recoveries.remove(index);
                    }
                    recoveries.push((path, file));
                }
            }
        }
        // Only edits not yet saved may supersede the durable history's order.
        for (path, file) in pending {
            if let Some(index) = recoveries
                .iter()
                .position(|(old_path, old)| *old_path == path && same_draft(old, file))
            {
                recoveries.remove(index);
            }
            recoveries.push((path, file));
        }
        #[derive(serde::Serialize)]
        struct Record<'a> {
            repo: Cow<'a, str>,
            updated: u64,
            files: Vec<FileRecord<'a>>,
            recoveries: Vec<FileRecord<'a>>,
        }
        serde_json::to_string(&Record {
            repo: self.repo_path.to_string_lossy(),
            updated: now_unix(),
            files: self
                .files
                .iter()
                .map(|(path, file)| merged.record(path, file))
                .collect(),
            recoveries: recoveries
                .into_iter()
                .map(|(path, file)| file_record(path, file))
                .collect(),
        })
    }
}
