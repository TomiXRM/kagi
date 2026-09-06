use super::session::*;
use kagi_git::backend::remove::{Recording, RemovePlan, RemoveReport};
use kagi_git::{Actor, Backend, OpOutcome};
use std::path::PathBuf;

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
#[derive(Clone, Debug)]
pub struct PlanToken {
    revision: RequestId,
}
#[derive(Clone, Debug)]
pub enum PlanState {
    Draft,
    Planning {
        request: RequestId,
    },
    Ready {
        token: PlanToken,
        plan: RemovePlan,
        request: RemoveRequest,
        policy: RemovePolicy,
    },
    Error {
        error: String,
        recording: Recording,
    },
    Approved,
}
pub struct PlanJob {
    revision: RequestId,
    request: RemoveRequest,
    policy: RemovePolicy,
}
pub struct PlanCompletion {
    revision: RequestId,
    state: PlanState,
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
                plan,
                request: self.request,
                policy: self.policy,
            },
            Err(e) => PlanState::Error {
                error: e.to_string(),
                recording: kagi_git::backend::remove::record_plan_error(
                    &self.request.owner.path,
                    self.policy.actor,
                    &e.to_string(),
                ),
            },
        };
        PlanCompletion {
            revision: self.revision,
            state,
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
pub fn apply_plan(sessions: &mut Sessions, completion: PlanCompletion) -> bool {
    if sessions.revision != completion.revision {
        return false;
    }
    sessions.state = completion.state;
    true
}
pub struct Approved {
    revision: RequestId,
    plan: RemovePlan,
    request: RemoveRequest,
    policy: RemovePolicy,
}
pub fn approve(
    sessions: &mut Sessions,
    token: PlanToken,
    policy: RemovePolicy,
) -> Result<Approved, AdmissionError> {
    let PlanState::Ready {
        token: current,
        plan,
        request,
        policy: planned,
    } = &sessions.state
    else {
        return Err(AdmissionError::StaleApproval);
    };
    if current.revision != token.revision || planned != &policy {
        return Err(AdmissionError::StaleApproval);
    }
    let approved = Approved {
        revision: token.revision,
        plan: plan.clone(),
        request: request.clone(),
        policy,
    };
    sessions.state = PlanState::Approved;
    Ok(approved)
}
pub struct RemoveJob {
    id: OperationId,
    plan: RemovePlan,
    policy: RemovePolicy,
    fault: Option<kagi_domain::remove::RemoveFaultPoint>,
    abandoned: std::sync::mpsc::Sender<RemoveCompletion>,
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
            let _ = self.abandoned.send(RemoveCompletion {
                id: self.id,
                report,
            });
        }
    }
}
#[derive(Clone, Debug)]
pub struct RemoveCompletion {
    id: OperationId,
    report: RemoveReport,
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
    if approved.revision != sessions.revision || !matches!(sessions.state, PlanState::Approved) {
        return Err(AdmissionError::StaleApproval);
    }
    if legacy.0 || sessions.has_leases() {
        return Err(AdmissionError::Busy);
    }
    if sessions
        .reconcile
        .values()
        .any(|(plan, _)| plan.common_dir == approved.plan.common_dir)
    {
        return Err(AdmissionError::NeedsReconcile);
    }
    let id = OperationId(next_id());
    sessions.leases.insert(approved.plan.common_dir.clone(), id);
    sessions.operations.insert(
        id,
        InFlight {
            plan: approved.plan.clone(),
            attachment: approved.request.owner,
        },
    );
    sessions.invalidate_plan();
    Ok(RemoveJob {
        id,
        plan: approved.plan,
        policy: approved.policy,
        fault: None,
        abandoned: sessions.abandoned_tx.clone(),
        ran: false,
    })
}
pub fn apply(sessions: &mut Sessions, completion: RemoveCompletion) -> Vec<Delivery> {
    if sessions.settled.contains(&completion.id) {
        return vec![];
    }
    let Some(owner) = sessions.operations.remove(&completion.id) else {
        return vec![];
    };
    sessions.settled.insert(completion.id);
    let stopped = !completion.report.progress.termination_unknown;
    if stopped && sessions.leases.get(&owner.plan.common_dir) == Some(&completion.id) {
        sessions.leases.remove(&owner.plan.common_dir);
    }
    if matches!(
        completion.report.recording.entry().outcome,
        OpOutcome::Unknown { .. }
    ) {
        sessions
            .reconcile
            .insert(completion.id, (owner.plan.clone(), stopped));
    }
    let mut deliveries = vec![];
    for path in [&owner.plan.repo, &owner.plan.target] {
        sessions.stale.insert(PathBuf::from(path));
        if path == &owner.plan.target && completion.report.target_exists == Some(false) {
            deliveries.push(Delivery::RemovedTarget(path.clone()));
        } else {
            deliveries.push(Delivery::Invalidate(path.clone()));
        }
    }
    deliveries.push(Delivery::Completed {
        id: completion.id,
        attachment: owner.attachment,
        report: Box::new(completion.report),
    });
    deliveries
}
