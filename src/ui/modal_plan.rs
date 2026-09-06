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

/// The single error line an input-bearing modal shows.
///
/// The **slot wins**: an execute/preflight failure on the modal's own `error`
/// field outlives the replan that follows it (nothing clears `error` unless the
/// input changes), so re-confirming after a failed execute can leave `Failed`
/// and `error` set at once. The slot's failure is the reason confirm is refused
/// *now*, so that is what the user is shown; `error` explains the last attempt
/// and surfaces whenever the slot itself has nothing to say.
pub(crate) fn plan_or_exec_error(
    plan: &ModalPlan,
    error: Option<SharedString>,
) -> Option<SharedString> {
    plan.error().map(SharedString::from).or(error)
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

/// The per-tab `RepoSession` every synchronous replan plans against is gone
/// (open failed, or the view came from cache). That is a plan failure like any
/// other — not a silent return that leaves the modal `Pending` (#510).
pub(crate) fn session_unavailable<T>(op: i18n::Op) -> Result<T, String> {
    Err(i18n::op_plan_failed(op, SESSION_UNAVAILABLE))
}

/// Diagnostic half of a session-acquisition failure. Untranslated, like every
/// other backend diagnostic; `op_plan_failed` supplies the localized frame.
pub(crate) const SESSION_UNAVAILABLE: &str = "repository session unavailable";

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(text: &str) -> Option<SharedString> {
        Some(SharedString::from(text.to_string()))
    }

    #[test]
    fn a_plan_failure_outranks_a_stale_execute_error() {
        let failed = ModalPlan::Failed("repo went away".into());
        assert_eq!(
            plan_or_exec_error(&failed, shared("execute failed")),
            shared("repo went away"),
        );
    }

    #[test]
    fn the_execute_error_shows_while_the_slot_has_nothing_to_say() {
        for slot in [ModalPlan::Pending, ModalPlan::default()] {
            assert_eq!(
                plan_or_exec_error(&slot, shared("execute failed")),
                shared("execute failed"),
            );
        }
        assert_eq!(plan_or_exec_error(&ModalPlan::Pending, None), None);
    }
}
