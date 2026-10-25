//! Conflict detection: the read-only I/O half (`detect_conflict_payload`,
//! which runs on a background thread) and the UI-thread apply that builds,
//! updates or drops the `Entity<ConflictView>` from its result.
//!
//! Split out of `operations/conflict.rs` (#704): detection observes, the
//! rest of that module acts. Detection deliberately owns no application
//! state — `Sessions`' conflict observation comes from accepted reads via
//! `on_view_published`, never from here (review P1).

use crate::ui::tab_ui_state_ops::PaneRevalidation;
use crate::ui::*;

#[cfg(feature = "gui-e2e")]
type DetectionHold = (
    gpui::Task<()>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    bool,
);

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DETECT_HOLD: std::cell::RefCell<Option<DetectionHold>> =
        const { std::cell::RefCell::new(None) };
}

/// T-PERF-RENDER-001: the `Send` result of the read-only conflict-detection I/O
/// (`KagiApp::detect_conflict_payload`), applied to `KagiApp` on the UI thread by
/// `apply_conflict_detect`.  Splitting the I/O out of the state mutation lets the
/// same detection run either synchronously (`reload`) or off the UI thread
/// (`detect_conflict_mode_async`) without changing the emitted `[kagi]` lines.
pub enum ConflictDetectOutcome {
    /// Repo/snapshot open failed, or saved edits could not be restored. Retain
    /// restore failures for a durable receipt instead of silently retrying.
    OpenFailed(Option<ConflictRestoreFailure>),
    /// No conflict session — clear Conflict Mode (emits `conflict-mode: cleared`
    /// only when a mode was previously open).
    Cleared,
    /// An active conflict/merge, including a fully staged operation awaiting
    /// Continue. The payload always owns a fresh snapshot and resolution buffer.
    Detected(Box<ConflictDetected>),
}

/// A restore error belongs to the snapshot that attempted to load its draft,
/// not whichever conflict happens to be displayed when the error arrives.
#[derive(Clone)]
pub struct ConflictRestoreFailure {
    pub observation: kagi_domain::conflict_family::ConflictObservation,
    pub cause: String,
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
    /// Damaged local documents were preserved under reserved names by restore.
    draft_restore_warnings: Vec<kagi_git::resolution::DraftRestoreWarning>,
}

