//! Resolution buffer + per-file undo / provenance + autosave — W26-CONFLICT-CORE
//! (T-CONFLICT-005, ADR-0057).
//!
//! The pure conflict hunk model lives in `kagi-domain` (ADR-0072). This backend
//! half keeps git2 materialization, autosave, and repository-index access.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use git2::{IndexEntry, Repository};

use super::GitError;
use kagi_domain::conflict_family::ConflictRevision;
use serde_json::Value;

pub use kagi_domain::resolution::{
    lines_to_text, text_to_lines, ConflictHunk, HunkChoice, HunkModel, LineOrder, LineOrigin,
    LineSelection, Region, ResolutionChoice, ResolvedLine, SelectionSide, TriState,
};

/// A byte-level resolution for a structurally-unmergeable conflict (#297):
/// binary blobs, symlinks (git mode 120000) and submodules / gitlinks (git mode
/// 160000).  These have no usable text buffer, so the chosen side is staged into
/// the index by its blob **OID + mode** directly (see
/// `stage_conflict_resolution`) rather than by writing lossy-UTF-8 text through
/// the working tree — which would corrupt binaries and, for a conflicted
/// symlink, dereference the on-disk link and write outside the repo (#298).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawResolution {
    /// The chosen side's blob (or gitlink commit) object id.
    pub oid: git2::Oid,
    /// The chosen side's git file mode (e.g. 0o100644, 0o100755, 0o120000,
    /// 0o160000).  Staged verbatim so the exec bit / symlink / gitlink survive.
    pub mode: u32,
}

/// A successful older snapshot must never acknowledge a newer keystroke.
#[derive(Debug, Default)]
struct EditGeneration {
    edited: AtomicU64,
    saved: AtomicU64,
}

#[derive(Debug, Default)]
struct PendingEdit {
    generation: Arc<EditGeneration>,
    captured: Option<u64>,
}

impl Clone for PendingEdit {
    fn clone(&self) -> Self {
        // An independent preservation clone is not a new user edit.
        Self::default()
    }
}

impl PendingEdit {
    fn mark(&self) {
        self.generation.edited.fetch_add(1, Ordering::Relaxed);
    }

    fn revision(&self) -> u64 {
        self.captured
            .unwrap_or_else(|| self.generation.edited.load(Ordering::Relaxed))
    }

    fn for_autosave(&self) -> Self {
        Self {
            generation: self.generation.clone(),
            captured: Some(self.revision()),
        }
    }

    fn mark_saved(&self) {
        self.generation
            .saved
            .fetch_max(self.revision(), Ordering::Relaxed);
    }

    fn is_pending(&self) -> bool {
        self.revision() > self.generation.saved.load(Ordering::Relaxed)
    }
}

/// The Result draft for a single file: the materialized side texts plus the
/// current resolution lines and an undo / redo history.
#[derive(Debug, Clone)]
struct FileResolution {
    /// Known operation + this file's exact unmerged stages, never path alone.
    identity: Option<ConflictRevision>,
    /// Worktree bytes observed alongside this file's source materialization.
    worktree_fingerprint: Option<String>,
    /// Presentation only; live validation always re-reads the path.
    worktree_changed: bool,
    /// Autosave producer provenance, never conflict identity or restore authority.
    /// A decoded origin belongs to a previous editor until the first live edit.
    editing_origin: Option<String>,
    live_editing_origin: bool,
    /// Transient recency only; decoded and cloned history never renews intent.
    pending_edit: PendingEdit,
    /// Explicit draft state, including undo-to-unresolved. A fresh index
    /// materialization must not supersede a saved draft merely by being null.
    draft_state: bool,
    /// Materialization rejected this draft; retain its context for inspection,
    /// never promote it back to live authority during a later re-detection.
    recovery_only: bool,
    /// Current side text (stage 2), `None` when that side is absent
    /// (modify-delete) or binary.
    current_text: Option<String>,
    /// Incoming side text (stage 3), `None` when absent or binary.
    incoming_text: Option<String>,
    /// Whether this file is a binary / non-text conflict (no line merge).
    binary: bool,
    /// Whether this conflict must be resolved by staging a raw blob OID rather
    /// than a text buffer (#297): binary, symlink (120000) or gitlink (160000).
    raw: bool,
    /// Current side blob OID + git mode (for the raw resolution path), when the
    /// current side is present.
    current_raw: Option<RawResolution>,
    /// Incoming side blob OID + git mode (for the raw resolution path), when the
    /// incoming side is present.
    incoming_raw: Option<RawResolution>,
    /// A chosen raw resolution (a whole side taken by OID).  Set by
    /// [`ResolutionBuffer::apply_choice`] for `raw` files instead of `result`.
    raw_result: Option<RawResolution>,
    /// The current Result lines (the editable draft).  `None` until a choice or
    /// manual edit produces a resolution.
    result: Option<Vec<ResolvedLine>>,
    /// Compact decisions retained with the draft across autosave and recovery.
    hunk_state: Option<hunk_state::HunkState>,
    /// Derived from this exact Result revision; not persisted or used by safety gates.
    marker_residue: bool,
    /// Undo stack: prior `result` states (most recent last).
    undo: Vec<Option<Vec<ResolvedLine>>>,
    /// Redo stack: states popped by undo (most recent last).
    redo: Vec<Option<Vec<ResolvedLine>>>,
}

