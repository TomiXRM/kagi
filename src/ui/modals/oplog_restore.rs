//! Confirmation for an Operation Log op-revert / restore-to-point (#334
//! slice 2b, ADR-0214 §5).

use gpui::SharedString;
use kagi_git::{Operation, OperationPlan};
use kagi_ui_core::i18n;

/// The planned `Operation::OpRevert` / `RestoreToPoint` and its card. The
/// plan is destructive, so `confirm_armed` gates the second confirm.
#[derive(Clone)]
pub struct OplogRestoreModal {
    pub op: Operation,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    pub confirm_armed: bool,
}

impl OplogRestoreModal {
    pub fn i18n_op(&self) -> i18n::Op {
        match self.op {
            Operation::OpRevert { .. } => i18n::Op::OpRevert,
            _ => i18n::Op::RestoreToPoint,
        }
    }

    /// The first confirm names the action; the second states what it does.
    pub fn confirm_label(&self) -> String {
        if self.confirm_armed {
            format!("\u{26a0} {}", i18n::oplog_panel::restore_armed())
        } else {
            self.i18n_op().t().to_string()
        }
    }

    /// The plan to draw. Its `preview_commits` are the `restore …` lines the
    /// backend re-checks, not commits; the user-facing list is the `Moves`
    /// warnings (the reverse actions) and `RefsOnly` (what is not restored).
    pub fn display_plan(&self) -> std::sync::Arc<OperationPlan> {
        std::sync::Arc::new(OperationPlan {
            preview_commits: Vec::new(),
            ..(*self.plan).clone()
        })
    }
}
