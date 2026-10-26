//! Aborting the in-progress operation from the UI (#704, Conflict C2).
//!
//! The one path: the header operation strip (or the conflict dashboard, which
//! dispatches the same action) opens the plan confirmation, and confirming
//! sends a `ConflictRequest::Abort` through the conflict family — plan →
//! confirm → `begin_write` → Backend preflight → execute → verify → receipt.
//! Nothing here touches the repository, and nothing here needs a
//! `ConflictView` to exist: the operation comes from the session's read model,
//! which is the whole point of the issue.
//!
//! The modal state structs live here with the action; their accessors live
//! with every other modal accessor, in `operations/modal_state/conflict.rs`
//! (#707 review — `active_modal` has one owner).

use std::sync::Arc;

use gpui::{Context, SharedString, Window};
use gpui_component::IconName;

use kagi_domain::conflict_family::{ConflictOperationKind, ConflictRevision, ObservedOperation};
use kagi_git::OperationPlan;

use super::dialog_a11y::ConfirmStage;
use super::i18n::Msg;
use super::modal_renderers::render_plan_modal_wrapper_staged;
use super::theme::theme;
use super::{KagiApp, ToastKind};
use crate::app;

/// State for a sequencer (rebase / cherry-pick / revert) conflict-continue
/// confirmation (ADR-0068 / T-CONFLICT-FLOW-032).  A `git <op> --continue` plan
/// shown before the sequencer is advanced.  Merge does NOT use this modal — it
/// routes to the commit message panel instead.
#[derive(Clone)]
pub struct ConflictContinuePlanModal {
    /// The computed `<op> --continue` plan.
    pub plan: Arc<OperationPlan>,
    /// The accepted full repository revision shown by this original plan,
    /// including staged content. Never replace it from the mode at Confirm.
    pub revision: ConflictRevision,
    pub operation_kind: ConflictOperationKind,
    /// Error message to show if execute failed (replaces the confirm button).
    pub error: Option<SharedString>,
}

/// State for the two-stage abort confirmation (#704).
///
/// Stage one is opening this from the operation strip; stage two is its
/// Confirm. The `operation` is the observation the strip was rendered from —
/// frozen here so the request carries the revision the *user saw*, and the
/// Backend refuses it if the repository has moved on since.
#[derive(Clone)]
pub struct ConflictAbortModal {
    pub plan: Arc<OperationPlan>,
    pub operation: ObservedOperation,
    pub error: Option<SharedString>,
}