impl FileResolution {
    fn empty() -> Self {
        FileResolution {
            identity: None,
            worktree_fingerprint: None,
            worktree_changed: false,
            editing_origin: None,
            live_editing_origin: false,
            pending_edit: PendingEdit::default(),
            draft_state: false,
            recovery_only: false,
            current_text: None,
            incoming_text: None,
            binary: false,
            raw: false,
            current_raw: None,
            incoming_raw: None,
            raw_result: None,
            result: None,
            hunk_state: None,
            marker_residue: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Move the complete editing history into recovery, retaining only live
    /// source/provenance facts on the current file. Draft bodies are not copied.
    fn take_draft_for_recovery(&mut self, marker_count: &mut usize) -> Self {
        let mut fresh = Self::empty();
        fresh.identity = self.identity.clone();
        fresh.worktree_fingerprint = self.worktree_fingerprint.clone();
        fresh.worktree_changed = self.worktree_changed;
        fresh.current_text = self.current_text.clone();
        fresh.incoming_text = self.incoming_text.clone();
        fresh.binary = self.binary;
        fresh.raw = self.raw;
        fresh.current_raw = self.current_raw;
        fresh.incoming_raw = self.incoming_raw;
        let mut saved = std::mem::replace(self, fresh);
        saved.recovery_only = true;
        *marker_count -= usize::from(saved.marker_residue);
        saved
    }

    /// Start a new preservation boundary after loading, but reuse the same
    /// producer token for every subsequent edit by this live owner.
    fn begin_edit(&mut self) {
        if !self.live_editing_origin {
            self.editing_origin = Some(crate::ops::backup::operation_id());
            self.live_editing_origin = true;
        }
        self.pending_edit.mark();
    }

    /// Push the current `result` onto the undo stack and clear redo, before a
    /// mutation.
    fn checkpoint(&mut self) {
        self.begin_edit();
        self.draft_state = true;
        self.undo.push(self.result.clone());
        self.redo.clear();
    }

    /// Publish a Result edit and its presentation verdict together. Undo/redo
    /// also pass here; unrelated files never need to be scanned again.
    fn replace_result(&mut self, result: Option<Vec<ResolvedLine>>, marker_count: &mut usize) {
        let residue = result.as_ref().is_some_and(|lines| {
            kagi_domain::resolution::resolved_lines_have_conflict_marker(lines)
        });
        *marker_count = *marker_count - usize::from(self.marker_residue) + usize::from(residue);
        self.result = result;
        self.marker_residue = residue;
    }
}

/// A damaged draft rescued during restore. The preserved bytes remain available
/// for manual recovery; the UI must announce this even though the base loaded.
#[derive(Debug)]
pub struct DraftRestoreWarning {
    pub rescued_path: PathBuf,
    pub recording_error: Option<String>,
}

/// The whole resolution buffer for one repository's conflict session.
///
/// Keyed by repository-relative path; serialized per ADR-0057 to
/// `~/.kagi/conflicts/<sha1(repo)>/buffer.json`.
#[derive(Debug)]
pub struct ResolutionBuffer {
    /// Absolute path to the repository working tree (the autosave key).
    repo_path: PathBuf,
    /// Frozen live operation and canonical git directory; never decoded authority.
    live_context: Option<(ConflictRevision, PathBuf)>,
    /// Per-file resolutions, ordered by path for deterministic serialization.
    files: BTreeMap<PathBuf, FileResolution>,
    /// Presentation-only aggregate, updated at each Result replacement.
    marker_residue_count: usize,
    /// Live source hunks, rebuilt from materialization. Only editing decisions
    /// and a materialization fingerprint are persisted with each file draft.
    hunks: BTreeMap<PathBuf, HunkModel>,
    /// Older, unbound or incompatible drafts in the same autosave document.
    recoveries: Vec<RecoveryEntry>,
    /// Last observed document and this owner's baseline for three-way draft merging.
    disk_snapshot: Arc<Mutex<Arc<draft_storage::Snapshot>>>,
    /// Preservation clones drain the same ordered background queue.
    draft_writer: Arc<draft_writer::Writer>,
    /// One-shot restore diagnostics, not inherited by preservation clones.
    draft_restore_warnings: Vec<DraftRestoreWarning>,
}

impl Clone for ResolutionBuffer {
    /// A snapshot can be edited independently, so it must not inherit the
    /// source's right to coalesce that live owner's unknown autosaves.
    fn clone(&self) -> Self {
        let mut files = self.files.clone();
        for file in files.values_mut() {
            file.live_editing_origin = false;
        }
        let mut recoveries = self.recoveries.clone();
        for entry in &mut recoveries {
            entry.file.live_editing_origin = false;
        }
        Self {
            repo_path: self.repo_path.clone(),
            live_context: self.live_context.clone(),
            files,
            marker_residue_count: self.marker_residue_count,
            hunks: self.hunks.clone(),
            recoveries,
            disk_snapshot: Arc::new(Mutex::new(
                self.disk_snapshot
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            )),
            draft_writer: self.draft_writer.clone(),
            draft_restore_warnings: Vec::new(),
        }
    }
}

mod conflict_boundary;
mod draft_merge;
mod draft_storage;
mod draft_writer;
pub use draft_writer::DraftSave;
#[cfg(test)]
mod draft_storage_tests;
mod drafts;
mod hunk_state;
mod materialize;
use materialize::materialize_zdiff3;
pub use materialize::ConflictMaterialization;
#[cfg(test)]
mod hunk_state_tests;
mod worktree_state;
use drafts::RecoveryEntry;
pub use drafts::{OwnedRecoveryDraft, RecoveryDraft};

// ────────────────────────────────────────────────────────────
// Construction / materialization
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Create an empty buffer for the repository at `repo_path`.
    pub fn new(repo_path: &Path) -> Self {
        ResolutionBuffer {
            repo_path: repo_path.to_path_buf(),
            live_context: None,
            files: BTreeMap::new(),
            marker_residue_count: 0,
            hunks: BTreeMap::new(),
            recoveries: Vec::new(),
            disk_snapshot: Arc::default(),
            draft_writer: Arc::default(),
            draft_restore_warnings: Vec::new(),
        }
    }

    /// Build a buffer from the repository's current index conflicts, materializing
    /// each file's current / incoming side texts via git2's `merge_file_from_index`
    /// with zdiff3 markers (falling back to standard markers, then to no
    /// materialization for binary / single-sided conflicts).
    ///
    /// No draft Result is set yet — the user picks a side or edits later.
    pub fn from_repo(repo: &Repository) -> Result<Self, GitError> {
        let workdir = repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;
        let mut buffer = ResolutionBuffer::new(workdir);

        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        index
            .read(true)
            .map_err(|error| GitError::Other(error.message().to_owned()))?;
        let context = crate::backend::conflict_ops::conflict_observe::draft_context(repo).and_then(
            |context| {
                let git_dir = repo.path().canonicalize().ok()?;
                Some((context, git_dir))
            },
        );
        buffer.live_context = context;
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;

        for entry in conflicts {
            let conflict = entry.map_err(|error| GitError::Other(error.message().to_owned()))?;
            let path = match conflict_path(&conflict) {
                Some(p) => p,
                None => continue,
            };

            let mut fr = FileResolution::empty();
            fr.identity = buffer.live_context.as_ref().map(|(context, git_dir)| {
                drafts::file_identity(git_dir, context, &path, &conflict)
            });
            fr.worktree_fingerprint = worktree_state::fingerprint(&workdir.join(&path)).ok();
            fr.current_text = blob_text(repo, conflict.our.as_ref());
            fr.incoming_text = blob_text(repo, conflict.their.as_ref());
            fr.binary = entry_is_binary(repo, conflict.our.as_ref())
                || entry_is_binary(repo, conflict.their.as_ref())
                || entry_is_binary(repo, conflict.ancestor.as_ref());
            // #297: structurally-unmergeable conflicts (binary / symlink /
            // gitlink) are resolved by staging a side's blob OID directly.
            // Record each present side's (oid, mode) and mark the file `raw` so
            // `apply_choice` routes to the byte-level path.
            fr.current_raw = side_raw(conflict.our.as_ref());
            fr.incoming_raw = side_raw(conflict.their.as_ref());
            fr.raw = fr.binary
                || is_special_mode(conflict.our.as_ref())
                || is_special_mode(conflict.their.as_ref())
                || is_special_mode(conflict.ancestor.as_ref());

            // Seed an initial draft for content conflicts: the zdiff3
            // materialization is informational (markers + base context); the
            // user resolves by choosing a side or editing. We do NOT auto-mark
            // it resolved (the markers would be residue), so `result` stays
            // None until a choice / edit.
            buffer.files.insert(path, fr);
        }

        Ok(buffer)
    }

    /// Build from the live index and restore only drafts bound to the exact
    /// operation and per-file stage identity. Legacy and mismatched drafts
    /// remain inspectable recovery entries, never current resolutions.
    pub fn from_repo_with_autosave(repo: &Repository) -> Result<Self, GitError> {
        let mut base = Self::from_repo(repo)?;
        if let Some(path) = buffer_file_path(&base.repo_path) {
            let (saved, warnings) = draft_storage::read_for_restore(&base.repo_path, &path)?;
            base.draft_restore_warnings = warnings;
            if let Some(saved) = saved {
                base.retain_drafts(saved.buffer);
                base.validate_hunk_states(Some(repo));
            }
        }
        {
            let mut snapshot = base.disk_snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if snapshot.hash.is_none() {
                *snapshot = Arc::new(draft_storage::Snapshot::capture(&base, None));
            }
        }
        Ok(base)
    }

    /// Consume restore diagnostics once; snapshot clones never replay notices.
    pub fn take_draft_restore_warnings(&mut self) -> Vec<DraftRestoreWarning> {
        std::mem::take(&mut self.draft_restore_warnings)
    }

    /// Revalidate every retained hunk draft before publishing file readiness.
    /// An unavailable materialization preserves the body without live authority.
    pub fn revalidate_hunk_states(&mut self) {
        let repo = Repository::open(&self.repo_path).ok();
        self.validate_hunk_states(repo.as_ref());
    }

    fn validate_hunk_states(&mut self, repo: Option<&Repository>) {
        let paths: Vec<_> = self
            .files
            .iter()
            .filter(|(_, file)| file.hunk_state.is_some())
            .map(|(path, _)| path.clone())
            .collect();
        for path in paths {
            let markers = repo.and_then(|repo| self.materialized_markers(repo, &path));
            // A retained model is not proof that its saved fingerprint matches
            // today's materialization. Restore decisions against a fresh model.
            self.hunks.remove(&path);
            if let Some(markers) = markers {
                self.ensure_hunks(&path, &markers.text, markers.marker_width);
            } else if let Some(file) = self.files.get_mut(&path) {
                let saved = file.take_draft_for_recovery(&mut self.marker_residue_count);
                self.preserve_recovery(path, saved);
            }
        }
    }

    /// Return the zdiff3 (or standard fallback) materialized marker text for a
    /// file, for the UI to show inside the 3-way view.  `None` when the file is
    /// not in the buffer or has no usable text merge.
    pub fn materialized_markers(
        &self,
        repo: &Repository,
        path: &Path,
    ) -> Option<ConflictMaterialization> {
        let fr = self.files.get(path)?;
        if fr.raw {
            return None;
        }
        materialize_zdiff3(repo, path)
    }
}

// ────────────────────────────────────────────────────────────
// Choices / manual edits / undo / redo
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Apply a side choice to `path`, replacing its Result draft.  Pushes the
    /// prior state onto the file's undo stack.
    ///
    /// Returns an error if `path` is not a tracked conflict, or if the chosen
    /// side is absent (e.g. choosing `Incoming` on a modify-delete where the
    /// incoming side was deleted).
    pub fn apply_choice(&mut self, path: &Path, choice: ResolutionChoice) -> Result<(), GitError> {
        let fr = self.files.get_mut(path).ok_or_else(|| {
            GitError::Other(format!("not a tracked conflict: {}", path.display()))
        })?;

        // #297: binary / symlink / gitlink conflicts have no text buffer — the
        // chosen side is recorded as a raw blob OID + mode and staged directly.
        // "Take current" / "Take incoming" map through here unchanged; the
        // Both* combinations are meaningless for an unmergeable blob.
        if fr.raw {
            let raw = match choice {
                ResolutionChoice::Current => fr
                    .current_raw
                    .ok_or_else(|| missing_side(path, "current"))?,
                ResolutionChoice::Incoming => fr
                    .incoming_raw
                    .ok_or_else(|| missing_side(path, "incoming"))?,
                ResolutionChoice::BothCurrentFirst | ResolutionChoice::BothIncomingFirst => {
                    return Err(GitError::Other(format!(
                        "cannot keep both sides of {}: a binary, symlink or submodule \
                         conflict must take exactly one side",
                        path.display()
                    )));
                }
            };
            fr.checkpoint();
            fr.raw_result = Some(raw);
            fr.replace_result(None, &mut self.marker_residue_count);
            return Ok(());
        }

        let current = fr.current_text.clone();
        let incoming = fr.incoming_text.clone();

        let lines: Vec<ResolvedLine> = match choice {
            ResolutionChoice::Current => side_lines(&current, LineOrigin::Current)
                .ok_or_else(|| missing_side(path, "current"))?,
            ResolutionChoice::Incoming => side_lines(&incoming, LineOrigin::Incoming)
                .ok_or_else(|| missing_side(path, "incoming"))?,
            ResolutionChoice::BothCurrentFirst => {
                let mut v = side_lines(&current, LineOrigin::Current)
                    .ok_or_else(|| missing_side(path, "current"))?;
                v.extend(
                    side_lines(&incoming, LineOrigin::Incoming)
                        .ok_or_else(|| missing_side(path, "incoming"))?,
                );
                v
            }
            ResolutionChoice::BothIncomingFirst => {
                let mut v = side_lines(&incoming, LineOrigin::Incoming)
                    .ok_or_else(|| missing_side(path, "incoming"))?;
                v.extend(
                    side_lines(&current, LineOrigin::Current)
                        .ok_or_else(|| missing_side(path, "current"))?,
                );
                v
            }
        };

        fr.checkpoint();
        fr.replace_result(Some(lines), &mut self.marker_residue_count);
        Ok(())
    }

