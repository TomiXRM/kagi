//! Delete-branch confirmation state (W2-DELETE; unmerged branches arm a
//! second confirm, #584).

use gpui::SharedString;
use kagi_git::ops::OperationPlan;
use kagi_ui_core::i18n::Msg;

/// State for an in-progress delete-branch confirmation (W2-DELETE).
///
/// Unmerged deletion requires two confirmations; blockers still refuse.
/// Recovery retains the tip under a mandatory backup ref.
#[derive(Clone)]
pub struct DeleteBranchModal {
    /// Frozen plan owner, including its departure revision.
    pub owner: crate::app::Attachment,
    /// First confirmation arms only unmerged deletion; errors/reopening reset it.
    pub confirm_armed: bool,
    /// The local branch name to delete.
    pub branch_name: String,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if preflight or execute failed.
    pub error: Option<SharedString>,
}

impl DeleteBranchModal {
    /// Settle the global plan latch before deciding whether to display its
    /// result. Tab departure invalidates approval, not completion. An unrelated
    /// planning tag is never owned by this plan; the latch is `planning`,
    /// because planning writes nothing (ADR-0196 Wave 3).
    pub fn settle_plan(
        owner: &crate::app::Attachment,
        current: Option<&crate::app::Attachment>,
        planning: &mut Option<&'static str>,
    ) -> bool {
        if *planning == Some("delete-branch-plan") {
            *planning = None;
        }
        current
            .is_some_and(|current| current.session == owner.session && current.visit == owner.visit)
    }

    /// Returns true when this confirmation only arms; false permits the caller
    /// to continue through its existing blockers/busy/preflight checks.
    /// An unmerged branch needs the second confirm (#584).
    pub fn requires_arm(&self) -> bool {
        self.plan.warnings.iter().any(|note| {
            matches!(
                note,
                kagi_domain::plan_note::PlanNote::Branch(
                    kagi_domain::plan_note::BranchNote::DeleteUnmerged { .. }
                )
            )
        })
    }

    /// Confirm-button label: armed wording after the first confirm.
    pub fn confirm_label(&self) -> &'static str {
        if self.confirm_armed {
            Msg::PlanDeleteBranchArmed.t()
        } else {
            Msg::PlanDeleteBranch.t()
        }
    }

    /// Where this card is in its confirm sequence (#354).
    pub fn confirm_stage(&self) -> crate::ui::dialog_a11y::ConfirmStage {
        use crate::ui::dialog_a11y::ConfirmStage;
        match (self.confirm_armed, self.requires_arm()) {
            (true, _) => ConfirmStage::Armed,
            (false, true) => ConfirmStage::Unarmed,
            (false, false) => ConfirmStage::Single,
        }
    }

    pub fn arm_if_required(&mut self) -> bool {
        if self.requires_arm() && !self.confirm_armed {
            self.confirm_armed = true;
            true
        } else {
            false
        }
    }
}
