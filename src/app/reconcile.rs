//! Reconciliation: the one exit from a write kagi could not account for.
//!
//! Split from `session.rs` on the lifecycle boundary — that file owns tabs,
//! identity and leases, this one owns what happens after settlement parked a
//! requirement. The order here is the contract: **prove the writer stopped
//! before reading anything**, because a snapshot taken while it is still
//! running is a pre-mutation read, and acknowledging it would report "settled"
//! about a write that was still going (ADR-0175, ADR-0177).
use super::*;

#[derive(Clone)]
pub struct ReconcileRead {
    id: OperationId,
    pub observation: String,
    stop_proven: bool,
    /// Whether the read could account for everything the operation left behind.
    /// An unresolved read may be shown, but never acknowledged: releasing the
    /// scope would report "settled" about work kagi cannot point at.
    resolved: bool,
}
impl ReconcileRead {
    /// The operation this read is about, so a caller that cannot act on it yet
    /// can offer the user another look rather than a dead end.
    pub fn operation(&self) -> OperationId {
        self.id
    }
    /// Whether this read may be acknowledged at all: a live child or a stash
    /// kagi cannot point at is a *later* answer, not a refusal to record now.
    pub fn settled(&self) -> bool {
        self.stop_proven && self.resolved
    }
    pub fn resolved(&self) -> bool {
        self.resolved
    }
    pub fn stop_proven(&self) -> bool {
        self.stop_proven
    }
}
pub struct ReconcileJob {
    id: OperationId,
    plan: Planned,
    remote: Option<crate::remote::stash::RemoteStashEvidence>,
    pull_stash: Option<kagi_git::backend::stash::StashEvidence>,
    child: Option<u32>,
}
impl ReconcileJob {
    pub fn run(self) -> Result<ReconcileRead, String> {
        // Stop proof **before** observation, never after (#702 re-review). A
        // snapshot taken while the writer is still running is a pre-mutation
        // read; labelling it `stop_proven` because the process happened to exit
        // between the read and the probe is exactly how an unfinished write
        // gets acknowledged as settled. While anything in the group is alive
        // there is nothing worth reading — come back later.
        if let Some(group) = self.child {
            if kagi_git::proc::group_alive(group) {
                return Ok(ReconcileRead {
                    id: self.id,
                    observation: format!(
                        "the writer's process group {group} is still running; \
                         nothing was read"
                    ),
                    stop_proven: false,
                    resolved: false,
                });
            }
        }
        // `resolved` is whether the read could account for everything the
        // operation left behind.
        let mut resolved = true;
        let (observation, stop_proven) = match &self.plan {
            Planned::Remove { plan, .. } => (
                kagi_git::Backend::read_remove_status(plan).map_err(|e| e.to_string())?,
                true,
            ),
            Planned::Stash { plan, .. } => (
                kagi_git::Backend::read_stash_status(plan).map_err(|e| e.to_string())?,
                true,
            ),
            Planned::RemoteStash { plan, .. } => {
                let evidence = self
                    .remote
                    .as_ref()
                    .ok_or("remote completion evidence is missing")?;
                (
                    crate::remote::stash::reconcile_remote_stash(plan, evidence, self.id.0)?,
                    true,
                )
            }
            Planned::Conflict { plan, .. } => (
                kagi_git::Backend::open(plan.repo())
                    .and_then(|backend| backend.conflict_snapshot())
                    .map(|snapshot| format!("conflict={snapshot:?}"))
                    .map_err(|e| e.to_string())?,
                true,
            ),
            Planned::Run(request) if writes_only_locally(request.name) => (
                kagi_git::Backend::open(&request.path)
                    .and_then(|mut backend| backend.snapshot(1))
                    .map(|snap| {
                        format!(
                            "head={} dirty={}",
                            snap.head.display(),
                            snap.status.is_dirty()
                        )
                    })
                    .map_err(|e| e.to_string())?,
                true,
            ),
            // Everything else may have touched a remote, and a local snapshot
            // cannot say whether a remote ref moved (ADR-0177).
            Planned::Run(request) => {
                let (observation, confirmed) = observe_remote_run(request)?;
                resolved = confirmed;
                (observation, true)
            }
            Planned::Pull(request) => {
                let (observation, accounted) = observe_pull(request, self.pull_stash.as_ref())?;
                resolved = accounted;
                (observation, true)
            }
        };
        Ok(ReconcileRead {
            id: self.id,
            observation,
            stop_proven,
            resolved,
        })
    }
}
/// Operations that write nothing to a remote, and may therefore be reconciled
/// from a local read.
///
/// An **allowlist**, and that direction is the point: the default is "this may
/// have moved a remote ref", so an operation nobody classified is unresolved
/// until someone names its remote effect — not quietly acknowledgeable from a
/// local snapshot. A denylist missed `branch-push` exactly that way (#702
/// review 4), and the next family added would have been missed the same way.
///
/// A fetch is a read, so the three operations that fetch before writing
/// locally — `branch-pull-ff` (fetch, then a local fast-forward),
/// `checkout-tracking` and `switch-to-latest` — belong here deliberately.
///
/// Branch cleanup is deliberately **not** here: it deletes remote branches
/// (`push --delete`), so when #701 moves it onto this family it starts out
/// unresolved rather than acknowledgeable, which is the safe way round.
fn writes_only_locally(name: &str) -> bool {
    matches!(
        name,
        "checkout"
            | "checkout-commit"
            | "commit"
            | "amend"
            | "cherry-pick"
            | "revert"
            | "merge"
            | "rebase"
            | "reset-current"
            | "create-branch"
            | "delete-branch"
            | "rename-branch"
            | "set-upstream"
            | "create-worktree"
            | "discard"
            | "branch-pull-ff"
            | "checkout-tracking"
            | "switch-to-latest"
    )
}