    /// Replace `path`'s Result draft with hand-edited `text`.  Each line is
    /// recorded with [`LineOrigin::Manual`] provenance.  Pushes the prior state
    /// onto the undo stack.
    pub fn set_manual_text(&mut self, path: &Path, text: &str) -> Result<(), GitError> {
        let fr = self.files.get_mut(path).ok_or_else(|| {
            GitError::Other(format!("not a tracked conflict: {}", path.display()))
        })?;
        let lines = text_to_lines(text, LineOrigin::Manual);
        fr.checkpoint();
        fr.replace_result(Some(lines), &mut self.marker_residue_count);
        Ok(())
    }

    /// Undo the last choice / edit for `path`.  Returns `true` if a state was
    /// restored, `false` if the undo stack was empty.
    pub fn undo(&mut self, path: &Path) -> bool {
        match self.files.get_mut(path) {
            Some(fr) => match fr.undo.pop() {
                Some(prev) => {
                    fr.begin_edit();
                    fr.redo.push(fr.result.clone());
                    fr.replace_result(prev, &mut self.marker_residue_count);
                    true
                }
                None => false,
            },
            None => false,
        }
    }

    /// Redo the last undone choice / edit for `path`.  Returns `true` if a state
    /// was re-applied.
    pub fn redo(&mut self, path: &Path) -> bool {
        match self.files.get_mut(path) {
            Some(fr) => match fr.redo.pop() {
                Some(next) => {
                    fr.begin_edit();
                    fr.undo.push(fr.result.clone());
                    fr.replace_result(next, &mut self.marker_residue_count);
                    true
                }
                None => false,
            },
            None => false,
        }
    }
}

