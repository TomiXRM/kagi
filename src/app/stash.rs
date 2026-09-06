use super::*;
pub use kagi_git::backend::stash::{StashAction, StashFaultPoint};
use kagi_git::backend::stash::{StashPlan, StashReport};
use kagi_git::Backend;

pub use kagi_git::backend::ExecutionPolicy as StashPolicy;
#[derive(Clone, Debug)]
pub struct StashRequest {
    pub owner: Attachment,
    pub action: StashAction,
}
pub struct StashPlanJob {
    revision: RequestId,
    request: StashRequest,
    policy: StashPolicy,
    expected_oid: Option<String>,
}
impl StashPlanJob {
    pub fn run(mut self) -> PlanCompletion {
        let result = if let Some(oid) = &self.expected_oid {
            Backend::plan_stash_drop_by_oid(&self.request.owner.path, oid)
        } else {
            Backend::plan_recorded_stash(&self.request.owner.path, self.request.action.clone())
                .map(Some)
        };
        if let Ok(Some(plan)) = &result {
            self.request.action = plan.action.clone();
        }
        let (state, error_job) = match result {
            Ok(None) => (PlanState::Draft, None),
            Ok(Some(plan)) => (
                PlanState::Ready {
                    token: PlanToken {
                        revision: self.revision,
                    },
                    prepared: Planned::Stash {
                        plan,
                        request: self.request,
                        policy: self.policy,
                    },
                },
                None,
            ),
            Err(e) => (
                PlanState::Error {
                    open_failed: matches!(
                        &e,
                        kagi_git::GitError::PathNotFound(_)
                            | kagi_git::GitError::NotARepository(_)
                            | kagi_git::GitError::BareRepository(_)
                    ),
                    error: e.to_string(),
                    recording: None,
                },
                Some(PlanErrorEvidence {
                    revision: self.revision,
                    request: self.request,
                    policy: self.policy,
                    error: e.to_string(),
                }),
            ),
        };
        PlanCompletion {
            revision: self.revision,
            state,
            error_job,
        }
    }
}
pub fn plan_stash(s: &mut Sessions, request: StashRequest, policy: StashPolicy) -> StashPlanJob {
    s.invalidate_plan();
    s.plan_owner = Some(request.owner.session);
    s.state = PlanState::Planning {
        request: s.revision,
    };
    StashPlanJob {
        revision: s.revision,
        request,
        policy,
        expected_oid: None,
    }
}
pub fn plan_stash_followup(
    s: &mut Sessions,
    owner: Attachment,
    oid: String,
    policy: StashPolicy,
) -> StashPlanJob {
    let mut job = plan_stash(
        s,
        StashRequest {
            owner,
            action: StashAction::Drop { index: 0 },
        },
        policy,
    );
    job.expected_oid = Some(oid);
    job
}
pub struct StashJob {
    id: OperationId,
    plan: StashPlan,
    policy: StashPolicy,
    fault: Option<StashFaultPoint>,
    abandoned: std::sync::mpsc::Sender<Completion>,
    ran: bool,
}
impl StashJob {
    pub fn id(&self) -> OperationId {
        self.id
    }
    #[doc(hidden)]
    pub fn with_fault_for_test(mut self, fault: StashFaultPoint) -> Self {
        self.fault = Some(fault);
        self
    }
    pub fn run(self) -> StashCompletion {
        self.run_with_events(|_| {})
    }
    pub fn run_with_events(
        mut self,
        event: impl FnMut(kagi_git::backend::stash::StashEvent),
    ) -> StashCompletion {
        // Mark consumed before execution: unwinding after finalize must not record abandonment.
        self.ran = true;
        let report = Backend::run_recorded_stash(
            &self.plan,
            self.policy.actor,
            self.policy.auto_snapshot,
            self.fault,
            event,
        );
        StashCompletion {
            id: self.id,
            report,
        }
    }
}
impl Drop for StashJob {
    fn drop(&mut self) {
        if !self.ran {
            let report = kagi_git::backend::stash::stash_failure(
                &self.plan,
                self.policy.actor,
                "job dropped before execution",
                kagi_git::backend::stash::StashStopReason::Abandoned,
            );
            let _ = self.abandoned.send(Completion::Stash(StashCompletion {
                id: self.id,
                report,
            }));
        }
    }
}
#[derive(Clone, Debug)]
pub struct StashCompletion {
    pub(crate) id: OperationId,
    pub(crate) report: StashReport,
}
impl StashCompletion {
    pub fn report(&self) -> &StashReport {
        &self.report
    }
}
pub fn prepare_stash(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
) -> Result<StashJob, AdmissionError> {
    if !matches!(approved.prepared, Planned::Stash { .. }) {
        return Err(AdmissionError::StaleApproval);
    }
    let id = reserve(s, &approved, legacy)?;
    let Planned::Stash { plan, policy, .. } = approved.prepared else {
        unreachable!()
    };
    Ok(StashJob {
        id,
        plan,
        policy,
        fault: None,
        abandoned: s.abandoned_tx.clone(),
        ran: false,
    })
}
#[derive(Clone, Debug)]
pub struct StashConflict {
    pub operation: OperationId,
    pub oid: String,
    pub(crate) identity: Vec<String>,
    pub(crate) pending: bool,
}
/// #482 stage 1: the conflict and its follow-up proposal belong to the session
/// that approved the stash, not to a path. Closing the tab detaches the session
/// and the payloads go with it, so a reopened tab on the same path starts clean.
impl Sessions {
    pub fn observe_stash_conflict(&mut self, owner: SessionId, identity: &[String]) {
        let Some(payload) = self.stash_conflicts.get(&owner) else {
            return;
        };
        if payload.pending && identity.is_empty() {
            let payload = self
                .stash_conflicts
                .remove(&owner)
                .expect("stash conflict was observed above");
            self.stash_followups.insert(owner, payload);
        } else if payload.pending
            || identity.is_empty()
            || !identity
                .iter()
                .all(|entry| payload.identity.contains(entry))
        {
            self.clear_stash_conflict(owner);
        }
    }
    pub fn stash_conflict(&self, owner: SessionId) -> Option<&StashConflict> {
        self.stash_conflicts.get(&owner)
    }
    pub fn clear_stash_conflict(&mut self, owner: SessionId) {
        self.stash_conflicts.remove(&owner);
        self.stash_followups.remove(&owner);
    }
    pub fn continue_stash_conflict(&mut self, owner: SessionId) {
        if let Some(payload) = self.stash_conflicts.get_mut(&owner) {
            payload.pending = true;
        }
    }
    pub fn take_stash_followup(&mut self, owner: SessionId) -> Option<StashConflict> {
        self.stash_followups.remove(&owner)
    }
}
