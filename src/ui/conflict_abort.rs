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
//! The modal state and its accessors live beside the action rather than in
//! `modals.rs` / `modal_state.rs` — the conflict modals are the feature, and
//! both of those files are at their LOC ceiling.

use std::sync::Arc;

use gpui::{Context, SharedString};
use gpui_component::IconName;

use kagi_domain::conflict_family::InProgressOperation;
use kagi_git::OperationPlan;

use super::i18n::Msg;
use super::modal_renderers::render_plan_modal_wrapper_styled;
use super::modals::ActiveModal;
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
    pub operation: InProgressOperation,
    pub error: Option<SharedString>,
}

impl KagiApp {
    #[inline]
    pub fn conflict_continue_modal(&self) -> Option<&ConflictContinuePlanModal> {
        match &self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn conflict_continue_modal_mut(&mut self) -> Option<&mut ConflictContinuePlanModal> {
        match &mut self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_conflict_continue_modal(&mut self, m: ConflictContinuePlanModal) {
        self.active_modal = Some(ActiveModal::ConflictContinue(m));
    }
    #[inline]
    pub fn clear_conflict_continue_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ConflictContinue(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn conflict_abort_modal(&self) -> Option<&ConflictAbortModal> {
        match &self.active_modal {
            Some(ActiveModal::ConflictAbort(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_conflict_abort_modal(&mut self, m: ConflictAbortModal) {
        self.active_modal = Some(ActiveModal::ConflictAbort(m));
    }
    #[inline]
    pub fn clear_conflict_abort_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ConflictAbort(_))) {
            self.active_modal = None;
        }
    }

    /// Stage one of the abort: show what it will do.
    ///
    /// Admission comes from the read model's `operation`, never from the
    /// presence of a `ConflictView` — that entity is dropped the moment the
    /// last conflict is resolved, and #704 is what happened when it was the
    /// gate. The preview plan is a live read; the request that follows freezes
    /// the revision the strip showed.
    pub fn open_conflict_abort_modal(&mut self, cx: &mut Context<Self>) {
        if self.reject_if_busy(cx) {
            return;
        }
        let Some(operation) = self.view().operation.clone() else {
            return;
        };
        let Some(repo) = self.repo_session.as_ref().map(|session| session.backend()) else {
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
        if operation.slug == "stash" {
            let identity = repo.stash_conflict_identity().unwrap_or_default();
            if let Some(owner) = self.active_session() {
                self.app_sessions.observe_stash_conflict(owner, &identity);
            }
        }
        let plan = match repo.plan_operation_abort() {
            Ok(plan) => plan,
            Err(error) => {
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
        cx.notify();
    }

    pub fn cancel_conflict_abort(&mut self) {
        self.clear_conflict_abort_modal();
    }

    /// Stage two: hand the frozen request to the conflict family. The family
    /// owns admission (`begin_write`), the live preflight, execution,
    /// verification and the receipt — this method decides nothing.
    pub fn confirm_conflict_abort(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.conflict_abort_modal().cloned() else {
            return;
        };
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
        else {
            return;
        };
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
pub(crate) fn render_conflict_abort_modal(
    modal: ConflictAbortModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper_styled(
        modal.plan,
        modal.error,
        Msg::ConflictConfirmAbort.t(),
        None,
        Some((IconName::Undo2.into(), theme().color_blocker)),
        |this, _cx| this.cancel_conflict_abort(),
        |this, cx| this.confirm_conflict_abort(cx),
        cx,
    )
}