// ────────────────────────────────────────────────────────────
// Hunk-level editing (W32-CONFLICT-EDITOR)
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Ensure a [`HunkModel`] exists for `path`, decomposing it from the supplied
    /// `marker_text` and its collision-free `marker_width` on first use.
    ///
    /// Idempotent: if a model already exists (e.g. the user has been editing
    /// hunks), it is preserved so per-hunk choices are not lost.  Returns `false`
    /// if `path` is not a tracked text conflict.
    /// Mismatched saved hunk authority preserves the exact draft as recovery
    /// and clears the live resolution before exposing the new source model.
    pub fn ensure_hunks(&mut self, path: &Path, marker_text: &str, marker_width: usize) -> bool {
        let Some(file) = self.files.get_mut(path).filter(|file| !file.raw) else {
            return false;
        };
        if !self.hunks.contains_key(path) {
            let mut model = HunkModel::from_marker_text_with_width(marker_text, marker_width);
            // Decoding alone grants no authority. retain_drafts first binds the
            // decisions to a live file with the same known stage identity.
            let restore_failed = self.live_context.is_some()
                && file
                    .hunk_state
                    .as_ref()
                    .is_some_and(|state| !state.restore(marker_text, &mut model));
            let recovery = restore_failed
                .then(|| file.take_draft_for_recovery(&mut self.marker_residue_count));
            file.hunk_state = Some(hunk_state::HunkState::new(marker_text, &model));
            self.hunks.insert(path.to_path_buf(), model);
            if let Some(saved) = recovery {
                self.preserve_recovery(path.to_path_buf(), saved);
            }
        }
        true
    }

    /// The hunk model for `path`, if one has been built via [`Self::ensure_hunks`].
    pub fn hunk_model(&self, path: &Path) -> Option<&HunkModel> {
        self.hunks.get(path)
    }

    /// Apply a per-hunk choice to the `n`-th hunk of `path`, then re-assemble and
    /// commit the file Result (pushing the prior Result onto the undo stack, so
    /// file-level undo still works).  Returns `true` on success.
    ///
    /// The repository is untouched; the caller autosaves (in-memory first).
    pub fn apply_hunk_choice(
        &mut self,
        path: &Path,
        hunk_index: usize,
        choice: HunkChoice,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_choice(hunk_index, choice) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    fn commit_hunk_model(&mut self, path: &Path) -> bool {
        let Some(model) = self.hunks.get(path) else {
            return false;
        };
        let assembled = model.assemble();
        if let Some(fr) = self.files.get_mut(path) {
            fr.checkpoint();
            if let Some(state) = &mut fr.hunk_state {
                state.update(model);
            }
            fr.replace_result(Some(assembled), &mut self.marker_residue_count);
            true
        } else {
            false
        }
    }

    /// Set every line checkbox for one file side, then re-assemble the Result.
    pub fn set_file_side_selection(
        &mut self,
        path: &Path,
        side: SelectionSide,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        model.set_file_side(side, taken);
        self.commit_hunk_model(path)
    }

    /// Set every line checkbox for one hunk side, then re-assemble the Result.
    pub fn set_hunk_side_selection(
        &mut self,
        path: &Path,
        hunk_index: usize,
        side: SelectionSide,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_side(hunk_index, side, taken) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Set one line checkbox, then re-assemble the Result.
    pub fn set_hunk_line_selection(
        &mut self,
        path: &Path,
        hunk_index: usize,
        side: SelectionSide,
        line_index: usize,
        taken: bool,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_line(hunk_index, side, line_index, taken) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Reorder adopted lines and re-assemble only when the order actually changes.
    pub fn set_hunk_line_order(
        &mut self,
        path: &Path,
        hunk_index: usize,
        order: LineOrder,
    ) -> bool {
        let Some(model) = self.hunks.get_mut(path) else {
            return false;
        };
        if !model.set_hunk_line_order(hunk_index, order) {
            return false;
        }
        self.commit_hunk_model(path)
    }

    /// Reset the `n`-th hunk of `path` to [`HunkChoice::Unresolved`] and
    /// re-assemble.  Convenience wrapper over [`Self::apply_hunk_choice`].
    pub fn reset_hunk(&mut self, path: &Path, hunk_index: usize) -> bool {
        self.apply_hunk_choice(path, hunk_index, HunkChoice::Unresolved)
    }

    /// Number of conflict hunks in `path`'s model (0 if not built / not tracked).
    pub fn hunk_count(&self, path: &Path) -> usize {
        self.hunks.get(path).map(|m| m.hunk_count()).unwrap_or(0)
    }

    /// Whether every hunk of `path` is resolved (false if the model is absent).
    pub fn hunks_all_resolved(&self, path: &Path) -> bool {
        self.hunks
            .get(path)
            .map(|m| m.all_resolved())
            .unwrap_or(false)
    }
}

// ────────────────────────────────────────────────────────────
// Queries
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Fast presentation verdict; Save/Continue must still use fresh validation.
    pub fn has_cached_marker_residue(&self) -> bool {
        self.marker_residue_count != 0
    }

    /// Cached verdict for a file's current Result; unknown/unresolved is false.
    pub fn cached_marker_residue(&self, path: &Path) -> bool {
        self.files.get(path).is_some_and(|file| file.marker_residue)
    }

    /// Fresh provenance-aware marker verdict for one authoritative Result.
    pub fn has_marker_residue(&self, path: &Path) -> bool {
        self.files
            .get(path)
            .and_then(|file| file.result.as_deref())
            .is_some_and(kagi_domain::resolution::resolved_lines_have_conflict_marker)
    }

    /// Whether the authoritative Result would be replaced by adopting source
    /// hunks. Empty manual Results count; identical bytes with new provenance do
    /// not. This presentation query neither allocates nor changes either model.
    pub fn result_differs_from_hunks(&self, path: &Path) -> bool {
        let Some(result) = self.files.get(path).and_then(|file| file.result.as_deref()) else {
            return false;
        };
        self.hunks
            .get(path)
            .is_some_and(|model| !model.matches_result_text(result))
    }

    /// Whether `path` has a Result draft (a side was chosen or text edited).
    /// A raw (binary / symlink / gitlink) file counts as resolved once a side is
    /// chosen via [`Self::apply_choice`] (#297).
    pub fn has_resolution(&self, path: &Path) -> bool {
        self.files
            .get(path)
            .map(|fr| fr.result.is_some() || fr.raw_result.is_some())
            .unwrap_or(false)
    }

    /// The chosen raw (byte-level) resolution for `path`, if this is a binary /
    /// symlink / gitlink conflict whose side has been chosen (#297).  The
    /// staging path uses this to add the OID at stage 0 directly.
    pub fn raw_resolution(&self, path: &Path) -> Option<RawResolution> {
        self.files.get(path).and_then(|fr| fr.raw_result)
    }

    /// The absolute working-tree path this buffer was built for (#321 viewer:
    /// lets the UI re-open the backend for blob bytes without a git2 dependency).
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Whether `path` is a raw (binary / symlink / gitlink) conflict.
    pub fn is_raw(&self, path: &Path) -> bool {
        self.files.get(path).map(|fr| fr.raw).unwrap_or(false)
    }

    /// The (oid, mode) of one side of a raw conflict, or `None` when the file is
    /// not tracked or that side is absent (modify-delete). (#321 viewer.)
    fn side_raw_for(&self, path: &Path, side: SelectionSide) -> Option<RawResolution> {
        let fr = self.files.get(path)?;
        match side {
            SelectionSide::Current => fr.current_raw,
            SelectionSide::Incoming => fr.incoming_raw,
        }
    }

    /// Frozen (OID string, Git mode) captured when this buffer was loaded.
    /// No repository access: use with `FileReadSource::ConflictStage` for worker
    /// preparation without rereading a subsequently changed conflict index.
    pub fn side_raw_meta(&self, path: &Path, side: SelectionSide) -> Option<(String, u32)> {
        let raw = self.side_raw_for(path, side)?;
        Some((raw.oid.to_string(), raw.mode))
    }

    /// The resolved text for `path` (lines joined with `\n`, trailing newline
    /// appended), or `None` if unresolved / unknown.
    pub fn resolved_text(&self, path: &Path) -> Option<String> {
        let fr = self.files.get(path)?;
        let lines = fr.result.as_ref()?;
        Some(lines_to_text(lines))
    }

    /// The per-line provenance of `path`'s Result draft, or `None` if unresolved.
    pub fn provenance(&self, path: &Path) -> Option<Vec<LineOrigin>> {
        let fr = self.files.get(path)?;
        let lines = fr.result.as_ref()?;
        Some(lines.iter().map(|l| l.origin).collect())
    }

    /// The current / incoming materialized side texts of `path` (for the UI's
    /// 3-way view), or `None` when the file is not tracked.
    pub fn sides(&self, path: &Path) -> Option<(Option<String>, Option<String>)> {
        self.files
            .get(path)
            .map(|fr| (fr.current_text.clone(), fr.incoming_text.clone()))
    }

    /// Paths whose Results contain manual/synthetic conflict markers.
    /// Chosen source and passthrough text is not marker residue (ADR-0057).
    /// Unresolved files without a Result are excluded.
    pub fn files_with_marker_residue(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for (path, fr) in &self.files {
            if let Some(lines) = &fr.result {
                if kagi_domain::resolution::resolved_lines_have_conflict_marker(lines) {
                    out.push(path.clone());
                }
            }
        }
        out
    }
}

// ────────────────────────────────────────────────────────────
// Autosave / load
// ────────────────────────────────────────────────────────────

impl ResolutionBuffer {
    /// Autosave the buffer under `~/.kagi/conflicts/<sha1(repo)>/buffer.json`
    /// (or `$KAGI_LOG_DIR/conflicts/...` in tests).  Returns the file path.
    ///
    /// Files with no Result draft are still recorded (so their materialized side
    /// texts survive a restart), but the format is compact.
    pub fn autosave(&self) -> Result<PathBuf, GitError> {
        self.draft_writer.autosave(self)
    }

    /// The writer's actual durable transaction, never queued recursively.
    fn autosave_now(&self) -> Result<PathBuf, GitError> {
        let path = buffer_file_path(&self.repo_path).ok_or_else(|| {
            GitError::Other(
                "conflicts: could not determine autosave dir (no HOME or KAGI_LOG_DIR)".to_string(),
            )
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                GitError::Other(format!(
                    "conflicts: mkdir failed for {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        self.save_draft_document(&path)?;
        for file in self
            .files
            .values()
            .chain(self.recoveries.iter().map(|entry| &entry.file))
        {
            file.pending_edit.mark_saved();
        }
        Ok(path)
    }

    /// Load a previously autosaved buffer, recovering a torn document from `.prev`.
    /// Compatibility inspection accessor: unrecoverable errors return `None`.
    /// Production restore and all writers use the checked reader and propagate
    /// errors instead of treating an undecodable document as an empty merge base.
    pub fn load(repo_path: &Path) -> Option<ResolutionBuffer> {
        let path = buffer_file_path(repo_path)?;
        draft_storage::read(repo_path, &path)
            .ok()
            .flatten()
            .map(|disk| disk.buffer)
    }

    /// Explicitly discard current drafts, retaining older recovery bodies.
    ///
    /// Do not call this from operation completion: expired identities already
    /// prevent automatic reuse, and their durable bodies must remain inspectable.
    pub fn clear(repo_path: &Path) -> Result<(), GitError> {
        let path = match buffer_file_path(repo_path) {
            Some(p) => p,
            None => return Ok(()),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| GitError::Other(format!("conflicts: draft directory: {e}")))?;
        }
        let _lock = draft_storage::lock(&path)?;
        if let Some(disk) = draft_storage::read(repo_path, &path)? {
            let mut saved = disk.buffer;
            if !saved.recoveries.is_empty() {
                // Discarding current drafts must not let an older preserved body
                // override a previously explicit undo-to-null.
                for (file_path, file) in std::mem::take(&mut saved.files) {
                    if file.draft_state && file.result.is_none() && file.raw_result.is_none() {
                        saved.preserve_recovery(file_path, file);
                    }
                }
                saved.hunks.clear();
                saved.marker_residue_count = 0;
                let json = saved
                    .to_json()
                    .map_err(|e| GitError::Other(format!("serialize conflict recovery: {e}")))?;
                return draft_storage::replace(&path, json.as_bytes(), None);
            }
        }
        let previous = draft_storage::sibling(&path, ".prev");
        match std::fs::remove_file(&previous) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(GitError::Other(format!("conflicts: delete backup: {e}"))),
        }
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(GitError::Other(format!(
                "conflicts: delete failed for {}: {}",
                path.display(),
                e
            ))),
        }
    }

    /// Serialize the buffer to a single JSON object string.
    ///
    /// Schema (one line):
    /// `{"repo":"<path>","updated":<u64>,"files":[{...},...]}`
    /// where each file is
    /// `{"path":"<p>","binary":<bool>,"current":<str|null>,"incoming":<str|null>,`
    /// `"result":[{"t":"<line>","o":"c|i|m"},...]|null,"raw_result":{"oid":"<hex>","mode":<u32>}|null}`.
    fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&BufferRecord {
            repo: self.repo_path.to_string_lossy(),
            updated: now_unix(),
            files: &self.files,
            recoveries: &self.recoveries,
        })
    }
}

