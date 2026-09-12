//! Conflict detection: the read-only I/O half (`detect_conflict_payload`,
//! which runs on a background thread) and the UI-thread apply that builds,
//! updates or drops the `Entity<ConflictView>` from its result.
//!
//! Split out of `operations/conflict.rs` (#704): detection observes, the
//! rest of that module acts. Detection deliberately owns no application
//! state — `Sessions`' conflict observation comes from accepted reads via
//! `on_view_published`, never from here (review P1).

use crate::ui::*;

/// T-PERF-RENDER-001: the `Send` result of the read-only conflict-detection I/O
/// (`KagiApp::detect_conflict_payload`), applied to `KagiApp` on the UI thread by
/// `apply_conflict_detect`.  Splitting the I/O out of the state mutation lets the
/// same detection run either synchronously (`reload`) or off the UI thread
/// (`detect_conflict_mode_async`) without changing the emitted `[kagi]` lines.
pub enum ConflictDetectOutcome {
    /// `Backend::open` failed — clear mode and leave the read model alone.
    OpenFailed,
    /// No conflict session — clear Conflict Mode (emits `conflict-mode: cleared`
    /// only when a mode was previously open).
    Cleared,
    /// A merge with MERGE_HEAD but no unmerged entries — resolved, ready to
    /// commit. Only the *editor* has nothing left to show: the merge itself
    /// lives on in the read model, which is what the abort is admitted from
    /// (#704).
    MergeResolvedReady,
    /// An active conflict/merge with files to resolve.  Boxed: the session +
    /// resolution buffer are large, and this variant is the rare case.
    Detected(Box<ConflictDetected>),
}

/// Payload of [`ConflictDetectOutcome::Detected`] — the assembled conflict state
/// the UI-thread apply moves into `self.conflict`.
pub struct ConflictDetected {
    stash_identity: Vec<String>,
    session: kagi_git::conflicts::ConflictSession,
    observation: kagi_domain::conflict_family::ConflictObservation,
    buffer: kagi_git::resolution::ResolutionBuffer,
    current_branch: String,
    selected_file: Option<usize>,
    editing_file: Option<usize>,
    /// Selected content file whose hunks were materialized; set as the open
    /// editor file on apply.
    editing_path: Option<PathBuf>,
}

impl KagiApp {
    /// Read-only conflict detection: opens the repo, detects the session, builds
    /// the resolution buffer, recomputes per-file status, auto-selects a file, and
    /// materializes zdiff3 markers for the selected content file.  This is the
    /// **entire I/O half** of conflict detection — pure inputs/outputs (no `self`)
    /// so it runs either synchronously (`detect_conflict_mode`) or on a background
    /// thread (`detect_conflict_mode_async`).  `current_branch` and the `prev_*`
    /// preservation indices are captured by the caller from `self`.
    pub(crate) fn detect_conflict_payload(
        repo_path: &Path,
        prev_selected_path: Option<PathBuf>,
        prev_editing_path: Option<PathBuf>,
        current_branch: String,
    ) -> ConflictDetectOutcome {
        let repo = match crate::ui::blocking_ops::open_backend(repo_path) {
            Ok(r) => r,
            Err(_) => return ConflictDetectOutcome::OpenFailed,
        };

        let snapshot = match repo.conflict_snapshot() {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => return ConflictDetectOutcome::Cleared,
            Err(_) => return ConflictDetectOutcome::OpenFailed,
        };
        let session = snapshot.session;

        // A merge with MERGE_HEAD present but no remaining unmerged index entries
        // is not a conflict to resolve — it is a resolved merge ready to commit.
        if matches!(session.op, kagi_git::ConflictOp::Merge { .. }) && session.files.is_empty() {
            return ConflictDetectOutcome::MergeResolvedReady;
        }

        // Build / reload the resolution buffer.  A previously-autosaved buffer
        // (e.g. from before a restart) is preferred so partial work survives;
        // otherwise materialize a fresh buffer from the index conflicts.
        // #297: index-authoritative buffer with autosaved drafts overlaid. The
        // old `load().or_else(from_repo)` let an autosave (which does not persist
        // the index-derived `raw` metadata) short-circuit `from_repo`, so a
        // binary/symlink "take side" failed with "that side does not exist".
        let mut buffer = repo
            .resolution_buffer_from_repo_with_autosave()
            .unwrap_or_else(|_| kagi_git::ResolutionBuffer::new(repo_path));

        // Recompute per-file status from the buffer (detection seeds Unresolved).
        let mut session = session;
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            if buffer.has_resolution(&f.path) {
                f.status = if residue.contains(&f.path) {
                    kagi_git::ConflictStatus::NeedsReview
                } else {
                    kagi_git::ConflictStatus::Resolved
                };
            } else {
                f.status = kagi_git::ConflictStatus::Unresolved;
            }
        }