impl KagiApp {
    /// Stage one of the abort: show what it will do.
    ///
    /// Admission comes from the read model's `operation`, never from the
    /// presence of a `ConflictView` — that entity is dropped the moment the
    /// last conflict is resolved, and #704 is what happened when it was the
    /// gate. The preview plan is a live read; the request that follows freezes
    /// the revision the strip showed.
    ///
    /// #755: opening also returns window focus to the root, the same handle
    /// every modal's Cancel / Confirm already restores. Escape reaches
    /// `cancel_active_modal` through one action (`CloseMainDiff`), and an
    /// action only runs along the dispatch path of the *rendered* focused
    /// element. The conflict screen can leave focus on an element that is no
    /// longer drawn — the Result pane mounts its `InputState` only in Edit
    /// mode (`conflict_editor.rs`) — and gpui then falls back to the dispatch
    /// tree's root, where no handler and no key context exist, so Escape (and
    /// every other window action) silently does nothing. Taking focus with
    /// the confirmation keeps the one Esc route alive whatever the user was
    /// last in.
    pub fn open_conflict_abort_modal(
        &mut self,
        owner: crate::app::Attachment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.conflict_action_owner_on_screen(&owner) {
            return;
        }
        let handle = window.window_handle();
        self.flush_conflict_drafts(owner.clone(), "abort", cx, move |app, cx| {
            let token = app
                .ui()
                .conflict
                .as_ref()
                .map(|pane| pane.read(cx).intent_token);
            let this = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        let current = app
                            .ui()
                            .conflict
                            .as_ref()
                            .map(|pane| pane.read(cx).intent_token);
                        if current != token {
                            return;
                        }
                        app.open_conflict_abort_modal_flushed(owner, window, cx)
                    });
                });
            });
        });
    }

    fn open_conflict_abort_modal_flushed(
        &mut self,
        owner: crate::app::Attachment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.conflict_action_owner_on_screen(&owner) {
            return;
        }
        if self.reject_if_busy(cx) {
            return;
        }
        let Some(operation) = self.view().operation.clone() else {
            return;
        };
        if self.refuse_conflict_abort_for_failed_draft(&owner, &operation, cx) {
            return;
        }
        let repo_session = self.ui().repo_session.clone();
        let Some(repo) = repo_session.as_ref().map(|session| session.backend()) else {
            self.push_toast(
                ToastKind::Error,
                SharedString::from(super::i18n::op_failed(
                    super::i18n::Op::RepoOpen,
                    "session unavailable",
                )),
                cx,
            );
            return;
        };
        // #309: a stash conflict is identified by its entry, not by a ref, so
        // the owner has to have observed that identity before it can approve
        // anything against it.
        if operation.kind() == ConflictOperationKind::StashApply {
            let identity = repo.stash_conflict_identity().unwrap_or_default();
            if let Some(owner) = self.active_session() {
                self.app_sessions.observe_stash_conflict(owner, &identity);
            }
        }
        let plan = match repo.plan_operation_abort() {
            Ok(plan) => plan,
            Err(error) => {
                if let Some(recording) = error.recording() {
                    self.notice_recording_failure(
                        &recording.entry().op,
                        recording,
                        std::path::Path::new(&recording.entry().repo),
                        None,
                    );
                    self.present_recorded(recording, true, cx);
                    self.present_app_notice();
                    cx.notify();
                    return;
                }
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(super::i18n::op_plan_failed(super::i18n::Op::Abort, error)),
                    cx,
                );
                return;
            }
        };
        klog!("conflict-mode: abort armed (second confirm required)");
        self.set_conflict_abort_modal(ConflictAbortModal {
            plan: Arc::new(plan),
            operation,
            error: None,
        });
        if let Some(handle) = self.root_focus.clone() {
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    /// A disk-loaded preservation buffer cannot stand in for a newer failed
    /// live draft, even after filesystem permissions have recovered.
    pub(crate) fn refuse_conflict_abort_for_failed_draft(
        &mut self,
        owner: &crate::app::Attachment,
        operation: &ObservedOperation,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.failed_conflict_draft_for(owner.session, cx) {
            return false;
        }
        let Some(repo_path) = self
            .tabs
            .iter()
            .find(|tab| tab.session == owner.session)
            .map(|tab| tab.path.clone())
        else {
            return false;
        };
        let blocker = kagi_domain::plan_note::PlanNote::Conflicts(
            kagi_domain::plan_note::ConflictsNote::DraftPreservationFailed,
        );
        let message = super::i18n::plan_note_text(&blocker);
        let entry = Self::refused_entry(
            &format!("{}-abort", operation.kind().slug()),
            kagi_domain::plan::StateSummary {
                head: format!("op={}", operation.kind().slug()),
                dirty: "unsaved decisions".into(),
            },
            &[blocker],
            &repo_path,
        );
        self.record_op_result(
            entry,
            cx,
            true,
            None,
            Some((ToastKind::Error, message.into())),
            true,
            true,
            None,
            false,
            None,
            None,
        );
        true
    }

    pub fn cancel_conflict_abort(&mut self) {
        self.clear_conflict_abort_modal();
    }

    /// Stage two: require the checked pane and recheck draft preservation before
    /// handing the frozen request to the family, which owns writer admission,
    /// live repository preflight, execution, verification and the receipt.
    pub fn confirm_conflict_abort(&mut self, cx: &mut Context<Self>) {
        let Some(owner) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
        else {
            return;
        };
        self.flush_conflict_drafts(owner, "abort", cx, |app, cx| {
            app.confirm_conflict_abort_flushed(cx)
        });
    }

    fn confirm_conflict_abort_flushed(&mut self, cx: &mut Context<Self>) {
        if !self
            .active_session()
            .is_some_and(|owner| self.pane_mutation_admitted(owner))
        {
            self.clear_conflict_abort_modal();
            cx.notify();
            return;
        }
        let Some(modal) = self.conflict_abort_modal().cloned() else {
            return;
        };
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
        else {
            return;
        };
        if self.refuse_conflict_abort_for_failed_draft(&owner, &modal.operation, cx) {
            self.clear_conflict_abort_modal();
            cx.notify();
            return;
        }
        let request = app::ConflictAppRequest {
            owner,
            request: kagi_git::Backend::conflict_abort_request(&modal.operation),
        };
        // A refused plan is reported by its own recording (toast + oplog +
        // notice); the plan this modal was showing is void either way.
        if !self.start_conflict_request(request, cx) {
            self.clear_conflict_abort_modal();
        }
    }
}

/// Abort confirmation overlay (#704) — the same plan card every other
/// destructive confirmation uses, so the preview, warnings and recovery
/// notes come from `plan_conflict_abort` rather than being re-worded here.
/// The strip/dashboard click was the first confirm, so the card opens armed:
/// its Confirm runs the abort, and assistive technology is told so (#354).
pub(crate) fn render_conflict_abort_modal(
    modal: ConflictAbortModal,
    // #462: the user's modal-section disclosure choices, borrowed for this
    // frame (see `modal_renderers_plan`'s module doc).
    overrides: &super::modal_shell::ModalSections,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper_staged(
        modal.plan,
        modal.error,
        Msg::ConflictConfirmAbort.t(),
        None,
        Some((IconName::Undo2.into(), theme().color_blocker)),
        ConfirmStage::Armed,
        |this, _cx| this.cancel_conflict_abort(),
        |this, cx| this.confirm_conflict_abort(cx),
        overrides,
        cx,
    )
}