// ────────────────────────────────────────────────────────────
// Index / blob helpers
// ────────────────────────────────────────────────────────────

/// Extract a conflict's path from whichever stage entry is present.
fn conflict_path(conflict: &git2::IndexConflict) -> Option<PathBuf> {
    let bytes = conflict
        .our
        .as_ref()
        .or(conflict.their.as_ref())
        .or(conflict.ancestor.as_ref())
        .map(|e| e.path.clone())?;
    // #293: byte-faithful — a lossy path drives the resolution write path
    // (`workdir.join(path)`), so a non-UTF-8 name would write a bogus renamed
    // file and leave the real conflict unresolved. Unix uses the raw bytes;
    // non-Unix skips a non-UTF-8 name (out of scope, #293).
    if let Ok(s) = std::str::from_utf8(&bytes) {
        return Some(PathBuf::from(s));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Read an index entry's blob as decoded text, or `None` for a missing /
/// binary / unreadable blob.
fn blob_text(repo: &Repository, entry: Option<&IndexEntry>) -> Option<String> {
    let entry = entry?;
    if entry.id.is_zero() {
        return None;
    }
    let blob = repo.find_blob(entry.id).ok()?;
    if blob.is_binary() {
        return None;
    }
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// Git file modes that have no text merge and must be resolved by staging a raw
/// OID (#297): symlink and gitlink / submodule.  (Regular blobs are 100644 /
/// 100755; trees are 040000.)
const MODE_SYMLINK: u32 = 0o120000;
const MODE_GITLINK: u32 = 0o160000;

/// Whether an index entry carries a symlink or gitlink mode.
fn is_special_mode(entry: Option<&IndexEntry>) -> bool {
    matches!(
        entry.map(|e| e.mode),
        Some(MODE_SYMLINK) | Some(MODE_GITLINK)
    )
}

/// The (oid, mode) of a present index-entry side, for the raw resolution path.
fn side_raw(entry: Option<&IndexEntry>) -> Option<RawResolution> {
    let e = entry?;
    if e.id.is_zero() {
        return None;
    }
    Some(RawResolution {
        oid: e.id,
        mode: e.mode,
    })
}

/// Whether an index entry's blob is binary.
fn entry_is_binary(repo: &Repository, entry: Option<&IndexEntry>) -> bool {
    let entry = match entry {
        Some(e) => e,
        None => return false,
    };
    if entry.id.is_zero() {
        return false;
    }
    match repo.find_blob(entry.id) {
        Ok(blob) => {
            let content = blob.content();
            let probe = &content[..content.len().min(8 * 1024)];
            blob.is_binary() || probe.contains(&0u8)
        }
        Err(_) => false,
    }
}

// ────────────────────────────────────────────────────────────
// Line helpers (chars()-safe; split on '\n' over &str)
// ────────────────────────────────────────────────────────────

/// Convert an optional side text into resolution lines with the given origin,
/// or `None` when the side is absent.
fn side_lines(text: &Option<String>, origin: LineOrigin) -> Option<Vec<ResolvedLine>> {
    text.as_ref().map(|t| text_to_lines(t, origin))
}

/// Error for a missing side on a choice.
fn missing_side(path: &Path, side: &str) -> GitError {
    GitError::Other(format!(
        "cannot take {} side of {}: that side does not exist (modify/delete or binary)",
        side,
        path.display()
    ))
}

// ────────────────────────────────────────────────────────────
// Path resolution (mirrors drafts.rs)
// ────────────────────────────────────────────────────────────

/// Resolve the conflicts autosave directory:
/// 1. `$KAGI_LOG_DIR/conflicts/` when set and non-empty.
/// 2. `$HOME/.kagi/conflicts/` otherwise.
fn conflicts_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("conflicts"));
        }
    }
    dirs_home().map(|home| home.join(".kagi").join("conflicts"))
}

