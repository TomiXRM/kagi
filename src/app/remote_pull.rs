//! Remote SSH pull: cached preview, asynchronously resolved lease scope, and one
//! transport-owned durable receipt.
use super::*;
use kagi_domain::plan_note::{PlanNote, PlanTitle, PullNote, PullTitle};
use kagi_domain::remote::{RemotePullConfig, RemotePullFingerprint, RemotePullHead, RemoteRepoId};
use std::sync::{mpsc::Sender, Arc};

#[derive(Clone, Debug)]
pub struct RemotePullRequest {
    pub owner: crate::remote::stash::RemoteAttachment,
    pub plan: Arc<kagi_git::OperationPlan>,
    pub cached_head_oid: Option<String>,
    pub cached_remote_dirty: bool,
}

#[derive(Clone, Debug)]
pub struct RemotePullPlan {
    pub preview: Arc<kagi_git::OperationPlan>,
    pub repo_id: RemoteRepoId,
    pub physical_toplevel: String,
    pub head: RemotePullHead,
    pub config: RemotePullConfig,
    pub fingerprint: RemotePullFingerprint,
}

pub struct RemotePullPlanJob {
    revision: RequestId,
    request: RemotePullRequest,
    fixture: Option<crate::remote::PullRepoIdentity>,
}
impl RemotePullPlanJob {
    pub fn run(self) -> PlanCompletion {
        let result = self.fixture.map(Ok).unwrap_or_else(|| {
            crate::remote::resolve_pull_identity(&self.request.owner.host, &self.request.owner.root)
        });
        let state = match result {
            Ok(identity) => {
                // A cached preview is not authority: its HEAD, dirty state,
                // checkout, and upstream must all describe the live host.
                let matches_preview = matches!(
                    &self.request.plan.title,
                    PlanTitle::Pull(PullTitle::PullRemote { branch, upstream, .. })
                        if identity.head.branch.as_deref() == Some(branch.as_str())
                            && identity.head.upstream.as_deref() == Some(upstream.as_str())
                            && Some(identity.head.oid.as_str())
                                == self.request.cached_head_oid.as_deref()
                            && identity.remote_dirty == self.request.cached_remote_dirty
                            && identity.config.is_some()
                );
                if !matches_preview {
                    let blocker = PlanNote::Pull(PullNote::RemotePreviewStale);
                    PlanState::Error {
                        error: blocker.message_en(),
                        blocker: Some(blocker),
                        open_failed: false,
                        recording: None,
                    }
                } else {
                    PlanState::Ready {
                        token: PlanToken {
                            revision: self.revision,
                        },
                        prepared: Planned::RemotePull {
                            plan: Box::new(RemotePullPlan {
                                preview: self.request.plan.clone(),
                                repo_id: identity.repo_id,
                                physical_toplevel: identity.physical_toplevel,
                                head: identity.head,
                                config: identity.config.expect("matched pull configuration"),
                                fingerprint: identity.fingerprint,
                            }),
                            request: self.request,
                        },
                    }
                }
            }
            Err(error) => PlanState::Error {
                error: error.to_string(),
                blocker: None,
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

pub fn plan_remote_pull(sessions: &mut Sessions, request: RemotePullRequest) -> RemotePullPlanJob {
    sessions.invalidate_plan();
    sessions.plan_owner = Some(request.owner.session);
    sessions.state = PlanState::Planning {
        request: sessions.revision,
    };
    RemotePullPlanJob {
        revision: sessions.revision,
        request,
        fixture: None,
    }
}

#[doc(hidden)]
pub fn plan_remote_pull_for_test(
    sessions: &mut Sessions,
    request: RemotePullRequest,
    identity: crate::remote::PullRepoIdentity,
) -> RemotePullPlanJob {
    let mut job = plan_remote_pull(sessions, request);
    job.fixture = Some(identity);
    job
}

#[derive(Clone, Debug)]
pub struct RemotePullCompletion {
    pub id: OperationId,
    pub report: crate::remote::RemotePullReport,
}

pub struct RemotePullJob {
    id: OperationId,
    request: RemotePullRequest,
    identity: crate::remote::PullRepoIdentity,
    abandoned: Sender<Completion>,
    ran: bool,
    fixture: Option<crate::remote::RemotePullReport>,
}
impl RemotePullJob {
    pub fn id(&self) -> OperationId {
        self.id
    }

    #[doc(hidden)]
    pub fn with_report_for_test(mut self, report: crate::remote::RemotePullReport) -> Self {
        self.fixture = Some(report);
        self
    }

    pub fn run(mut self) -> RemotePullCompletion {
        let report = self.fixture.take().unwrap_or_else(|| {
            let owner = &self.request.owner;
            crate::remote::remote_pull(
                &owner.host,
                &owner.root,
                &self.identity,
                &self.request.plan.current,
            )
        });
        self.ran = true;
        RemotePullCompletion {
            id: self.id,
            report,
        }
    }
}
impl Drop for RemotePullJob {
    fn drop(&mut self) {
        if !self.ran {
            let report = crate::remote::abandoned_remote_pull(
                &self.request.owner.host,
                &self.request.owner.root,
                &self.request.plan.current,
            );
            let _ = self
                .abandoned
                .send(Completion::RemotePull(Box::new(RemotePullCompletion {
                    id: self.id,
                    report,
                })));
        }
    }
}

pub fn prepare_remote_pull(
    s: &mut Sessions,
    approved: Approved,
) -> Result<RemotePullJob, AdmissionError> {
    if !matches!(approved.prepared, Planned::RemotePull { .. }) {
        return Err(AdmissionError::StaleApproval);
    }
    let running = begin_write(s, &approved)?;
    let Planned::RemotePull { request, plan } = approved.prepared else {
        unreachable!()
    };
    let remote_dirty = request.cached_remote_dirty;
    Ok(RemotePullJob {
        id: running.operation_id,
        request,
        identity: crate::remote::PullRepoIdentity {
            repo_id: plan.repo_id,
            physical_toplevel: plan.physical_toplevel,
            head: plan.head,
            config: Some(plan.config),
            fingerprint: plan.fingerprint,
            remote_dirty,
        },
        abandoned: s.abandoned_tx.clone(),
        ran: false,
        fixture: None,
    })
}