impl KagiApp {
    /// Hold the next real async result immediately before its freshness checks.
    /// The flag proves it is a restore error, or a detection when false is passed.
    #[cfg(feature = "gui-e2e")]
    pub fn hold_next_conflict_detection_for_e2e(
        hold: gpui::Task<()>,
        expect_restore_error: bool,
    ) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        DETECT_HOLD.with(|slot| {
            assert!(slot
                .borrow_mut()
                .replace((hold, reached.clone(), expect_restore_error))
                .is_none());
        });
        reached
    }

    #[cfg(feature = "gui-e2e")]
    pub fn clear_conflict_detection_hold_for_e2e() {
        DETECT_HOLD.with(|slot| drop(slot.borrow_mut().take()));
    }

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
            Err(_) => return ConflictDetectOutcome::OpenFailed(None),
        };

        let snapshot = match repo.conflict_snapshot() {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => return ConflictDetectOutcome::Cleared,
            Err(_) => return ConflictDetectOutcome::OpenFailed(None),
        };
        let session = snapshot.session;

        // Live index sides remain authoritative (#297). Restore only drafts
        // bound to this operation and file's stages; old/legacy drafts stay
        // recoverable without marking the current conflict resolved. Saved
        // data never replaces the raw metadata rebuilt from the index.
        let mut buffer = match repo.resolution_buffer_from_repo_with_autosave() {
            Ok(buffer) => buffer,
            Err(error) => {
                return ConflictDetectOutcome::OpenFailed(Some(ConflictRestoreFailure {
                    observation: snapshot.observation,
                    cause: error.to_string(),
                }))
            }
        };
        let draft_restore_warnings = buffer.take_draft_restore_warnings();

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
                        buffer.ensure_hunks(&path, &markers.text, markers.marker_width);
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
            draft_restore_warnings,
            current_branch,
            selected_file,
            editing_file,
            editing_path,
        }))
    }

    fn present_conflict_restore_warnings(
        &mut self,
        outcome: &mut ConflictDetectOutcome,
        cx: &mut Context<Self>,
    ) {
        // Rescue already happened in background I/O and was recorded there.
        // Even a superseded request must announce its durable recovery handoff.
        let ConflictDetectOutcome::Detected(detected) = outcome else {
            return;
        };
        let warnings = std::mem::take(&mut detected.draft_restore_warnings);
        if warnings.is_empty() {
            return;
        }
        for warning in warnings {
            let name = warning.rescued_path.file_name().unwrap_or_default();
            let message = Msg::conflict_draft_rescued(&name.to_string_lossy());
            if let Some(error) = warning.recording_error {
                self.app_notices.push_back(
                    format!(
                        "{message}\n\n{}",
                        i18n::op_failed(i18n::Op::RecordOperation, error)
                    )
                    .into(),
                );
                self.present_app_notice();
                continue;
            }
            self.push_toast(ToastKind::Error, SharedString::from(message), cx);
        }
        cx.notify();
    }

    /// Foreground half of conflict detection: apply a [`ConflictDetectOutcome`]
    /// computed by [`detect_conflict_payload`] to `self`, emitting the same
    /// `[kagi]` contract lines in the same order as the original synchronous
    /// implementation. ADR-0118: this is the single point that builds / updates /
    /// drops the `Entity<ConflictView>` — `Detected` updates an existing entity in
    /// place (preserving its splits / editor inputs / before-text) or creates a
    /// new one; `Cleared` drops it, while a failed restore retains its live body
    /// behind the checked-read mutation gate.
    /// "was a conflict open?" (Cleared) and editor-close (Detected) checks read
    /// the entity here because they must reflect the current UI state at apply
    /// time. Needs `cx` (entity create / read / update).
    pub fn apply_conflict_detect(
        &mut self,
        owner: crate::app::Attachment,
        mut outcome: ConflictDetectOutcome,
        cx: &mut Context<Self>,
    ) {
        self.present_conflict_restore_warnings(&mut outcome, cx);
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
        // reload. A superseded answer cannot discard or re-authorize the
        // existing pane owned by an accepted detection.
        let accepted_operation = self.reads.get(Some(owner.session)).operation.clone();
        let accepted = accepted_operation.as_ref();
        let stale = match (&outcome, accepted) {
            (ConflictDetectOutcome::Detected(d), Some(op)) => op.observation != d.observation,
            (ConflictDetectOutcome::Detected(_), None) => false,
            (ConflictDetectOutcome::OpenFailed(Some(failure)), accepted) => accepted.map_or_else(
                || self.reads.has_read(owner.session),
                |op| op.observation != failure.observation,
            ),
            (
                ConflictDetectOutcome::Cleared | ConflictDetectOutcome::OpenFailed(None),
                accepted,
            ) => accepted.is_some(),
        };
        if stale {
            // A superseded outcome owns neither an existing pane nor its
            // pending revalidation. Rejected success is not a replacement:
            // keep its live Result until an accepted detection can adopt it.
            if matches!(outcome, ConflictDetectOutcome::OpenFailed(Some(_)))
                || self
                    .ui
                    .get(&owner.session)
                    .is_some_and(|ui| ui.conflict.is_some())
            {
                return;
            }
            // The read that superseded it re-detects on its own; this re-arm
            // covers the commit points that do not.
            if let Some(state) = self.ui.get_mut(&owner.session) {
                state.conflict_detected = false;
                // With no live pane there is no retained body to validate.
                // Preserve the first-open rearm behavior.
                if state.pane_revalidation == PaneRevalidation::AwaitingConflict {
                    state.pane_revalidation = PaneRevalidation::Settled;
                }
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
            if matches!(outcome, ConflictDetectOutcome::OpenFailed(_)) {
                self.app_sessions.clear_stash_conflict(owner.session);
            } else {
                self.app_sessions
                    .observe_stash_conflict(owner.session, identity);
            }
        }
        if let Some(ui) = self.ui.get_mut(&owner.session) {
            ui.conflict_restore_failed =
                matches!(&outcome, ConflictDetectOutcome::OpenFailed(Some(_)));
        }
        match outcome {
            ConflictDetectOutcome::OpenFailed(error) => {
                // A global observation can change while this exact file's
                // identity does not (for example, unrelated CLI staging). A
                // failed restore is never a replacement for its live editor.
                let retain = error.is_some()
                    && self
                        .ui
                        .get(&owner.session)
                        .is_some_and(|ui| ui.conflict.is_some());
                if let Some(error) = error {
                    self.record_operation_completion(
                        "conflict-draft-restore",
                        StateSummary {
                            head: "unobserved".into(),
                            dirty: "unobserved".into(),
                        },
                        OpOutcome::Failed { error: error.cause },
                        Some(Vec::new()),
                        &owner.path,
                        active_owner && exact_visit,
                        false,
                        Some(Msg::conflict_draft_restore_failed()),
                        cx,
                    );
                }
                if active_owner && exact_visit {
                    // A prior approval cannot outlive the failed checked read.
                    // The recorded failure already owns the bounded preview.
                    self.clear_conflict_continue_modal();
                    self.clear_conflict_abort_modal();
                }
                // This failure passed the observation/request freshness checks.
                // Keep the run-once guard until explicit reload/revalidation.
                if let Some(ui) = self.ui.get_mut(&owner.session) {
                    ui.conflict_detected = true;
                    if retain {
                        // A failed disk read cannot replace an already-owned
                        // Result. Keep the exact entity/input and its unsaved
                        // body, but do not acknowledge this checked pane read.
                        ui.pane_revalidation = PaneRevalidation::AwaitingConflict;
                    } else {
                        ui.pane_revalidation = PaneRevalidation::Settled;
                        ui.conflict = None;
                    }
                }
                if retain {
                    // Only a successful explicit re-detection may settle the
                    // existing mutation gate; the common tail must not do so.
                    return;
                }
            }
            ConflictDetectOutcome::Cleared => {
                if self.clear_conflict_after_draft_handoff(owner.clone(), cx) {
                    return;
                }
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
            ConflictDetectOutcome::Detected(detected) => {
                let accepted_view = self.reads.get(Some(owner.session));
                let current_commit = accepted_view
                    .head_oid
                    .as_ref()
                    .and_then(|head| accepted_view.rows.iter().find(|row| &row.id.0 == head))
                    .map(|row| format!("{} · {}", row.short_id, row.summary))
                    .unwrap_or_default();
                if let Some(ui) = self.ui.get_mut(&owner.session) {
                    ui.retire_issue_conversation_scope();
                }
                let ConflictDetected {
                    stash_identity: _,
                    session,
                    observation,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    editing_path,
                    draft_restore_warnings: _,
                } = *detected;
                if matches!(session.op, kagi_git::ConflictOp::Merge { .. })
                    && session.files.is_empty()
                {
                    klog!("conflict-mode: merge resolved — ready to commit");
                } else {
                    klog!(
                        "conflict-mode: {} {} file(s)",
                        session.op.slug(),
                        session.files.len()
                    );
                }

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
                    skip_approval: None,
                };

                match self
                    .ui
                    .get(&owner.session)
                    .and_then(|ui| ui.conflict.clone())
                {
                    // Re-detect against a retained pane. An **unchanged**
                    // observation must retain draft owners untouched (#722 P2):
                    // resolution buffer carries the undo/redo stack, and the
                    // entity carries the selected file/hunk and scroll, so
                    // swapping in a freshly read `mode` would silently discard
                    // the user's work. `revision` is a content fingerprint of
                    // the observed operation, so equality means the repository
                    // never moved. A changed one is folded in place below.
                    Some(entity)
                        if entity.read(cx).mode.as_ref().is_some_and(|current| {
                            current.revision == mode.revision
                                && current.session.files.iter().all(|file| {
                                    current.buffer.worktree_fingerprint(&file.path)
                                        == mode.buffer.worktree_fingerprint(&file.path)
                                        && current.buffer.worktree_changed(&file.path)
                                            == mode.buffer.worktree_changed(&file.path)
                                })
                        }) =>
                    {
                        entity.update(cx, |view, cx| {
                            view.current_commit = current_commit;
                            view.refresh_saved_reads(cx);
                        });
                    }
                    Some(entity) => {
                        entity.update(cx, |view, cx| {
                            view.current_commit = current_commit;
                            view.adopt_detected(mode, editing_path, cx);
                        });
                    }
                    // Continue handed this staged merge to CommitPanel. An
                    // accepted-read revalidation still checks the observation,
                    // but must not recreate the retired saved workspace.
                    None if self.ui.get(&owner.session).is_some_and(|ui| {
                        ui.conflict_merge_pending
                            && matches!(mode.session.op, kagi_git::ConflictOp::Merge { .. })
                            && mode.session.files.is_empty()
                    }) => {}
                    // Fresh conflict: build the entity, capturing the repo path +
                    // a weak back-ref for its deferred parent callbacks.
                    None => {
                        let weak_app = cx.weak_entity();
                        let repo_path = owner.path.clone();
                        let entity = cx.new(|cx| {
                            let mut view = conflict_view::ConflictView::new(
                                weak_app,
                                repo_path,
                                owner.clone(),
                                cx,
                            );
                            view.current_commit = current_commit;
                            view.adopt_detected(mode, editing_path, cx);
                            view
                        });
                        if let Some(ui) = self.ui.get_mut(&owner.session) {
                            ui.conflict = Some(entity);
                        }
                    }
                }
            }
        }
        // #722 P1: this detector ran against the accepted read (the guard at
        // the top drops any result that does not), so the retained conflict
        // pane has now been checked — updated in place, rebuilt, or dropped.
        // It settles the gate only for the pane pass that is waiting on it: a
        // detection that lands before a newer read is accepted, or before that
        // read's pass has run, must not re-admit Continue / Skip.
        if let Some(ui) = self.ui.get_mut(&owner.session) {
            if ui.pane_revalidation == PaneRevalidation::AwaitingConflict {
                ui.pane_revalidation = PaneRevalidation::Settled;
            }
        }
    }
    /// Detect (or clear) Conflict Mode for the active session.
    ///
    /// Runs at most once per owner per cycle. `reload()` and tab activation
    /// re-arm that owner's guard. Opens the repo read-only, calls
    /// `detect_conflict_session`, and on a hit builds a fresh
    /// `ResolutionBuffer` from the index (restoring only identity-matched
    /// autosaved drafts), recomputes each
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
            ui.conflict_detect_generation = ui.conflict_detect_generation.wrapping_add(1);
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
        let Some(ui) = self.ui_mut() else {
            return;
        };
        ui.conflict_detected = true;
        ui.conflict_detect_generation = ui.conflict_detect_generation.wrapping_add(1);
        let generation = ui.conflict_detect_generation;
        let read_key = self.reads.current_key(owner.session);

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
        #[cfg(feature = "gui-e2e")]
        let hold = DETECT_HOLD.with(|slot| slot.borrow_mut().take());
        cx.spawn(async move |this, acx| {
            let mut outcome = task.await;
            #[cfg(feature = "gui-e2e")]
            if let Some((hold, reached, expect_restore_error)) = hold {
                reached.store(
                    if expect_restore_error {
                        matches!(outcome, ConflictDetectOutcome::OpenFailed(Some(_)))
                    } else {
                        matches!(outcome, ConflictDetectOutcome::Detected(_))
                    },
                    std::sync::atomic::Ordering::Release,
                );
                hold.await;
            }
            let _ = this.update(acx, |app, cx| {
                // Content identity cannot distinguish requests for the same Git
                // state. Superseded successes cannot reopen a newer failed read
                // any more than superseded failures can replace its live pane.
                if !app.reads.is_fresh(read_key)
                    || app
                        .ui
                        .get(&owner.session)
                        .is_none_or(|ui| ui.conflict_detect_generation != generation)
                {
                    app.present_conflict_restore_warnings(&mut outcome, cx);
                    return;
                }
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
