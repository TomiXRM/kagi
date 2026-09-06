//! The plan slot an input-bearing modal carries, and the two adapters that keep
//! a `plan_*` result and its rendering on one path (#510).
//!
//! Split out of `modals.rs` on the LOC gate: this is plan-state glue, not modal
//! state. The state machine itself lives in [`crate::app::PlanSlot`], which is
//! pure and unit-tested there.

use crate::app::PlanSlot;
use gpui::SharedString;
use kagi_git::OperationPlan;
use kagi_ui_core::i18n;

/// The plan slot every input-bearing modal carries: a plan for the current
/// input, or the explicit failure that replaced it (#510). `Pending`/`Failed`
/// hold no plan, so the confirm paths refuse Enter and hide the button.
pub type ModalPlan = PlanSlot<std::sync::Arc<OperationPlan>>;

/// The single error line an input-bearing modal shows: the plan failure that
/// invalidated the slot (#510), or the execute/preflight failure kept on the
/// modal's own `error` field. Never both — a plan failure left no plan to run.
pub(crate) fn plan_or_exec_error(
    plan: &ModalPlan,
    error: Option<SharedString>,
) -> Option<SharedString> {
    error.or_else(|| plan.error().map(SharedString::from))
}

/// One `plan_*` result in the shape [`ModalPlan::replan`] takes: the plan behind
/// an `Arc`, or the localized plan-failure text the modal renders (#510).
pub(crate) fn plan_outcome(
    op: i18n::Op,
    result: Result<OperationPlan, kagi_git::GitError>,
) -> Result<std::sync::Arc<OperationPlan>, String> {
    result
        .map(std::sync::Arc::new)
        .map_err(|e| i18n::op_plan_failed(op, e))
}
