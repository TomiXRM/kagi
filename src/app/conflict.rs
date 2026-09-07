//! Conflict Save / directory-file application boundary (#484 C1).
use super::*;
use kagi_domain::conflict_family::{ConflictObservation, ConflictRequest};
use kagi_git::backend::conflict_ops::{
    ConflictFaultPoint, ConflictPlan, ConflictReport, ConflictSnapshot,
};
use kagi_git::backend::ExecutionPolicy;

#[derive(Clone, Debug)]
pub struct ConflictAppRequest {
    pub owner: Attachment,
    pub request: ConflictRequest,
}

pub struct ConflictPlanJob {
    revision: RequestId,
    request: ConflictAppRequest,
    policy: ExecutionPolicy,
}

impl ConflictPlanJob {
    pub fn run(self) -> PlanCompletion {
        let state = match kagi_git::Backend::plan_recorded_conflict(
            &self.request.owner.path,
            self.request.request.clone(),
        ) {
            Ok(plan) => PlanState::Ready {
                token: PlanToken {
                    revision: self.revision,
                },
                prepared: Planned::Conflict {
                    plan: Box::new(plan),
                    request: self.request,
                    policy: self.policy,
                },
            },
            Err(error) => PlanState::Error {
                error: error.to_string(),
                open_failed: matches!(
                    error,
                    kagi_git::GitError::PathNotFound(_)
                        | kagi_git::GitError::NotARepository(_)
                        | kagi_git::GitError::BareRepository(_)
                ),
                recording: Some(kagi_git::Backend::record_conflict_refusal(
                    &self.request.owner.path,
                    self.policy,
                    &self.request.request,
                    &error.to_string(),
                )),
            },
        };
        PlanCompletion {
            revision: self.revision,
            state,
            error_job: None,
        }
    }
}

pub fn plan_conflict(
    sessions: &mut Sessions,
    request: ConflictAppRequest,
    policy: ExecutionPolicy,
) -> ConflictPlanJob {
    sessions.invalidate_plan();
    sessions.plan_owner = Some(request.owner.session);
    sessions.state = PlanState::Planning {
        request: sessions.revision,
    };
    ConflictPlanJob {
        revision: sessions.revision,
        request,
        policy,
    }
}

pub struct ConflictJob {
    id: OperationId,
    plan: Box<ConflictPlan>,
    policy: ExecutionPolicy,
    fault: Option<ConflictFaultPoint>,
    abandoned: std::sync::mpsc::Sender<Completion>,
    ran: bool,
}

impl ConflictJob {
    pub fn id(&self) -> OperationId {
        self.id
    }

    #[doc(hidden)]
    pub fn with_fault_for_test(mut self, fault: ConflictFaultPoint) -> Self {
        self.fault = Some(fault);
        self
    }

    pub fn run(mut self) -> ConflictCompletion {
        self.ran = true;
        let report = kagi_git::Backend::run_recorded_conflict(&self.plan, self.policy, self.fault);
        ConflictCompletion {
            id: self.id,
            report,
        }
    }
}

impl Drop for ConflictJob {
    fn drop(&mut self) {
        if self.ran {
            return;
        }
        let report = kagi_git::Backend::conflict_abandoned(&self.plan, self.policy);
        let _ = self
            .abandoned
            .send(Completion::Conflict(Box::new(ConflictCompletion {
                id: self.id,
                report,
            })));
    }
}

#[derive(Clone, Debug)]
pub struct ConflictCompletion {
    pub(crate) id: OperationId,
    pub(crate) report: ConflictReport,
}

impl ConflictCompletion {
    pub fn report(&self) -> &ConflictReport {
        &self.report
    }
}

pub fn prepare_conflict(
    sessions: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
) -> Result<ConflictJob, AdmissionError> {
    let Planned::Conflict { plan, .. } = &approved.prepared else {
        return Err(AdmissionError::StaleApproval);
    };
    let id = reserve(sessions, &approved, legacy)?;
    sessions.conflict_states.insert(
        approved.prepared.owner_session(),
        ConflictOwnerState::InFlight {
            operation: id,
            revision: plan.request().revision().clone(),
        },
    );
    let Planned::Conflict { plan, policy, .. } = approved.prepared else {
        unreachable!()
    };
    Ok(ConflictJob {
        id,
        plan,
        policy,
        fault: None,
        abandoned: sessions.abandoned_tx.clone(),
        ran: false,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictOwnerState {
    Observed(ConflictObservation),
    InFlight {
        operation: OperationId,
        revision: kagi_domain::conflict_family::ConflictRevision,
    },
    Settled(Option<ConflictObservation>),
}

impl Sessions {
    pub fn observe_conflict(&mut self, owner: SessionId, observation: Option<ConflictObservation>) {
        if !self.is_attached(owner)
            || matches!(
                self.conflict_states.get(&owner),
                Some(ConflictOwnerState::InFlight { .. })
            )
        {
            return;
        }
        match observation {
            Some(value) => {
                self.conflict_states
                    .insert(owner, ConflictOwnerState::Observed(value));
            }
            None => {
                self.conflict_states.remove(&owner);
            }
        }
    }

    pub fn conflict_state(&self, owner: SessionId) -> Option<&ConflictOwnerState> {
        self.conflict_states.get(&owner)
    }

    pub fn conflict_snapshot(
        path: &std::path::Path,
    ) -> Result<Option<ConflictSnapshot>, kagi_git::GitError> {
        kagi_git::Backend::open(path)?.conflict_snapshot()
    }
}