        // Preserve the previously-selected file across re-detections by PATH
        // (issue #285): a per-file Save re-sorts / renumbers `files`, so the old
        // index would silently point at a different file. Fall back to the first
        // unresolved file (KDiff3-style "land on work to do"), then index 0.
        let selected_file =
            kagi_git::resolve_selected_file(&session.files, prev_selected_path.as_deref());

        // W33: preserve the dashboard editing file across re-detection — by PATH,
        // for the same reason (issue #285). Dropped if that file is gone.
        let editing_file = prev_editing_path
            .as_deref()
            .and_then(|p| session.files.iter().position(|f| f.path == p));

        // The center A/B editor renders from the hunk model, which needs the repo
        // to materialize zdiff3 markers.  With auto-selection the user never
        // clicked, so build the hunk model for the selected content file here.
        let mut editing_path = None;
        if let Some(idx) = selected_file {
            if let Some(f) = session.files.get(idx) {
                if f.kind == kagi_git::ConflictKind::Content {
                    let path = f.path.clone();
                    if let Some(markers) = repo.materialized_markers(&buffer, &path) {
                        buffer.ensure_hunks(&path, &markers);
                    }
                    editing_path = Some(path);
                }
            }
        }

        ConflictDetectOutcome::Detected(Box::new(ConflictDetected {
            stash_identity: if matches!(session.op, kagi_git::ConflictOp::StashConflict) {
                repo.stash_conflict_identity().unwrap_or_default()
            } else {
                vec![]
            },
            session,
            observation: snapshot.observation,
            buffer,
            current_branch,
            selected_file,
            editing_file,
            editing_path,
        }))
    }

    /// Foreground half of conflict detection: apply a [`ConflictDetectOutcome`]
    /// computed by [`detect_conflict_payload`] to `self`, emitting the same
    /// `[kagi]` contract lines in the same order as the original synchronous
    /// implementation. ADR-0118: this is the single point that builds / updates /
    /// drops the `Entity<ConflictView>` — `Detected` updates an existing entity in
    /// place (preserving its splits / editor inputs / before-text) or creates a
    /// new one; `Cleared` / `MergeResolvedReady` / `OpenFailed` drop it. The
    /// "was a conflict open?" (Cleared) and editor-close (Detected) checks read
    /// the entity here because they must reflect the current UI state at apply
    /// time. Needs `cx` (entity create / read / update).
    pub fn apply_conflict_detect(
        &mut self,
        outcome: ConflictDetectOutcome,
        cx: &mut Context<Self>,
    ) {
        // #704 review P1: this detector must NOT write `Sessions`' conflict
        // observation. Its job is keyed on the repository path alone — no
        // `SessionId`, no read revision — so a `Cleared` it observed before a
        // newer accepted read can marshal back afterwards and erase the
        // revision the strip is still showing, turning the next Abort into a
        // `StaleApproval`. `on_view_published` owns it, from accepted `Reads`.
        if let Some(owner) = self.active_session() {
            let identity = match &outcome {
                ConflictDetectOutcome::Detected(d) => d.stash_identity.as_slice(),
                _ => &[],
            };
            if matches!(outcome, ConflictDetectOutcome::OpenFailed) {
                self.app_sessions.clear_stash_conflict(owner);
            } else {
                self.app_sessions.observe_stash_conflict(owner, identity);
            }
        }
        match outcome {
            ConflictDetectOutcome::OpenFailed => {
                self.conflict = None;
            }
            ConflictDetectOutcome::Cleared => {
                if self
                    .conflict
                    .as_ref()
                    .is_some_and(|e| e.read(cx).mode.is_some())
                {
                    klog!("conflict-mode: cleared");
                }
                // Drop the entity (clears mode + editing + splits + before-text;
                // the accepted Stage-1 reset delta on re-entry).
                self.conflict = None;
            }
            ConflictDetectOutcome::MergeResolvedReady => {
                klog!("conflict-mode: merge resolved — ready to commit");
                // Only the *editor* has nothing left to show. The merge itself
                // lives on in the read model (#704).
                self.conflict = None;
            }
            ConflictDetectOutcome::Detected(detected) => {
                let ConflictDetected {
                    stash_identity: _,
                    session,
                    observation,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    editing_path,
                } = *detected;
                // Same bytes as the `eprintln!` it replaces — `klog!` adds the
                // `[kagi] ` prefix (ADR-0096); moving the file made the raw
                // form a new ratchet entry, and converting is the fix.
                klog!(
                    "conflict-mode: {} {} file(s)",
                    session.op.slug(),
                    session.files.len()
                );

                // Freeze the revision the *read model* holds, not this
                // detector's own re-read (#704 review P1): `Sessions` learns
                // the observation from accepted reads only, so a Save frozen
                // against a different one is refused as a stale approval. The
                // fallback covers the window before a tab's first read, where
                // `Sessions` has nothing either and any request is refused.
                let mode = conflict_view::ConflictMode {
                    revision: self
                        .view()
                        .operation
                        .as_ref()
                        .map(|op| op.revision().clone())
                        .unwrap_or(observation.revision),
                    session,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    skip_armed: false,
                };
                let files = mode.session.files.clone();

                match self.conflict.clone() {
                    // Re-detect: update the existing entity in place so its splits
                    // / editor inputs / before-text / scroll survive the reload.
                    Some(entity) => {
                        entity.update(cx, |v, _| {
                            let prev_editing = v.editing.clone();
                            // W32: close the editor if the edited file is no longer
                            // conflicted (reads the entity's current `editing`).
                            if let Some(editing) = v.editing.clone() {
                                if !files.iter().any(|f| f.path == editing) {
                                    v.editing = None;
                                }
                            }
                            v.mode = Some(mode);
                            if let Some(path) = editing_path {
                                v.editing = Some(path);
                            }
                            // Issue #285: the editor file just changed, so the
                            // stored hunk index belongs to the old file — reset it
                            // (the new file may have fewer hunks).
                            if v.editing != prev_editing {
                                v.selected_hunk = 0;
                            }
                        });
                    }
                    // Fresh conflict: build the entity, capturing the repo path +
                    // a weak back-ref for its deferred parent callbacks.
                    None => {
                        let weak_app = cx.weak_entity();
                        let repo_path = self.repo_path.clone().unwrap_or_default();
                        let Some(owner) = self
                            .active_session()
                            .and_then(|session| self.app_sessions.attachment(session))
                        else {
                            self.conflict = None;
                            return;
                        };
                        let entity = cx.new(|_| {
                            let mut v =
                                conflict_view::ConflictView::new(weak_app, repo_path, owner);
                            v.mode = Some(mode);
                            v.editing = editing_path;
                            v
                        });
                        self.conflict = Some(entity);
                    }
                }
            }
        }
    }
}
