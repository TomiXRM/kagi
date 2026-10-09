//! Branch-menu "Sync" group plan state (pull ff-only / push / push + set
//! upstream / sync-to-remote) and how it labels the shared plan card.

use gpui::SharedString;
use kagi_git::ops::OperationPlan;
use kagi_ui_core::i18n::Msg;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPlanKind {
    /// #536: match the current branch, index and working tree to the
    /// upstream after retaining everything local (destructive → armed).
    SyncToRemote,
    PullFfOnly,
    Push,
    PushSetUpstream,
}

#[derive(Clone)]
pub struct BranchPlanModal {
    pub kind: BranchPlanKind,
    pub branch_name: String,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Two-stage confirm gate for a `destructive` plan (SyncToRemote):
    /// `false` = first click pending, `true` = armed.
    pub confirm_armed: bool,
    /// Only this visit's successful fetch can retain the unchanged confirmation.
    pub fetch_owner: Option<(crate::app::SessionId, u64)>,
    pub dirty_digest: Option<kagi_domain::status::WorktreeDigest>,
    /// Local tip, upstream name and upstream tip; external ref/config moves sweep it.
    pub fetched_refs: Option<(
        kagi_domain::commit::CommitId,
        String,
        kagi_domain::commit::CommitId,
    )>,
}

impl BranchPlanModal {
    /// Sync-to-remote is two-stage; pull / push are a single confirm (#354).
    pub fn confirm_stage(&self) -> crate::ui::dialog_a11y::ConfirmStage {
        use crate::ui::dialog_a11y::ConfirmStage;
        match self.kind {
            BranchPlanKind::SyncToRemote => ConfirmStage::two_stage(self.confirm_armed),
            _ => ConfirmStage::Single,
        }
    }

    /// Confirm-button label. Sync-to-remote is destructive, so the label
    /// swaps to the armed wording after the first confirm (ADR-0023).
    pub fn confirm_label(&self) -> SharedString {
        match self.kind {
            BranchPlanKind::PullFfOnly => "Pull".into(),
            BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => "Push".into(),
            BranchPlanKind::SyncToRemote if self.confirm_armed => {
                Msg::PlanSyncToRemoteArmed.t().into()
            }
            BranchPlanKind::SyncToRemote => Msg::PlanSyncToRemote.t().into(),
        }
    }
}
