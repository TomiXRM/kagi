//! Memory capture is immediate; only a successful completion acknowledges disk.

use super::*;
use std::sync::Condvar;
use std::time::{Duration, Instant};

const COALESCE: Duration = Duration::from_millis(250);

#[derive(Default)]
struct State {
    generation: u64,
    completed: u64,
    saved: u64,
    path: Option<PathBuf>,
    failure: Option<String>,
    pending: Option<ResolutionBuffer>,
    queued_at: Option<Instant>,
    urgent: bool,
    running: bool,
}

#[derive(Default)]
pub(super) struct Writer {
    state: Mutex<State>,
    changed: Condvar,
    transaction: Mutex<()>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Pending snapshots share this writer; don't recursively format them.
        f.debug_struct("DraftWriter").finish_non_exhaustive()
    }
}

/// A Send completion handle. Queue acceptance does not acknowledge durability.
/// Older handles cannot acknowledge a later generation, including across clones.
#[derive(Clone, Debug)]
pub struct DraftSave {
    writer: Arc<Writer>,
    generation: u64,
}

impl DraftSave {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_current(&self) -> bool {
        self.writer
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation
            == self.generation
    }

    /// Wait for this snapshot or a newer coalesced snapshot to reach disk.
    /// Run on a background executor, never the GUI thread. This does not shorten
    /// the coalescing interval; explicit operation barriers use `flush` instead.
    pub fn wait(&self) -> Result<PathBuf, GitError> {
        self.writer.wait(self.generation, false)
    }

    /// Force this snapshot to start and await durable completion off the GUI
    /// thread. The caller must still reject a superseded generation.
    pub fn flush(&self) -> Result<(), GitError> {
        self.writer.wait(self.generation, true).map(|_| ())
    }
}

impl Writer {
    fn queue(self: &Arc<Self>, snapshot: ResolutionBuffer) -> Result<DraftSave, GitError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.generation += 1;
        let generation = state.generation;
        state.pending = Some(snapshot);
        state.queued_at.get_or_insert_with(Instant::now);
        if !state.running {
            state.running = true;
            let writer = self.clone();
            if let Err(error) = std::thread::Builder::new()
                .name("conflict-draft-writer".into())
                .spawn(move || writer.run())
            {
                let message = format!("conflicts: could not start draft writer: {error}");
                state.running = false;
                state.pending = None;
                state.queued_at = None;
                state.completed = generation;
                state.failure = Some(message.clone());
                self.changed.notify_all();
                return Err(GitError::Other(message));
            }
        }
        self.changed.notify_all();
        Ok(DraftSave {
            writer: self.clone(),
            generation,
        })
    }

    fn run(self: Arc<Self>) {
        loop {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.pending.is_none() {
                state.running = false;
                return;
            }
            while !state.urgent {
                let deadline = state.queued_at.expect("pending snapshot has a deadline") + COALESCE;
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                state = self
                    .changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let snapshot = state.pending.take().expect("pending snapshot");
            let generation = state.generation;
            state.queued_at = None;
            state.urgent = false;
            drop(state);

            let _transaction = self.transaction.lock().unwrap_or_else(|e| e.into_inner());
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| snapshot.autosave_now()))
                    .unwrap_or_else(|_| {
                        Err(GitError::Other(
                            "conflicts: draft writer panicked; durable write was not acknowledged"
                                .into(),
                        ))
                    });
            self.finish(generation, &result);
        }
    }

    fn finish(&self, generation: u64, result: &Result<PathBuf, GitError>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(path) = result {
            state.saved = state.saved.max(generation);
            state.path = Some(path.clone());
        }
        if generation >= state.completed {
            state.completed = generation;
            state.failure = result.as_ref().err().map(ToString::to_string);
        }
        self.changed.notify_all();
    }

    fn wait(&self, generation: u64, urgent: bool) -> Result<PathBuf, GitError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if urgent && state.pending.is_some() {
            state.urgent = true;
            self.changed.notify_all();
        }
        while state.completed < generation {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if state.saved >= generation {
            state.path.clone().ok_or_else(|| {
                GitError::Other("conflicts: no durable draft completion exists".into())
            })
        } else {
            Err(GitError::Other(state.failure.clone().unwrap_or_else(
                || "conflicts: draft write was not acknowledged".into(),
            )))
        }
    }

    pub(super) fn autosave(&self, buffer: &ResolutionBuffer) -> Result<PathBuf, GitError> {
        let generation = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation;
        if generation > 0 {
            // Never let a stale preservation clone acknowledge a queued body
            // whose write failed. The live owner must queue and complete a retry.
            self.wait(generation, true)?;
        }
        let _transaction = self.transaction.lock().unwrap_or_else(|e| e.into_inner());
        let result = buffer.autosave_now();
        self.finish(generation, &result);
        result
    }
}

