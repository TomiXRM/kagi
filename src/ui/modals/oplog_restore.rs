//! Confirmation for an Operation Log op-revert / restore-to-point (#334
//! slice 2b, ADR-0214 §5).

use gpui::SharedString;
use kagi_domain::{ref_restore::RefRestore, restore_preview::RestorePreview};
use kagi_git::{Operation, OperationPlan};
use kagi_ui_core::i18n;

/// The after-restore graph shown on the card (#334 slice 2c): computed once
/// when the card opens, from the tab's loaded commits. Display only.
#[derive(Clone)]
pub struct RestoreGraphPreview {
    pub graph: RestorePreview,
    /// One-line summary per drawn row (same order as the preview's rows).
    pub summaries: Vec<SharedString>,
    /// Existing current-graph rail geometry, aligned with the preview rows.
    pub rails: Vec<kagi_domain::graph::GraphRow>,
    pub merges: Vec<bool>,
    /// Attached branch at planning; follows its predicted ref tip, not the old row.
    pub head_branch: Option<String>,
}

/// The planned `Operation::OpRevert` / `RestoreToPoint` and its card. The
/// plan is destructive, so `confirm_armed` gates the second confirm.
#[derive(Clone)]
pub struct OplogRestoreModal {
    pub op: Operation,
    pub plan: std::sync::Arc<OperationPlan>,
    /// Canonical plan rows decoded once on opening; the original plan stays intact.
    pub restores: Vec<RefRestore>,
    pub error: Option<SharedString>,
    pub confirm_armed: bool,
    /// `None` when the plan moves nothing (a blocked plan).
    pub preview: Option<std::sync::Arc<RestoreGraphPreview>>,
}

impl OplogRestoreModal {
    pub fn i18n_op(&self) -> i18n::Op {
        match self.op {
            Operation::OpRevert { .. } => i18n::Op::OpRevert,
            _ => i18n::Op::RestoreToPoint,
        }
    }

    /// The second stage names exactly how many refs are about to move.
    pub fn confirm_label(&self) -> String {
        if self.confirm_armed {
            i18n::oplog_panel::restore_confirm(self.restores.len())
        } else {
            i18n::Msg::OplogPanel(i18n::oplog_panel::OplogPanelMsg::RestoreConfirm)
                .t()
                .to_string()
        }
    }

    /// Two-stage (#354): tells assistive technology whether the next confirm runs it.
    pub fn confirm_stage(&self) -> crate::ui::dialog_a11y::ConfirmStage {
        use crate::ui::dialog_a11y::ConfirmStage;
        if self.confirm_armed {
            ConfirmStage::Armed
        } else {
            ConfirmStage::Unarmed
        }
    }
}