/// `$HOME` then `$USERPROFILE`.
fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// `<conflicts_dir>/<sha1(repo_path)>/buffer.json`.
///
/// The key is normalized so that two spellings of the same repository hash
/// identically: it is canonicalized when the path exists (resolving symlinks
/// such as macOS's `/var` → `/private/var` and any `repo.workdir()` trailing
/// slash) and otherwise falls back to the trailing-separator-stripped string.
fn buffer_file_path(repo_path: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(repo_path).ok();
    let key_owned = match &canonical {
        Some(p) => p.to_string_lossy().into_owned(),
        None => repo_path
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string(),
    };
    let hash = crate::drafts::sha1_hex(key_owned.as_bytes());
    conflicts_dir().map(|dir| dir.join(hash).join("buffer.json"))
}

/// Current wall-clock time in Unix epoch seconds (0 on clock error).
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// JSON records
// ────────────────────────────────────────────────────────────

// Borrow the persisted data without cloning draft text or serializing live-index
// metadata, source hunk data, or undo history. Domain types remain serde-free.
#[derive(serde::Serialize)]
struct BufferRecord<'a> {
    repo: Cow<'a, str>,
    updated: u64,
    #[serde(serialize_with = "serialize_files")]
    files: &'a BTreeMap<PathBuf, FileResolution>,
    #[serde(serialize_with = "serialize_recoveries")]
    recoveries: &'a [RecoveryEntry],
}

#[derive(serde::Serialize)]
struct FileRecord<'a> {
    path: Cow<'a, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path_bytes: Option<&'a [u8]>,
    identity: Option<IdentityRecord<'a>>,
    editing_origin: Option<&'a str>,
    draft_state: bool,
    recovery_only: bool,
    binary: bool,
    current: Option<&'a str>,
    incoming: Option<&'a str>,
    #[serde(serialize_with = "serialize_result")]
    result: &'a Option<Vec<ResolvedLine>>,
    raw_result: Option<RawRecord<'a>>,
    hunk_state: &'a Option<hunk_state::HunkState>,
    worktree_fingerprint: &'a Option<String>,
}

#[derive(serde::Serialize)]
struct IdentityRecord<'a> {
    version: u8,
    fingerprint: &'a str,
}

#[derive(serde::Serialize)]
struct LineRecord<'a> {
    t: &'a str,
    o: char,
}

#[derive(serde::Serialize)]
struct RawRecord<'a> {
    #[serde(serialize_with = "serialize_oid")]
    oid: &'a git2::Oid,
    mode: u32,
}

fn serialize_files<S: serde::Serializer>(
    files: &BTreeMap<PathBuf, FileResolution>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(files.iter().map(|(path, fr)| file_record(path, fr)))
}

fn serialize_recoveries<S: serde::Serializer>(
    recoveries: &[RecoveryEntry],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(
        recoveries
            .iter()
            .map(|entry| file_record(&entry.path, &entry.file)),
    )
}

fn file_record<'a>(path: &'a Path, fr: &'a FileResolution) -> FileRecord<'a> {
    FileRecord {
        path: path.to_string_lossy(),
        path_bytes: path
            .to_str()
            .is_none()
            .then_some(path.as_os_str().as_encoded_bytes()),
        identity: fr.identity.as_ref().map(|identity| IdentityRecord {
            version: 1,
            fingerprint: identity.as_str(),
        }),
        editing_origin: fr.editing_origin.as_deref(),
        draft_state: fr.draft_state || fr.result.is_some() || fr.raw_result.is_some(),
        recovery_only: fr.recovery_only,
        binary: fr.binary,
        current: fr.current_text.as_deref(),
        incoming: fr.incoming_text.as_deref(),
        result: &fr.result,
        hunk_state: &fr.hunk_state,
        worktree_fingerprint: &fr.worktree_fingerprint,
        raw_result: fr.raw_result.as_ref().map(|raw| RawRecord {
            oid: &raw.oid,
            mode: raw.mode,
        }),
    }
}

fn serialize_result<S: serde::Serializer>(
    result: &Option<Vec<ResolvedLine>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match result {
        Some(lines) => serializer.collect_seq(lines.iter().map(|line| LineRecord {
            t: &line.text,
            o: line.origin.tag(),
        })),
        None => serializer.serialize_none(),
    }
}

fn serialize_oid<S: serde::Serializer>(oid: &git2::Oid, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(oid)
}

// Decode entries independently to retain the legacy defaults and per-file recovery.
#[derive(serde::Deserialize)]
struct SavedBuffer {
    files: Vec<Value>,
    #[serde(default)]
    recoveries: Vec<Value>,
}

