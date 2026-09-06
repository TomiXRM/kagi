use super::*;
pub use kagi_git::backend::stash::{StashAction, StashFaultPoint};
use kagi_git::backend::stash::{StashPlan, StashReport};
use kagi_git::Backend;
use std::path::Path;

pub use kagi_git::backend::ExecutionPolicy as StashPolicy;
#[derive(Clone, Debug)]
pub struct StashRequest {
    pub owner: Attachment,
    pub action: StashAction,
}
#[derive(Clone, Debug)]
pub struct RemoteStashRequest {
    pub owner: crate::remote::stash::RemoteAttachment,
    pub index: usize,
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
pub struct RemoteStashPlanJob {
    revision: RequestId,
    request: RemoteStashRequest,
    policy: StashPolicy,
    fixture: Option<crate::remote::stash::RemotePlanFixture>,
}
impl RemoteStashPlanJob {
    pub fn run(self) -> PlanCompletion {
        let result = if let Some(fixture) = self.fixture {
            crate::remote::stash::plan_remote_stash_drop_for_test(
                self.request.owner.clone(),
                self.request.index,
                fixture,
            )
        } else {
            crate::remote::stash::plan_remote_stash_drop(
                self.request.owner.clone(),
                self.request.index,
            )
        };
        let state = match result {
            Ok(plan) => PlanState::Ready {
                token: PlanToken {
                    revision: self.revision,
                },
                prepared: Planned::RemoteStash {
                    plan: Box::new(plan),
                    request: self.request,
                    policy: self.policy,
                },
            },
            Err(error) => PlanState::Error {
                error: error.to_string(),
                open_failed: false,
                recording: None,
            },
        };
        PlanCompletion {
            revision: self.revision,
            state,
            error_job: None,
        }
    }
}
pub fn plan_remote_stash(
    sessions: &mut Sessions,
    request: RemoteStashRequest,
    policy: StashPolicy,
) -> RemoteStashPlanJob {
    sessions.invalidate_plan();
    sessions.state = PlanState::Planning {
        request: sessions.revision,
    };
    RemoteStashPlanJob {
        revision: sessions.revision,
        request,
        policy,
        fixture: None,
    }
}

#[doc(hidden)]
pub fn plan_remote_stash_for_test(
    sessions: &mut Sessions,
    request: RemoteStashRequest,
    policy: StashPolicy,
    fixture: crate::remote::stash::RemotePlanFixture,
) -> RemoteStashPlanJob {
    let mut job = plan_remote_stash(sessions, request, policy);
    job.fixture = Some(fixture);
    job
}

pub struct LocalStashJob {
    id: OperationId,
    plan: StashPlan,
    policy: StashPolicy,
    fault: Option<StashFaultPoint>,
    abandoned: std::sync::mpsc::Sender<Completion>,
    ran: bool,
}
impl LocalStashJob {
    fn run_with_events(
        &mut self,
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
            report: StashExecutionReport::Local(Box::new(report)),
        }
    }
}
impl Drop for LocalStashJob {
    fn drop(&mut self) {
        if !self.ran {
            let report = kagi_git::backend::stash::stash_failure(
                &self.plan,
                self.policy.actor,
                "job dropped before execution",
                kagi_git::backend::stash::StashStopReason::Abandoned,
            );
            let _ = self
                .abandoned
                .send(Completion::Stash(Box::new(StashCompletion {
                    id: self.id,
                    report: StashExecutionReport::Local(Box::new(report)),
                })));
        }
    }
}
#[derive(Clone, Debug)]
pub struct StashCompletion {
    pub(crate) id: OperationId,
    pub(crate) report: StashExecutionReport,
}
#[derive(Clone, Debug)]
pub enum StashExecutionReport {
    Local(Box<StashReport>),
    Remote(Box<crate::remote::stash::RemoteStashReport>),
}
impl StashCompletion {
    pub fn report(&self) -> &StashReport {
        match &self.report {
            StashExecutionReport::Local(report) => report,
            StashExecutionReport::Remote(_) => {
                panic!("remote stash completion has a remote report")
            }
        }
    }
    pub fn remote_report(&self) -> Option<&crate::remote::stash::RemoteStashReport> {
        match &self.report {
            StashExecutionReport::Remote(report) => Some(report),
            StashExecutionReport::Local(_) => None,
        }
    }
}
pub enum StashJob {
    Local(LocalStashJob),
    Remote {
        id: OperationId,
        plan: Box<crate::remote::stash::RemoteStashPlan>,
        policy: StashPolicy,
        fault: crate::remote::stash::RemoteStashFault,
        abandoned: std::sync::mpsc::Sender<Completion>,
        ran: bool,
    },
}
impl StashJob {
    pub fn id(&self) -> OperationId {
        match self {
            Self::Local(job) => job.id,
            Self::Remote { id, .. } => *id,
        }
    }
    #[doc(hidden)]
    pub fn with_fault_for_test(mut self, fault: StashFaultPoint) -> Self {
        if let Self::Local(job) = &mut self {
            job.fault = Some(fault);
        }
        self
    }
    #[doc(hidden)]
    pub fn with_remote_fault_for_test(
        mut self,
        fault: crate::remote::stash::RemoteStashFault,
    ) -> Self {
        if let Self::Remote { fault: current, .. } = &mut self {
            *current = fault;
        }
        self
    }
    pub fn run(self) -> StashCompletion {
        self.run_with_events(|_| {})
    }
    pub fn run_with_events(
        mut self,
        event: impl FnMut(kagi_git::backend::stash::StashEvent),
    ) -> StashCompletion {
        match &mut self {
            Self::Local(job) => job.run_with_events(event),
            Self::Remote {
                id,
                plan,
                policy,
                fault,
                abandoned: _,
                ran,
            } => {
                *ran = true;
                let report =
                    crate::remote::stash::run_remote_stash_drop(plan, id.0, *policy, *fault);
                StashCompletion {
                    id: *id,
                    report: StashExecutionReport::Remote(Box::new(report)),
                }
            }
        }
    }
}
impl Drop for StashJob {
    fn drop(&mut self) {
        let Self::Remote {
            id,
            plan,
            policy,
            fault: _,
            abandoned,
            ran,
        } = self
        else {
            return;
        };
        if !*ran {
            let report = crate::remote::stash::run_remote_stash_drop(
                plan,
                id.0,
                *policy,
                crate::remote::stash::RemoteStashFault::LocalSpawn,
            );
            let _ = abandoned.send(Completion::Stash(Box::new(StashCompletion {
                id: *id,
                report: StashExecutionReport::Remote(Box::new(report)),
            })));
        }
    }
}
pub fn prepare_stash(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
) -> Result<StashJob, AdmissionError> {
    if !matches!(
        approved.prepared,
        Planned::Stash { .. } | Planned::RemoteStash { .. }
    ) {
        return Err(AdmissionError::StaleApproval);
    }
    let id = reserve(s, &approved, legacy)?;
    match approved.prepared {
        Planned::Stash { plan, policy, .. } => Ok(StashJob::Local(LocalStashJob {
            id,
            plan,
            policy,
            fault: None,
            abandoned: s.abandoned_tx.clone(),
            ran: false,
        })),
        Planned::RemoteStash { plan, policy, .. } => Ok(StashJob::Remote {
            id,
            plan,
            policy,
            fault: crate::remote::stash::RemoteStashFault::None,
            abandoned: s.abandoned_tx.clone(),
            ran: false,
        }),
        _ => unreachable!(),
    }
}
#[derive(Clone, Debug)]
pub struct StashConflict {
    pub operation: OperationId,
    pub oid: String,
    pub(crate) identity: Vec<String>,
    pub(crate) pending: bool,
}
impl Sessions {
    pub fn observe_stash_conflict(&mut self, owner: &Path, identity: &[String]) {
        let Some(payload) = self.stash_conflicts.get(owner) else {
            return;
        };
        if payload.pending && identity.is_empty() {
            let payload = self
                .stash_conflicts
                .remove(owner)
                .expect("stash conflict was observed above");
            self.stash_followups.insert(owner.to_path_buf(), payload);
        } else if payload.pending
            || identity.is_empty()
            || !identity
                .iter()
                .all(|entry| payload.identity.contains(entry))
        {
            self.clear_stash_conflict(owner);
        }
    }
    pub fn stash_conflict(&self, owner: &Path) -> Option<&StashConflict> {
        self.stash_conflicts.get(owner)
    }
    pub fn clear_stash_conflict(&mut self, owner: &Path) {
        self.stash_conflicts.remove(owner);
        self.stash_followups.remove(owner);
    }
    pub fn continue_stash_conflict(&mut self, owner: &Path) {
        if let Some(payload) = self.stash_conflicts.get_mut(owner) {
            payload.pending = true;
        }
    }
    pub fn take_stash_followup(&mut self, owner: &Path) -> Option<StashConflict> {
        self.stash_followups.remove(owner)
    }
}
