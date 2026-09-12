//! The pull-workflow family (ADR-0196 Wave 3).
//!
//! Pull is the one write that is not one write: a dirty pull runs
//! StashPush → Pull → StashPop, and each child already writes its own durable
//! receipt. So the family carries every receipt it produced
//! ([`PullReport::steps`], in execution order) plus the one that *decided* the
//! workflow ([`PullTerminal::decisive`]) — which is not always the last: a pull
//! that failed and whose stash was then restored successfully is a failed pull,
//! not a successful pop. [`apply`](super::apply) settles on the decisive
//! receipt and the UI presents it.
//!
//! Nothing here synthesizes an oplog entry. A workflow that refuses before any
//! child could run still returns a recorded no-execute step, produced by the
//! blocking core.
use super::*;
use kagi_git::backend::recording::{self, RunReport};
use kagi_git::backend::stash::StashEvidence;
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::{OperationPlan, StateSummary};
use std::path::PathBuf;
use std::sync::Arc;

/// The message every auto-stash the pull workflow creates carries.
///
/// It is the only handle on an entry whose OID could not be established
/// (#623), so the writer and the reconcile read share one definition of it.
pub const AUTO_STASH_MESSAGE: &str = "kagi: auto-stash before pull";

#[derive(Clone, Debug)]
pub struct PullRequest {
    /// The tab that approved the write. Frozen: completion is routed to it.
    pub owner: Attachment,
    /// The oplog `op` name; also the busy label. Always `"pull"`.
    pub name: &'static str,
    pub path: PathBuf,
    /// Frozen at approval — the lease scope.
    pub repo: RepoId,
    pub plan: Arc<OperationPlan>,
    /// The confirmed auto-stash: the workflow may run StashPush/StashPop.
    pub auto_stash: bool,
    /// The dirty set the confirmation named (#625 / ADR-0192). The blocking
    /// core refuses before stashing if it moved; a reconcile compares the
    /// working tree it finds against it.
    pub promised_dirty: Option<kagi_domain::status::WorktreeDigest>,
}

/// What the UI does with a finished workflow. `Partial` means the pull or the
/// stash restoration changed repository state but the confirmed workflow did
/// not finish.
#[derive(Clone, Debug)]
pub enum PullPresentation {
    Success { summary: String },
    Failed { error: String },
    Partial { error: String },
}

/// The step the workflow settles on, plus what the UI shows for it.
#[derive(Clone, Debug)]
pub struct PullTerminal {
    /// Where the decisive receipt sits in [`PullReport::steps`]. Private: the
    /// constructors are the only way to set it, so [`PullReport::decisive`]
    /// cannot be handed an index that is not there.
    decisive: usize,
    pub presentation: PullPresentation,
    /// An auto-stash this workflow created and did **not** restore: the
    /// recovery context a reconcile needs. A pull whose termination is
    /// unconfirmed never has its stash popped on the user's behalf.
    ///
    /// The backend's own evidence, not just an OID: when the entry could not be
    /// identified (#623) there *is* no OID, and the reconcile read has to fall
    /// back to matching [`AUTO_STASH_MESSAGE`] against the live stash list.
    pub stash: Option<StashEvidence>,
}

