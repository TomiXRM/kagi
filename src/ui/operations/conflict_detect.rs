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
    /// (#704). Carries the observation so a stale one can be told from the
    /// accepted read, like `Detected` (#707 re-review).
    MergeResolvedReady(kagi_domain::conflict_family::ConflictObservation),
    /// An active conflict/merge with files to resolve.  Boxed: the session +
    /// resolution buffer are large, and this variant is the rare case.
    Detected(Box<ConflictDetected>),
}

/// Payload of [`ConflictDetectOutcome::Detected`] — the assembled conflict state
/// the UI-thread apply moves into the frozen owner's conflict slot.
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
    /// The tab a detection run belongs to, frozen at launch (#707 review):
    /// SessionId, visit and path together, so a result that lands after a
    /// close and reopen of the same repository is recognisably not this tab's.
    pub(crate) fn detect_owner(&self) -> Option<crate::app::Attachment> {
        self.active_session()
            .and_then(|session| self.app_sessions.attachment(session))
    }

    /// Read-only conflict detection: opens the repo, detects the session, builds
    /// the resolution buffer, recomputes per-file status, auto-selects a file, and
    /// materializes zdiff3 markers for the selected content file.  This is the
    /// **entire I/O half** of conflict detection — pure inputs/outputs (no `self`)
    /// so it runs either synchronously (`detect_conflict_mode`) or on a background
    /// thread (`detect_conflict_mode_async`).  `current_branch` and the `prev_*`
    /// preservation indices are captured by the caller from `self`.
    pub fn detect_conflict_payload(
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
            return ConflictDetectOutcome::MergeResolvedReady(snapshot.observation);
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
        owner: crate::app::Attachment,
        outcome: ConflictDetectOutcome,
        cx: &mut Context<Self>,
    ) {
        // The result belongs to its frozen attachment. A completion from the
        // visit that just departed may refresh its inactive owner's retained
        // pane, but it cannot become authoritative after that owner is active
        // again; activation revalidation must win over the old visit.
        let active_owner = self.active_session() == Some(owner.session);
        let Some(attached) = self.app_sessions.attachment(owner.session) else {
            return;
        };
        let exact_visit = owner.visit == attached.visit;
        let owner_is_current = attached.path == owner.path
            && attached.worktree == owner.worktree
            && (exact_visit || (!active_owner && owner.visit < attached.visit));
        if !owner_is_current {
            if let Some(ui) = self.ui.get_mut(&owner.session) {
                ui.conflict_detected = false;
            }
            return;
        }
        if !self.ui.contains_key(&owner.session) {
            return;
        }
        // #707 review: and it must describe the repository the tab is
        // *showing*. The accepted read is authoritative (ADR-0183), so a
        // payload observed against an older state is dropped whole — never
        // re-labelled with the accepted revision, which would hand a stale
        // resolution buffer an identity that admission accepts. With no
        // accepted read yet there is nothing to disagree with: the detector's
        // own observation stands as display, and `Sessions` holding no
        // observation keeps Save and D/F refused until a read lands.
        //
        // Every outcome is checked, not only `Detected`: a stale "there is no
        // conflict" answer tears down the editor and (for `OpenFailed`) the
        // stash identity of a tab whose accepted read says an operation is
        // very much in progress, and leaves no projection until the next
        // reload. Fail closed and re-detect instead.
        let accepted_operation = self.reads.get(Some(owner.session)).operation.clone();
        let accepted = accepted_operation.as_ref();
        let stale = match (&outcome, accepted) {
            (ConflictDetectOutcome::Detected(d), Some(op)) => op.observation != d.observation,
            (ConflictDetectOutcome::MergeResolvedReady(o), Some(op)) => &op.observation != o,
            (
                ConflictDetectOutcome::Detected(_) | ConflictDetectOutcome::MergeResolvedReady(_),
                None,
            ) => false,
            (ConflictDetectOutcome::Cleared | ConflictDetectOutcome::OpenFailed, accepted) => {
                accepted.is_some()
            }
        };
        if stale {
            // The read that superseded it re-detects on its own; this re-arm
            // covers the commit points that do not.
            if let Some(state) = self.ui.get_mut(&owner.session) {
                state.conflict_detected = false;
            }
            return;
        }
        // #704 review P1: this detector must NOT write `Sessions`' conflict
        // observation. The accepted read remains authoritative; a detection
        // completion only projects that observation into the conflict pane.
        // `on_view_published` owns the Sessions observation from accepted Reads.
        //
        // The stash identity is proposal evidence and therefore stricter than
        // retained display: an old visit may refresh an inactive pane, but it
        // cannot create or clear a proposal for the next visit.
        if exact_visit {
            let identity = match &outcome {
                ConflictDetectOutcome::Detected(d) => d.stash_identity.as_slice(),
                _ => &[],
            };
            if matches!(outcome, ConflictDetectOutcome::OpenFailed) {
                self.app_sessions.clear_stash_conflict(owner.session);
            } else {
                self.app_sessions
                    .observe_stash_conflict(owner.session, identity);
            }
        }
        match outcome {
            ConflictDetectOutcome::OpenFailed => {
                if let Some(ui) = self.ui.get_mut(&owner.session) {
                    ui.conflict = None;
                }
            }
            ConflictDetectOutcome::Cleared => {
                if self
                    .ui
                    .get(&owner.session)
                    .and_then(|ui| ui.conflict.as_ref())
                    .is_some_and(|entity| entity.read(cx).mode.is_some())
                {
                    klog!("conflict-mode: cleared");
                }
                if let Some(ui) = self.ui.get_mut(&owner.session) {
                    ui.conflict = None;
                }
            }
            ConflictDetectOutcome::MergeResolvedReady(_) => {
                klog!("conflict-mode: merge resolved — ready to commit");
                if let Some(ui) = self.ui.get_mut(&owner.session) {
                    ui.conflict = None;
                }
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

                // This session and buffer were read at `observation`, so that
                // is the revision they get. The guard above already proved it
                // equals the accepted read's when there is one (#707 review).
                let mode = conflict_view::ConflictMode {
                    revision: observation.revision,
                    session,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    skip_armed: false,
                };
                let files = mode.session.files.clone();

                match self
                    .ui
                    .get(&owner.session)
                    .and_then(|ui| ui.conflict.clone())
                {
                    // Re-detect against a retained pane. An **unchanged**
                    // observation must change nothing at all (#722 P2): the
                    // resolution buffer carries the undo/redo stack, and the
                    // entity carries the selected file/hunk and scroll, so
                    // swapping in a freshly read `mode` would silently discard
                    // the user's work. `revision` is a content fingerprint of
                    // the observed operation, so equality means the repository
                    // never moved. A changed one is folded in place below.
                    Some(entity)
                        if entity
                            .read(cx)
                            .mode
                            .as_ref()
                            .is_some_and(|current| current.revision == mode.revision) =>
                    {
                        let _ = entity;
                    }
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
                        let repo_path = owner.path.clone();
                        let entity = cx.new(|_| {
                            let mut view = conflict_view::ConflictView::new(
                                weak_app,
                                repo_path,
                                owner.clone(),
                            );
                            view.mode = Some(mode);
                            view.editing = editing_path;
                            view
                        });
                        if let Some(ui) = self.ui.get_mut(&owner.session) {
                            ui.conflict = Some(entity);
                        }
                    }
                }
            }
        }
    }
    /// Detect (or clear) Conflict Mode for the active session.
    ///
    /// Runs at most once per owner per cycle. `reload()` and tab activation
    /// re-arm that owner's guard. Opens the repo read-only, calls
    /// `detect_conflict_session`, and on a hit builds a fresh
    /// `ResolutionBuffer` from the index (preferring a previously autosaved
    /// buffer so a partial resolution survives a restart), recomputes each
    /// file's status from the buffer, and stores the `ConflictMode` (via
    /// `apply_conflict_detect`, which builds / updates the owner's
    /// `ConflictView` entity). On a miss it drops that owner's entity. The
    /// repository is never mutated here.
    pub fn detect_conflict_mode(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                if let Some(ui) = self.ui_mut() {
                    ui.conflict = None;
                }
                return;
            }
        };
        let Some(owner) = self.detect_owner() else {
            return;
        };
        if self.ui().conflict_detected {
            return;
        }
        if let Some(ui) = self.ui_mut() {
            ui.conflict_detected = true;
        }

        // Snapshot the preservation inputs the I/O step needs (prev selection /
        // editing index), then run the read-only Git/index/file I/O synchronously.
        // Issue #285: capture the previously-selected/editing files by PATH, not
        // index — a per-file Save re-sorts `session.files`, so a stored index
        // would silently follow to a different file after re-detection.
        let (prev_selected_path, prev_editing_path) = self
            .ui()
            .conflict
            .as_ref()
            .map(|e| {
                let v = e.read(cx);
                let sel = v.mode.as_ref().and_then(|c| {
                    c.selected_file
                        .and_then(|i| c.session.files.get(i))
                        .map(|f| f.path.clone())
                });
                (sel, v.editing.clone())
            })
            .unwrap_or((None, None));
        let current_branch = self.view().status_summary.branch.clone();
        let outcome = Self::detect_conflict_payload(
            &repo_path,
            prev_selected_path,
            prev_editing_path,
            current_branch,
        );
        self.apply_conflict_detect(owner, outcome, cx);
    }

    /// T-PERF-RENDER-001: async sibling of [`detect_conflict_mode`].
    ///
    /// Runs the same read-only Backend / index / `ResolutionBuffer` I/O on a
    /// background thread (`cx.background_spawn`), then marshals the result back to
    /// [`apply_conflict_detect`] on the UI thread. The owner-local run-once guard
    /// is armed up-front so repeated calls never re-launch the work. Used by the
    /// startup / tab-switch commit points where `reload()` (which calls the sync
    /// variant) did not run.
    pub fn detect_conflict_mode_async(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                if let Some(ui) = self.ui_mut() {
                    ui.conflict = None;
                }
                return;
            }
        };
        let Some(owner) = self.detect_owner() else {
            return;
        };
        if self.ui().conflict_detected {
            return;
        }
        if let Some(ui) = self.ui_mut() {
            ui.conflict_detected = true;
        }

        // Issue #285: capture the previously-selected/editing files by PATH, not
        // index — a per-file Save re-sorts `session.files`, so a stored index
        // would silently follow to a different file after re-detection.
        let (prev_selected_path, prev_editing_path) = self
            .ui()
            .conflict
            .as_ref()
            .map(|e| {
                let v = e.read(cx);
                let sel = v.mode.as_ref().and_then(|c| {
                    c.selected_file
                        .and_then(|i| c.session.files.get(i))
                        .map(|f| f.path.clone())
                });
                (sel, v.editing.clone())
            })
            .unwrap_or((None, None));
        let current_branch = self.view().status_summary.branch.clone();

        let task = cx.background_spawn(async move {
            Self::detect_conflict_payload(
                &repo_path,
                prev_selected_path,
                prev_editing_path,
                current_branch,
            )
        });
        cx.spawn(async move |this, acx| {
            let outcome = task.await;
            let _ = this.update(acx, |app, cx| {
                app.apply_conflict_detect(owner, outcome, cx);
                cx.notify();
            });
        })
        .detach();
    }
}

/// A detector payload read right now, for the #707 regressions that have to
/// land one observation's payload while a different one is on screen. The
/// production callers go through `detect_conflict_mode{,_async}`, which freeze
/// the owner; this is the read half on its own.
pub fn detect_payload_for_test(repo: &std::path::Path, branch: &str) -> ConflictDetectOutcome {
    KagiApp::detect_conflict_payload(repo, None, None, branch.to_string())
}