/// What a remote-writing run left on the remote, read live and compared
/// against the expectation frozen at approval.
///
/// Never against current values. Kagi's lease does not stop an external Git
/// from moving the local branch back to the remote's old tip, and comparing
/// "what this repository has now" would then call an unlanded push confirmed
/// (#702 re-review). An operation whose remote effect could not be named at
/// approval stays unconfirmed — the observation says what the remote holds and
/// the user decides, but the scope is never reopened on a guess (ADR-0177).
fn observe_remote_run(request: &RunRequest) -> Result<(String, bool), String> {
    let Some(expected) = request.remote.as_ref() else {
        return Ok((
            format!(
                "{}: this operation's remote effect was not named at approval, \
                 so nothing can confirm it",
                request.name
            ),
            false,
        ));
    };
    let live =
        kagi_git::Backend::read_remote_ref(&request.path, &expected.remote, &expected.refname)
            .map_err(|e| e.to_string())?;
    let confirmed = expected.matches(live.as_deref());
    Ok((
        format!(
            "{}/{} expected={} live={} confirmed={confirmed}",
            expected.remote,
            expected.refname,
            expected.describe(),
            live.as_deref().unwrap_or("absent"),
        ),
        confirmed,
    ))
}

/// What a pull left behind, read back live: where HEAD and its upstream now
/// stand, whether the auto-stash entry can be accounted for, and whether the
/// working tree is still the one the confirmation named. Never retries and
/// never pops. The `bool` says whether the stash could be accounted for.
fn observe_pull(
    request: &PullRequest,
    stash: Option<&kagi_git::backend::stash::StashEvidence>,
) -> Result<(String, bool), String> {
    let snap = kagi_git::Backend::open(&request.path)
        .and_then(|mut backend| backend.snapshot(1))
        .map_err(|e| e.to_string())?;
    let upstream = match &snap.head {
        kagi_git::Head::Attached { branch, .. } | kagi_git::Head::Unborn { branch } => snap
            .branches
            .iter()
            .find(|candidate| &candidate.name == branch)
            .and_then(|candidate| candidate.upstream.as_ref())
            .map(|up| {
                format!(
                    "{} ahead={} behind={}",
                    up.remote_branch, up.ahead, up.behind
                )
            })
            .unwrap_or_else(|| "none".to_string()),
        kagi_git::Head::Detached { .. } => "detached".to_string(),
    };
    let (stash, resolved) = match stash {
        None => ("none".to_string(), true),
        Some(evidence) => match evidence.oid.as_deref() {
            Some(oid) => match kagi_git::Backend::unique_stash_index(&request.path, oid)
                .map_err(|e| e.to_string())?
            {
                Some(index) => (format!("{oid} at stash@{{{index}}}"), true),
                None => (format!("{oid} no longer in the stash list"), true),
            },
            // #623: the entry could not be identified when it was created, so
            // there is no OID to look up — only what the workflow wrote. Match
            // kagi's own auto-stash message against the live list. While more
            // than one answers to it the read stays unresolved: acknowledging
            // would report "settled" about work kagi cannot point at.
            None => {
                let candidates: Vec<_> = snap
                    .stashes
                    .iter()
                    .filter(|entry| entry.message.contains(AUTO_STASH_MESSAGE))
                    .collect();
                match candidates.as_slice() {
                    [only] => (
                        format!(
                            "unidentified auto-stash resolved to {} at stash@{{{}}}",
                            only.target.short(),
                            only.index
                        ),
                        true,
                    ),
                    [] => (
                        "an auto-stash was created but no entry answers to it".to_string(),
                        true,
                    ),
                    many => (
                        format!(
                            "{} entries answer to the auto-stash; it cannot be told apart",
                            many.len()
                        ),
                        false,
                    ),
                }
            }
        },
    };
    let digest = snap.status.digest();
    let promise = match request.promised_dirty {
        Some(promised) if promised == digest => "as confirmed",
        Some(_) => "moved",
        None => "not promised",
    };
    Ok((
        format!(
            "head={} upstream={upstream} auto_stash={} stash={stash} dirty={} ({promise})",
            snap.head.display(),
            request.auto_stash,
            digest.0
        ),
        resolved,
    ))
}