/// Every receipt the pull workflow produced, in execution order.
#[derive(Clone, Debug)]
pub struct PullReport {
    pub steps: Vec<RunReport>,
    pub terminal: PullTerminal,
}
impl PullReport {
    /// Settle on the step at `decisive`.
    ///
    /// # Panics
    /// `decisive` must index `steps`; every construction site is in-crate.
    pub fn new(
        steps: Vec<RunReport>,
        decisive: usize,
        presentation: PullPresentation,
        stash: Option<StashEvidence>,
    ) -> Self {
        assert!(
            decisive < steps.len(),
            "the decisive receipt must be one of the steps that ran"
        );
        Self {
            steps,
            terminal: PullTerminal {
                decisive,
                presentation,
                stash,
            },
        }
    }
    /// The receipt the workflow settles on — not always the last one run: a
    /// pull that failed and whose stash was then restored is a failed pull.
    pub fn decisive(&self) -> &RunReport {
        &self.steps[self.terminal.decisive]
    }
    /// Its position, for a presenter that walks the steps in execution order.
    pub fn decisive_index(&self) -> usize {
        self.terminal.decisive
    }
    /// Did any step's receipt fail to reach the oplog? A mutation that happened
    /// but was not recorded must never be presented as a clean success (#501),
    /// and that is true of a *child*'s receipt too, not only the decisive one.
    pub fn recording_failed(&self) -> bool {
        self.steps
            .iter()
            .any(|step| matches!(step.recording, recording::Recording::Failed { .. }))
    }
    /// Which receipt speaks for the workflow.
    ///
    /// Normally the decisive one. But a step whose receipt never reached the
    /// oplog outranks it: "this happened and kagi could not record it" is the
    /// fact the user needs, and `entry_for_recording` already renders exactly
    /// that ("changed but not recorded"). Announcing the clean decisive receipt
    /// beside it would show a success for a workflow that was not fully
    /// recorded (#501, #702 re-review).
    ///
    /// Pure, and the single place the choice is made: the presenter walks the
    /// steps and announces this index — one toast, one footer, one auto-open.
    pub fn announce(&self) -> usize {
        self.steps
            .iter()
            .position(|step| matches!(step.recording, recording::Recording::Failed { .. }))
            .unwrap_or(self.terminal.decisive)
    }
    /// Settle on the last step run — the common case.
    ///
    /// # Panics
    /// `steps` must not be empty: every exit from the workflow records at
    /// least one receipt, including the ones that start no child.
    pub fn settled(
        steps: Vec<RunReport>,
        presentation: PullPresentation,
        stash: Option<StashEvidence>,
    ) -> Self {
        let decisive = steps
            .len()
            .checked_sub(1)
            .expect("a pull workflow records at least one step");
        Self::new(steps, decisive, presentation, stash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::oplog::OpLogEntry;
    use kagi_git::{OperationOutcome, StateSummary};

    fn step(recording: recording::Recording) -> RunReport {
        RunReport {
            result: Ok(OperationOutcome::Unit),
            recording,
            stash: None,
        }
    }
    fn entry() -> OpLogEntry {
        OpLogEntry::new(
            "stash-push",
            "/repo",
            StateSummary {
                head: "branch: main".into(),
                dirty: "dirty".into(),
            },
            OpOutcome::Success {
                after: StateSummary {
                    head: "branch: main".into(),
                    dirty: "clean".into(),
                },
            },
        )
    }

    /// #501, across the whole workflow: a mutation that happened but was not
    /// recorded must never be presented as a clean success — and a pull's
    /// children are recorded separately, so a *child*'s append can be the one
    /// that never landed while the decisive receipt's did.
    #[test]
    fn a_child_receipt_that_never_landed_is_not_a_clean_success() {
        let lost = recording::Recording::Failed {
            attempted: entry(),
            error: "disk full".into(),
        };
        let kept = recording::Recording::Appended {
            path: "/log".into(),
            entry: entry(),
        };
        let report = PullReport::settled(
            vec![step(lost), step(kept.clone())],
            PullPresentation::Success {
                summary: "fast-forward".into(),
            },
            None,
        );
        assert!(
            report.recording_failed(),
            "the decisive receipt landed, but the stash-push's did not"
        );
        assert_eq!(
            report.announce(),
            0,
            "so the workflow speaks through the receipt that never landed — a \
             clean decisive receipt must not announce a success for it"
        );
        let all_kept = PullReport::settled(
            vec![step(kept.clone()), step(kept.clone())],
            PullPresentation::Success {
                summary: "fast-forward".into(),
            },
            None,
        );
        assert!(!all_kept.recording_failed());
        // And the decisive receipt's *own* append failing is the same fact: a
        // clean `pull: …` success must never follow "changed but not recorded".
        let decisive_lost = PullReport::settled(
            vec![
                step(kept.clone()),
                step(recording::Recording::Failed {
                    attempted: entry(),
                    error: "disk full".into(),
                }),
            ],
            PullPresentation::Success {
                summary: "fast-forward".into(),
            },
            None,
        );
        assert!(
            decisive_lost.recording_failed(),
            "the receipt that speaks for the workflow is the one that was lost"
        );
        assert_eq!(decisive_lost.announce(), decisive_lost.decisive_index());
        assert_eq!(
            all_kept.announce(),
            all_kept.decisive_index(),
            "with everything recorded, the decisive receipt speaks"
        );
    }
}

/// Admission for the pull workflow. Same rung as [`approve_run`]: the plan
/// lives in the confirmation modal, so there is no plan slot to spend.
pub fn approve_pull(s: &mut Sessions, request: PullRequest) -> Result<Approved, AdmissionError> {
    let owner = request.owner.clone();
    approve_modal_plan(s, owner, Planned::Pull(request))
}

/// The blocking core, already bound to its arguments. `Err` means the
/// repository would not open: nothing ran, and the job records that itself.
pub type PullExecute = Box<dyn FnOnce() -> Result<PullReport, String> + Send + 'static>;

pub struct PullJob {
    id: OperationId,
    stamp: OwnerStamp,
    request: PullRequest,
    execute: PullExecute,
}
impl PullJob {
    pub fn id(&self) -> OperationId {
        self.id
    }
    /// The owner frozen at admission; the completion is routed by it.
    pub fn stamp(&self) -> OwnerStamp {
        self.stamp
    }
    /// The completion to settle with if this job's task never returns one.
    pub fn abandonment(&self) -> PullAbandonment {
        PullAbandonment {
            id: self.id,
            name: self.request.name,
            path: self.request.path.clone(),
            before: self.request.plan.current.clone(),
        }
    }
    pub fn run(self) -> PullCompletion {
        let report = (self.execute)().unwrap_or_else(|error| {
            let entry = OpLogEntry::new(
                self.request.name,
                self.request.path.display().to_string(),
                self.request.plan.current.clone(),
                OpOutcome::Failed {
                    error: error.clone(),
                },
            );
            PullReport::settled(
                vec![RunReport {
                    result: Err(kagi_git::GitError::Other(error.clone())),
                    recording: recording::finalize(entry),
                    stash: None,
                }],
                PullPresentation::Failed { error },
                None,
            )
        });
        PullCompletion {
            id: self.id,
            report,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PullCompletion {
    pub id: OperationId,
    pub report: PullReport,
}

/// What to settle an admitted pull with when its task unwinds before it can
/// complete (#289).
///
/// A panic is not evidence of termination: the write may have happened and
/// nothing can say otherwise. Recording it as `Unknown` keeps the retained
/// lease paired with a reconcile entry, instead of leaving an operation that
/// can never be settled and a busy mirror that disagrees with the lease.
/// Built before the job is dispatched; it appends nothing until it is used.
#[derive(Clone, Debug)]
pub struct PullAbandonment {
    id: OperationId,
    name: &'static str,
    path: PathBuf,
    before: StateSummary,
}
impl PullAbandonment {
    pub fn into_completion(self) -> PullCompletion {
        let evidence =
            "the pull task unwound; whether the write happened cannot be established".to_string();
        let entry = OpLogEntry::new(
            self.name,
            self.path.display().to_string(),
            self.before.clone(),
            OpOutcome::Unknown {
                after: self.before,
                evidence: evidence.clone(),
            },
        );
        PullCompletion {
            id: self.id,
            report: PullReport::settled(
                vec![RunReport {
                    result: Err(kagi_git::GitError::TerminationUnknown(
                        kagi_git::Termination::abandoned(evidence.clone()),
                    )),
                    recording: recording::finalize(entry),
                    stash: None,
                }],
                PullPresentation::Partial { error: evidence },
                None,
            ),
        }
    }
}

pub fn prepare_pull(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
    execute: PullExecute,
) -> Result<PullJob, AdmissionError> {
    let Planned::Pull(request) = &approved.prepared else {
        return Err(AdmissionError::StaleApproval);
    };
    let request = request.clone();
    let running = begin_write(s, &approved, legacy)?;
    Ok(PullJob {
        id: running.operation_id,
        stamp: running.owner_stamp,
        request,
        execute,
    })
}