impl ResolutionBuffer {
    /// Capture the latest body synchronously, then serialize disk I/O on one
    /// background writer. Pending snapshots coalesce for at most 250 ms before
    /// starting; an already-running fsync can extend the unacknowledged window.
    /// Call on the current live owner's buffer; preservation clones share
    /// barriers, not independent coalescing editors.
    pub fn queue_autosave(&self) -> Result<DraftSave, GitError> {
        self.draft_writer.queue(self.autosave_snapshot())
    }

    /// Force queued work to start, and await durable completion. This is a
    /// blocking backend barrier: GUI close/quit and operation gates must await
    /// it on their background executor. Failure stays pending until a retry wins.
    pub fn flush(&self) -> Result<(), GitError> {
        let generation = self
            .draft_writer
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation;
        if generation == 0 {
            return Ok(());
        }
        self.draft_writer.wait(generation, true).map(|_| ())
    }

    /// Includes live edits not yet captured by the queued writer, including a
    /// failed synchronous autosave. Queue those edits before using `flush` as
    /// a durability barrier; `flush` alone never captures a new snapshot.
    pub fn has_unflushed_drafts(&self) -> bool {
        let state = self
            .draft_writer
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.generation > state.saved
            || self
                .files
                .values()
                .chain(self.recoveries.iter().map(|entry| &entry.file))
                .any(|file| file.pending_edit.is_pending())
    }

    fn autosave_snapshot(&self) -> Self {
        Self {
            repo_path: self.repo_path.clone(),
            live_context: self.live_context.clone(),
            files: self
                .files
                .iter()
                .map(|(path, file)| (path.clone(), file.autosave_snapshot()))
                .collect(),
            marker_residue_count: self.marker_residue_count,
            hunks: self.hunks.clone(),
            recoveries: self
                .recoveries
                .iter()
                .map(|entry| RecoveryEntry {
                    path: entry.path.clone(),
                    file: entry.file.autosave_snapshot(),
                })
                .collect(),
            disk_snapshot: self.disk_snapshot.clone(),
            draft_writer: self.draft_writer.clone(),
            draft_restore_warnings: Vec::new(),
        }
    }
}

impl FileResolution {
    fn autosave_snapshot(&self) -> Self {
        // Undo/redo can contain hundreds of large bodies; persistence needs only
        // the current one. Keep the captured edit generation, not Clone's new owner.
        Self {
            identity: self.identity.clone(),
            worktree_fingerprint: self.worktree_fingerprint.clone(),
            worktree_changed: self.worktree_changed,
            editing_origin: self.editing_origin.clone(),
            live_editing_origin: self.live_editing_origin,
            pending_edit: self.pending_edit.for_autosave(),
            draft_state: self.draft_state,
            recovery_only: self.recovery_only,
            current_text: self.current_text.clone(),
            incoming_text: self.incoming_text.clone(),
            binary: self.binary,
            raw: self.raw,
            current_raw: self.current_raw,
            incoming_raw: self.incoming_raw,
            raw_result: self.raw_result,
            result: self.result.clone(),
            hunk_state: self.hunk_state.clone(),
            marker_residue: self.marker_residue,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}