fn parse_buffer_json(repo_path: &Path, json: &str) -> Option<ResolutionBuffer> {
    // Serde structs also accept sequences; the persisted record must be an object.
    if !json.trim_start().starts_with('{') {
        return None;
    }
    let saved: SavedBuffer = serde_json::from_str(json).ok()?;
    let mut buffer = ResolutionBuffer::new(repo_path);
    for file in saved.files {
        if let Some((path, resolution)) = parse_file_object(file, &mut buffer.marker_residue_count)
        {
            if let Some(previous) = buffer.files.insert(path.clone(), resolution) {
                buffer.marker_residue_count -= usize::from(previous.marker_residue);
                buffer.preserve_recovery(path, previous);
            }
        }
    }
    for file in saved.recoveries {
        let mut ignored_marker_count = 0;
        if let Some((path, resolution)) = parse_file_object(file, &mut ignored_marker_count) {
            buffer.preserve_recovery(path, resolution);
        }
    }
    Some(buffer)
}

fn parse_file_object(value: Value, marker_count: &mut usize) -> Option<(PathBuf, FileResolution)> {
    let Value::Object(mut fields) = value else {
        return None;
    };
    let path = drafts::parse_path(fields.remove("path_bytes"), fields.remove("path"))?;
    let result = parse_result_array(fields.remove("result"));
    let raw_result = parse_raw_result(fields.remove("raw_result"));
    let draft_state = fields
        .get("draft_state")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || result.is_some()
        || raw_result.is_some();
    let mut resolution = FileResolution {
        identity: drafts::parse_identity(fields.remove("identity")),
        worktree_fingerprint: json_string(fields.remove("worktree_fingerprint")),
        worktree_changed: false,
        editing_origin: json_string(fields.remove("editing_origin"))
            .filter(|origin| !origin.is_empty()),
        live_editing_origin: false,
        pending_edit: PendingEdit::default(),
        draft_state,
        recovery_only: !matches!(
            fields.remove("recovery_only"),
            None | Some(Value::Bool(false))
        ),
        binary: fields
            .get("binary")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        current_text: json_string(fields.remove("current")),
        incoming_text: json_string(fields.remove("incoming")),
        result: None,
        hunk_state: fields
            .remove("hunk_state")
            .and_then(|value| serde_json::from_value(value).ok()),
        marker_residue: false,
        raw_result,
        raw: false,
        current_raw: None,
        incoming_raw: None,
        undo: Vec::new(),
        redo: Vec::new(),
    };
    resolution.replace_result(result, marker_count);
    Some((path, resolution))
}

fn json_string(value: Option<Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text),
        _ => None,
    }
}

fn parse_raw_result(value: Option<Value>) -> Option<RawResolution> {
    let value = value?;
    Some(RawResolution {
        oid: git2::Oid::from_str(value.get("oid")?.as_str()?).ok()?,
        mode: u32::try_from(value.get("mode")?.as_u64()?).ok()?,
    })
}

fn parse_result_array(value: Option<Value>) -> Option<Vec<ResolvedLine>> {
    let Value::Array(lines) = value? else {
        return None;
    };
    Some(
        lines
            .into_iter()
            .filter_map(|line| {
                let Value::Object(mut fields) = line else {
                    return None;
                };
                let origin = fields
                    .get("o")
                    .and_then(Value::as_str)
                    .and_then(|tag| tag.chars().next())
                    .map(LineOrigin::from_tag)
                    .unwrap_or(LineOrigin::Manual);
                Some(ResolvedLine {
                    text: json_string(fields.remove("t")).unwrap_or_default(),
                    origin,
                })
            })
            .collect(),
    )
}

// ────────────────────────────────────────────────────────────
// Unit tests (pure logic; repo-backed behaviour in tests/conflicts_test.rs)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "resolution/order_tests.rs"]
mod order_tests;

#[cfg(test)]
#[path = "resolution/draft_tests.rs"]
mod draft_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn buf_with_sides(current: &str, incoming: &str) -> (ResolutionBuffer, PathBuf) {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/repo"));
        let path = PathBuf::from("a.txt");
        let fr = FileResolution {
            identity: None,
            editing_origin: None,
            live_editing_origin: false,
            pending_edit: PendingEdit::default(),
            draft_state: false,
            recovery_only: false,
            current_text: Some(current.to_string()),
            incoming_text: Some(incoming.to_string()),
            binary: false,
            raw: false,
            current_raw: None,
            incoming_raw: None,
            raw_result: None,
            result: None,
            hunk_state: None,
            worktree_fingerprint: None,
            worktree_changed: false,
            marker_residue: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(path.clone(), fr);
        (b, path)
    }

    #[test]
    fn choice_current_and_incoming() {
        let (mut b, p) = buf_with_sides("current line\n", "incoming line\n");
        b.apply_choice(&p, ResolutionChoice::Current).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "current line\n");
        assert_eq!(b.provenance(&p).unwrap(), vec![LineOrigin::Current]);

