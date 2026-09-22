//! The explicit exit from a remote write whose result is unobservable (#706).
//!
//! Split from `reconcile.rs` on the lifecycle boundary that file already
//! draws: it owns *observing* a parked requirement, this owns the one release
//! that happens when the observation can never come.
//!
//! A child module rather than a sibling, because the eligibility rule is
//! `Resolution` itself — a private type that must stay inside the reconcile
//! lifecycle rather than becoming another module's business.
use super::*;

/// The explicit exit from a write whose remote result is **unobservable**
/// (#706).
///
/// Separate from [`acknowledge`] on purpose, and never reachable from it. The
/// ordinary acknowledgement means "kagi looked and everything agreed"; this one
/// means "kagi could not look, the user saw that, and the requirement is being
/// released anyway". Conflating them would turn the strictest statement kagi
/// makes about a remote write into the weakest.
///
/// Three things make it safe to offer at all:
///
/// 1. **Eligibility is re-derived from the parked entry**, never from the read
///    a caller happens to be holding: only a known remote-writing family whose
///    writer is proven stopped and whose promise came back unobservable.
/// 2. **The audit record is written first.** The release is an administrative
///    act, and one that left no trace is not one — a failed recording keeps the
///    requirement and the lease exactly where they were.
/// 3. **The record never claims the write succeeded.** It is `Unknown`, it
///    names the original operation and its id, and it says in English that kagi
///    did not verify the remote.
pub struct UnobservableReleaseJob {
    id: OperationId,
    /// The original operation's name, kept so the audit row points at the write
    /// being released rather than at the release itself.
    op: &'static str,
    repo: String,
    before: kagi_git::StateSummary,
    reason: String,
    observation: String,
    stop_proven: bool,
}
impl UnobservableReleaseJob {
    /// The durable half, run off the UI thread: append the audit record.
    ///
    /// Never touches `Sessions` — the requirement is still open when this
    /// returns, and [`acknowledge_unobserved`] decides on the result.
    pub fn run(self) -> UnobservableReleaseReport {
        use kagi_git::backend::recording;
        use kagi_git::oplog::{OpLogEntry, OpOutcome};
        let evidence = format!(
            "{} (kagi operation {}) was released without an observed remote \
             result: {}. Kagi did NOT verify what the remote holds; this record \
             accounts for the release of the reconcile requirement, never for \
             the success of the write. Observation at release: {}",
            self.op, self.id.0, self.reason, self.observation
        );
        let after = kagi_git::StateSummary {
            head: self.before.head.clone(),
            dirty: "unknown".to_string(),
        };
        let entry = OpLogEntry::new(
            "reconcile-release-unobservable",
            self.repo.clone(),
            self.before,
            // Unknown, and nothing weaker: the release changes what kagi is
            // *requiring*, not what the remote holds.
            OpOutcome::Unknown { after, evidence },
        )
        .with_worktree(Some(self.repo));
        UnobservableReleaseReport {
            id: self.id,
            stop_proven: self.stop_proven,
            recording: recording::finalize(entry),
        }
    }
}

/// What [`UnobservableReleaseJob::run`] proved. Deliberately not `Clone`: it is
/// a one-shot capability, and its private fields are what keep a caller from
/// assembling one for an operation that was never eligible.
pub struct UnobservableReleaseReport {
    id: OperationId,
    stop_proven: bool,
    recording: kagi_git::backend::recording::Recording,
}
impl UnobservableReleaseReport {
    /// Present the audit result without allowing callers to replace its proof.
    pub fn recording(&self) -> &kagi_git::backend::recording::Recording {
        &self.recording
    }

    /// The operation this release is about, so a refusal can still offer the
    /// user the entry that is blocking them.
    pub fn operation(&self) -> OperationId {
        self.id
    }
}

/// The one remote-writing run a parked entry is, when it is one kagi can name.
/// Derived from the entry rather than from a [`ReconcileRead`], so eligibility
/// is a property of the requirement and not of a value the UI carries around.
fn eligible_remote_write(entry: &ReconcileEntry) -> Option<&RunRequest> {
    let ReconcileTarget::Planned(plan) = &entry.target else {
        return None;
    };
    match plan.as_ref() {
        Planned::Run(request) if writes_to_a_known_remote(request.name) => Some(request),
        _ => None,
    }
}

/// Validate the armed release and hand back the job that records it.
///
/// `Err` is the same [`AdmissionError::NeedsReconcile`] the ordinary path uses:
/// from the caller's side this is still "that requirement is not releasable",
/// and nothing was written.
pub fn prepare_unobservable_release(
    sessions: &Sessions,
    read: ReconcileRead,
) -> Result<UnobservableReleaseJob, AdmissionError> {
    let Some(entry) = sessions.reconcile.get(&read.id) else {
        return Err(AdmissionError::NeedsReconcile);
    };
    if !entry.stopped && !read.stop_proven {
        return Err(AdmissionError::NeedsReconcile);
    }
    let Some(reason) = read.unobservable_reason() else {
        return Err(AdmissionError::NeedsReconcile);
    };
    let Some(request) = eligible_remote_write(entry) else {
        return Err(AdmissionError::NeedsReconcile);
    };
    Ok(UnobservableReleaseJob {
        id: read.id,
        op: request.name,
        repo: request.path.display().to_string(),
        before: request.plan.current.clone(),
        reason: reason.to_string(),
        observation: read.observation,
        stop_proven: read.stop_proven,
    })
}

/// Close an unobservable requirement against the audit record written for it,
/// and release the scope it holds.
///
/// Validates the same things [`prepare_unobservable_release`] did, because the
/// UI arms and confirms in two steps and the entry is the authority at both.
/// Then the record decides: an append that failed leaves the requirement and
/// the lease untouched and hands back the cause, because a release nobody can
/// audit is exactly the silent one this path exists to avoid.
pub fn acknowledge_unobserved(
    sessions: &mut Sessions,
    report: UnobservableReleaseReport,
) -> Result<(), String> {
    use kagi_git::backend::recording::Recording;
    let Some(entry) = sessions.reconcile.get(&report.id) else {
        return Err(
            "this reconcile requirement is no longer open; nothing was released".to_string(),
        );
    };
    if !entry.stopped && !report.stop_proven {
        return Err("the writer is not proven stopped, so the scope stays reserved".to_string());
    }
    if eligible_remote_write(entry).is_none() {
        return Err(
            "this operation is not one of the remote-writing families kagi can \
             release without an observation"
                .to_string(),
        );
    }
    // Durable first. Removing the requirement on a failed append would release
    // the scope with no record that anyone did.
    if let Recording::Failed { error, .. } = &report.recording {
        return Err(format!(
            "the release could not be recorded in the operation log ({error}); \
             the reconcile requirement is still open and the scope is still held"
        ));
    }
    let scope = entry.scope.clone();
    sessions.reconcile.remove(&report.id);
    sessions.release_lease(&scope, report.id);
    Ok(())
}
