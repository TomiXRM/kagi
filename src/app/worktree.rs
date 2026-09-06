use super::*;
use kagi_git::backend::remove::{RemovePlan, RemoveReport};
use kagi_git::{Actor, Backend};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovePolicy {
    pub actor: Actor,
}
impl Default for RemovePolicy {
    fn default() -> Self {
        Self {
            actor: Actor::Human,
        }
    }
}
#[derive(Clone, Debug)]
pub struct RemoveRequest {
    pub owner: Attachment,
    pub name: String,
    pub delete_branch: bool,
}
pub struct PlanJob {
    revision: RequestId,
    request: RemoveRequest,
    policy: RemovePolicy,
}
impl PlanJob {
    pub fn run(self) -> PlanCompletion {
        let state = match Backend::plan_recorded_remove(
            &self.request.owner.path,
            &self.request.name,
            self.request.delete_branch,
        ) {
            Ok(plan) => PlanState::Ready {
                token: PlanToken {
                    revision: self.revision,
                },
                prepared: Planned::Remove {
                    plan,
                    request: self.request,
                    policy: self.policy,
                },
            },
            Err(e) => PlanState::Error {
                open_failed: false,
                error: e.to_string(),
                recording: Some(kagi_git::backend::remove::record_plan_error(
                    &self.request.owner.path,
                    self.policy.actor,
                    &e.to_string(),
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
pub fn plan_remove(
    sessions: &mut Sessions,
    request: RemoveRequest,
    policy: RemovePolicy,
) -> PlanJob {
    sessions.invalidate_plan();
    sessions.state = PlanState::Planning {
        request: sessions.revision,
    };
    PlanJob {
        revision: sessions.revision,
        request,
        policy,
    }
}
pub struct RemoveJob {
    id: OperationId,
    plan: RemovePlan,
    policy: RemovePolicy,
    fault: Option<kagi_domain::remove::RemoveFaultPoint>,
    abandoned: std::sync::mpsc::Sender<Completion>,
    ran: bool,
}
impl RemoveJob {
    pub fn id(&self) -> OperationId {
        self.id
    }
    #[doc(hidden)]
    pub fn with_fault_for_test(mut self, fault: kagi_domain::remove::RemoveFaultPoint) -> Self {
        self.fault = Some(fault);
        self
    }
    pub fn run(self) -> RemoveCompletion {
        self.run_with_events(|_| {})
    }
    pub fn run_with_events(
        mut self,
        event: impl FnMut(kagi_git::backend::remove::RemoveEvent),
    ) -> RemoveCompletion {
        let report = Backend::run_recorded_remove_with_events(
            &self.plan,
            self.policy.actor,
            self.fault,
            event,
        );
        self.ran = true;
        RemoveCompletion {
            id: self.id,
            report,
        }
    }
}
impl Drop for RemoveJob {
    fn drop(&mut self) {
        if !self.ran {
            let report = Backend::abandoned_remove(&self.plan, self.policy.actor);
            let _ = self.abandoned.send(
                RemoveCompletion {
                    id: self.id,
                    report,
                }
                .into(),
            );
        }
    }
}
#[derive(Clone, Debug)]
pub struct RemoveCompletion {
    pub(crate) id: OperationId,
    pub(crate) report: RemoveReport,
}
impl RemoveCompletion {
    pub fn report(&self) -> &RemoveReport {
        &self.report
    }
}
pub fn prepare_remove(
    sessions: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
) -> Result<RemoveJob, AdmissionError> {
    if !matches!(approved.prepared, Planned::Remove { .. }) {
        return Err(AdmissionError::StaleApproval);
    }
    let id = reserve(sessions, &approved, legacy)?;
    let Planned::Remove { plan, policy, .. } = approved.prepared else {
        unreachable!()
    };
    Ok(RemoveJob {
        id,
        plan,
        policy,
        fault: None,
        abandoned: sessions.abandoned_tx.clone(),
        ran: false,
    })
}