        b.apply_choice(&p, ResolutionChoice::Incoming).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "incoming line\n");
        assert_eq!(b.provenance(&p).unwrap(), vec![LineOrigin::Incoming]);
    }

    #[test]
    fn choice_both_orders() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.apply_choice(&p, ResolutionChoice::BothCurrentFirst)
            .unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "A\nB\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Current, LineOrigin::Incoming]
        );

        b.apply_choice(&p, ResolutionChoice::BothIncomingFirst)
            .unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "B\nA\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Incoming, LineOrigin::Current]
        );
    }

    #[test]
    fn manual_text_and_provenance() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.set_manual_text(&p, "hand\nedited\n").unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "hand\nedited\n");
        assert_eq!(
            b.provenance(&p).unwrap(),
            vec![LineOrigin::Manual, LineOrigin::Manual]
        );
    }

    #[test]
    fn undo_redo_round_trip() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        assert!(!b.has_resolution(&p));
        b.apply_choice(&p, ResolutionChoice::Current).unwrap();
        b.apply_choice(&p, ResolutionChoice::Incoming).unwrap();
        assert_eq!(b.resolved_text(&p).unwrap(), "B\n");

        assert!(b.undo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "A\n");
        assert!(b.undo(&p));
        assert!(!b.has_resolution(&p)); // back to no resolution
        assert!(!b.undo(&p)); // stack empty

        assert!(b.redo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "A\n");
        assert!(b.redo(&p));
        assert_eq!(b.resolved_text(&p).unwrap(), "B\n");
        assert!(!b.redo(&p));
    }

    #[test]
    fn marker_residue_detected_in_manual_edit() {
        let (mut b, p) = buf_with_sides("A\n", "B\n");
        b.set_manual_text(&p, "<<<<<<< HEAD\nA\n=======\nB\n>>>>>>> other\n")
            .unwrap();
        let residue = b.files_with_marker_residue();
        assert_eq!(residue, vec![p.clone()]);

        // Clean resolution has no residue.
        b.set_manual_text(&p, "A and B\n").unwrap();
        assert!(b.files_with_marker_residue().is_empty());
    }

    #[test]
    fn missing_side_choice_errors() {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/repo"));
        let p = PathBuf::from("md.txt");
        let fr = FileResolution {
            identity: None,
            editing_origin: None,
            live_editing_origin: false,
            pending_edit: PendingEdit::default(),
            draft_state: false,
            recovery_only: false,
            current_text: Some("only current\n".to_string()),
            incoming_text: None, // modify/delete: incoming deleted
            binary: false,
            raw: false,
            current_raw: None,
            incoming_raw: None,
            raw_result: None,
            result: None,
            hunk_state: None,
            worktree_fingerprint: None,
            worktree_changed: false,
            marker_residue: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(p.clone(), fr);
        assert!(b.apply_choice(&p, ResolutionChoice::Current).is_ok());
        assert!(b.apply_choice(&p, ResolutionChoice::Incoming).is_err());
    }

    #[test]
    fn json_round_trips_result_and_provenance() {
        let (mut b, p) = buf_with_sides("A\nC\n", "B\n");
        b.apply_choice(&p, ResolutionChoice::BothCurrentFirst)
            .unwrap();
        let json = b.to_json().expect("encode buffer");
        let parsed = parse_buffer_json(Path::new("/tmp/repo"), &json).expect("parse");
        assert_eq!(parsed.resolved_text(&p).unwrap(), "A\nC\nB\n");
        assert_eq!(
            parsed.provenance(&p).unwrap(),
            vec![
                LineOrigin::Current,
                LineOrigin::Current,
                LineOrigin::Incoming
            ]
        );
        // Side texts survive.
        let (cur, inc) = parsed.sides(&p).unwrap();
        assert_eq!(cur.as_deref(), Some("A\nC\n"));
        assert_eq!(inc.as_deref(), Some("B\n"));
    }

    #[test]
    fn json_round_trips_special_chars_and_null_side() {
        let mut b = ResolutionBuffer::new(Path::new("/tmp/re\"po"));
        let p = PathBuf::from("x.txt");
        let fr = FileResolution {
            identity: None,
            editing_origin: None,
            live_editing_origin: false,
            pending_edit: PendingEdit::default(),
            draft_state: false,
            recovery_only: false,
            current_text: Some("quote \"x\"\ttab\n".to_string()),
            incoming_text: None,
            binary: false,
            raw: false,
            current_raw: None,
            incoming_raw: None,
            raw_result: None,
            result: None,
            hunk_state: None,
            worktree_fingerprint: None,
            worktree_changed: false,
            marker_residue: false,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        b.files.insert(p.clone(), fr);
        let json = b.to_json().expect("encode buffer");
        let parsed = parse_buffer_json(Path::new("/tmp/re\"po"), &json).expect("parse");
        let (cur, inc) = parsed.sides(&p).unwrap();
        assert_eq!(cur.as_deref(), Some("quote \"x\"\ttab\n"));
        assert_eq!(inc, None);
        assert!(!parsed.has_resolution(&p));
    }

    #[test]
    fn parse_rejects_non_object() {
        assert!(parse_buffer_json(Path::new("/tmp/repo"), "nope").is_none());
        assert!(parse_buffer_json(Path::new("/tmp/repo"), "[[]]").is_none());
    }

    #[test]
    fn legacy_records_preserve_resolution_states_and_optional_defaults() {
        let json = r#"{"files":[
            null,
            {"result":[]},
            {"path":"unresolved"},
            {"path":"empty","result":[]},
            {"path":"text","current":"A","result":[
                {"t":"A","o":"c"},{"t":"B","o":"i"},{"t":"C","o":"?"},{},42
            ],"raw_result":{"oid":"not-an-oid","mode":33188}},
            {"path":"raw","binary":true,"raw_result":{
                "oid":"0123456789012345678901234567890123456789","mode":40960
            }}
        ]}"#;
        let buffer = parse_buffer_json(Path::new("/tmp/repo"), json).expect("legacy buffer");
        assert!(!buffer.has_resolution(Path::new("")));
        assert!(!buffer.has_resolution(Path::new("unresolved")));
        assert!(buffer.has_resolution(Path::new("empty")));
        assert_eq!(
            buffer.resolved_text(Path::new("empty")).as_deref(),
            Some("")
        );
        let text = Path::new("text");
        assert_eq!(buffer.resolved_text(text).as_deref(), Some("A\nB\nC\n\n"));
        assert_eq!(buffer.sides(text), Some((Some("A".into()), None)));
        assert_eq!(
            buffer.provenance(text),
            Some(vec![
                LineOrigin::Current,
                LineOrigin::Incoming,
                LineOrigin::Manual,
                LineOrigin::Manual,
            ])
        );
        assert_eq!(buffer.raw_resolution(text), None);
        assert_eq!(
            buffer.raw_resolution(Path::new("raw")),
            Some(RawResolution {
                oid: git2::Oid::from_str("0123456789012345678901234567890123456789").unwrap(),
                mode: 40960,
            })
        );
    }

    // ── Hunk-level model (W32-CONFLICT-EDITOR) ──────────────────────

    /// A two-hunk zdiff3 materialization with passthrough context around and
    /// between the conflicts.
    const TWO_HUNK_ZDIFF3: &str = "\
keep top
<<<<<<< Current
a-current
||||||| Base
a-base
=======
a-incoming
>>>>>>> Incoming
keep middle
<<<<<<< Current
b-current
=======
b-incoming
>>>>>>> Incoming
keep bottom
";

    #[test]
    fn buffer_apply_hunk_choice_updates_result_and_undo() {
        let (mut b, p) = buf_with_sides(
            "keep top\na-current\nkeep middle\nb-current\nkeep bottom\n",
            "keep top\na-incoming\nkeep middle\nb-incoming\nkeep bottom\n",
        );
        assert!(b.ensure_hunks(&p, TWO_HUNK_ZDIFF3, 7));
        assert_eq!(b.hunk_count(&p), 2);

        assert!(b.apply_hunk_choice(&p, 0, HunkChoice::AcceptCurrent));
        assert!(b.apply_hunk_choice(&p, 1, HunkChoice::AcceptIncoming));
        assert!(b.hunks_all_resolved(&p));

        let text = b.resolved_text(&p).unwrap();
        assert!(text.contains("a-current"));
        assert!(text.contains("b-incoming"));
        assert!(text.contains("keep top"));
        assert!(!super::super::checklist::text_has_conflict_marker(&text));

        // File-level undo unwinds the last hunk commit.
        assert!(b.undo(&p));
        let after_undo = b.resolved_text(&p).unwrap();
        // After undoing hunk 1's commit, only hunk 0 was applied → hunk 1 still
        // contributes its markers, so residue is present again.
        assert!(super::super::checklist::text_has_conflict_marker(
            &after_undo
        ));
    }

    #[test]
    fn ensure_hunks_is_idempotent() {
        let (mut b, p) = buf_with_sides(
            "keep top\na-current\nkeep middle\nb-current\nkeep bottom\n",
            "keep top\na-incoming\nkeep middle\nb-incoming\nkeep bottom\n",
        );
        b.ensure_hunks(&p, TWO_HUNK_ZDIFF3, 7);
        b.apply_hunk_choice(&p, 0, HunkChoice::AcceptCurrent);
        // A second ensure with different text must NOT clobber existing choices.
        b.ensure_hunks(&p, "<<<<<<< Current\nz\n=======\nw\n>>>>>>> Incoming\n", 7);
        assert_eq!(b.hunk_count(&p), 2);
        assert_eq!(b.hunk_model(&p).unwrap().resolved_hunk_count(), 1);
    }
}