pub fn prepare_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileJob, String> {
    let entry = sessions.reconcile.get(&id).ok_or("no reconcile request")?;
    // An unproven termination is readable exactly when something can prove it:
    // the remote evidence, or the process group the executor could not account
    // for. `Termination::Abandoned` has neither — kagi lost its own executor —
    // and says so rather than offering a read that cannot mean anything.
    if !entry.stopped && entry.remote.is_none() && entry.child.is_none() {
        return Err("execution termination is unconfirmed and nothing is left to probe".into());
    }
    Ok(ReconcileJob {
        id,
        plan: entry.plan.clone(),
        remote: entry.remote.clone(),
        pull_stash: entry.pull.clone(),
        child: entry.child,
    })
}
pub fn read_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileRead, String> {
    prepare_reconcile(sessions, id)?.run()
}
/// Close one reconcile requirement and release the scope it holds.
///
/// The exit from an unproven termination, for **every** local family — the run
/// pipeline, pull, and whatever migrates next. The lifecycle is one path:
///
/// 1. the executor reports `GitError::TerminationUnknown(kagi_git::Termination)`,
///    which says whether it saw the child stop and, if not, carries its pid;
/// 2. `apply` settles: the lease is released only on a proven stop, and an
///    entry is registered either way, so a retained lease always has something
///    to be acknowledged against;
/// 3. [`prepare_reconcile`] / [`read_reconcile`] observe — including asking the
///    OS whether that pid is still there, which is what turns "unproven" into
///    `stop_proven` later, without ever treating a repository snapshot as
///    proof that a process stopped;
/// 4. this releases the scope, but only for a read that is both `stop_proven`
///    and `resolved`.
///
/// A family adds nothing to this path: it only has to let its
/// `TerminationUnknown` reach the completion with its type intact.
pub fn acknowledge(sessions: &mut Sessions, read: ReconcileRead) -> Result<(), AdmissionError> {
    let Some(entry) = sessions.reconcile.get(&read.id) else {
        return Err(AdmissionError::NeedsReconcile);
    };
    if !entry.stopped && !read.stop_proven {
        return Err(AdmissionError::NeedsReconcile);
    }
    if !read.resolved {
        return Err(AdmissionError::NeedsReconcile);
    }
    let scope = entry.plan.scope();
    sessions.reconcile.remove(&read.id);
    sessions.release_lease(&scope, read.id);
    Ok(())
}
