//! Reconciliation: the one exit from a write kagi could not account for.
//!
//! Split from `session.rs` on the lifecycle boundary — that file owns tabs,
//! identity and leases, this one owns what happens after settlement parked a
//! requirement. The order here is the contract: **prove the writer stopped
//! before reading anything**, because a snapshot taken while it is still
//! running is a pre-mutation read, and acknowledging it would report "settled"
//! about a write that was still going (ADR-0175, ADR-0177).
use super::*;

/// The explicit release (#706). A child module, not a sibling: its eligibility
/// rule *is* [`Resolution`], which stays private to this lifecycle.
mod unobserved;
pub use unobserved::*;

/// What the read could account for, as one coherent answer.
///
/// Replaces the bare `resolved: bool`, which had to carry two different
/// meanings at once: "nothing could be read" and "something was read and it
/// disagreed" both arrived as `false`, and no caller could tell them apart.
/// #706 turns exactly on that distinction — one of them is a dead end the user
/// may deliberately step out of, the other must never release the scope.
///
/// Private, and deliberately so: the only two states a caller may act on are
/// `resolved()` (acknowledge normally) and `can_acknowledge_unobserved()`
/// (release explicitly, against a durable audit record).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Resolution {
    /// Everything the operation left behind was read and accounted for.
    Accounted,
    /// A known remote-writing operation whose remote effect **could not be
    /// observed at all**: no promise was named at approval, or the remote it
    /// names cannot currently be addressed from this machine. This read proves
    /// neither success nor failure; repairing the configuration may make a later
    /// read possible.
    Unobservable { reason: String },
    /// Something was read and it disagreed with what was promised, or the
    /// question was asked and the answer could not be obtained. Never
    /// releasable: the remote may still be settling, and another look may say
    /// more than this one did.
    Unaccounted,
}

#[derive(Clone)]
pub struct ReconcileRead {
    id: OperationId,
    pub observation: String,
    stop_proven: bool,
    /// Only Accounted permits ordinary acknowledgement. Unobservable permits
    /// the separate audited release, without claiming the write was settled.
    resolution: Resolution,
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
        self.stop_proven && self.resolved()
    }
    /// **Strict**, and unchanged by #706: the operation's effects were observed
    /// and they were the promised ones. Explicit release never turns an
    /// unobservable read into a resolved one.
    pub fn resolved(&self) -> bool {
        matches!(self.resolution, Resolution::Accounted)
    }
    pub fn stop_proven(&self) -> bool {
        self.stop_proven
    }
    /// Whether this read may take the **explicit** exit (#706): the writer is
    /// proven stopped, and what it promised on a remote is unobservable rather
    /// than merely unconfirmed.
    ///
    /// The one capability the UI inspects to decide whether to offer the armed
    /// two-step release. It is not an acknowledgement — the ordinary
    /// [`acknowledge`] still refuses this read — and it is never "kagi believes
    /// the write landed": what the release records is that kagi did **not**
    /// verify the remote.
    pub fn can_acknowledge_unobserved(&self) -> bool {
        self.unobservable_reason().is_some()
    }
    /// The English sentence the audit record carries, and the single rule
    /// [`can_acknowledge_unobserved`](Self::can_acknowledge_unobserved) and
    /// [`prepare_unobservable_release`] share, so the capability the UI shows
    /// and the capability the release checks cannot drift apart.
    fn unobservable_reason(&self) -> Option<&str> {
        match &self.resolution {
            Resolution::Unobservable { reason } if self.stop_proven => Some(reason),
            _ => None,
        }
    }
}
pub struct ReconcileJob {
    id: OperationId,
    target: ReconcileTarget,
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
            // Through the supervisor, so a job that still owns a second group
            // cannot be released past the one this requirement names (#703).
            if !kagi_git::proc::supervisor::group_stopped(group) {
                return Ok(ReconcileRead {
                    id: self.id,
                    observation: format!(
                        "the writer's process group {group} is still running; \
                         nothing was read"
                    ),
                    stop_proven: false,
                    resolution: Resolution::Unaccounted,
                });
            }
        }
        // What the read could account for of everything the operation left
        // behind. Starts at the optimistic end and is narrowed by what the
        // observation finds, never widened.
        let mut resolution = Resolution::Accounted;
        let plan = match self.target.clone() {
            ReconcileTarget::Planned(plan) => *plan,
            // A guarded write whose only mutation is local and idempotent — a
            // fetch, an editor save. The probe above already proved the group
            // is gone; there is nothing left of it to account for.
            ReconcileTarget::Guarded {
                kind: GuardKind::GroupOnly,
                ..
            } => {
                return Ok(ReconcileRead {
                    id: self.id,
                    observation: "the writer's process group is gone".to_string(),
                    stop_proven: true,
                    resolution: Resolution::Accounted,
                })
            }
            // A sequencer step. A stopped process says nothing about how far
            // the sequencer got, so the read is the live state itself — and an
            // unreadable repository is no read at all, which leaves the entry
            // exactly where it was (#702 review 7).
            ReconcileTarget::Guarded {
                kind: GuardKind::Sequencer,
                path,
            } => {
                let snapshot = kagi_git::Backend::open(&path)
                    .and_then(|backend| backend.conflict_snapshot())
                    .map_err(|e| e.to_string())?;
                return Ok(ReconcileRead {
                    id: self.id,
                    observation: format!("sequencer={snapshot:?}"),
                    stop_proven: true,
                    resolution: Resolution::Accounted,
                });
            }
        };
        let (observation, stop_proven) = match &plan {
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
                let (observation, reading) = observe_remote_run(request)?;
                resolution = reading;
                (observation, true)
            }
            Planned::Pull(request) => {
                let (observation, accounted) = observe_pull(request, self.pull_stash.as_ref())?;
                // A pull writes nothing to a remote: an unaccounted auto-stash
                // is a local entry the user can still act on, never a dead end.
                if !accounted {
                    resolution = Resolution::Unaccounted;
                }
                (observation, true)
            }
        };
        Ok(ReconcileRead {
            id: self.id,
            observation,
            stop_proven,
            resolution,
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

/// The operations whose remote effect kagi knows how to name — exactly the
/// families [`kagi_git::Backend::remote_expectation`] produces an expectation
/// for today.
///
/// An **allowlist**, for the same reason [`writes_only_locally`] is one and
/// pointing the same way: a family nobody has classified is *not* eligible for
/// the explicit release. A future remote-writing operation that forgets to
/// freeze an expectation therefore stays blocked instead of inheriting a
/// release path nobody reasoned about for it (#706).
///
/// Used only to decide eligibility for the explicit release. It never widens
/// what `resolved()` means: every operation here is still confirmed only by an
/// observation that agrees with its frozen promise.
fn writes_to_a_known_remote(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "branch-push"
            | "branch-push-set-upstream"
            | "force-with-lease-push"
            | "delete-remote-branch"
            | "push-tag"
            | "pr-merge"
            | "branch-cleanup"
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
fn observe_remote_run(request: &RunRequest) -> Result<(String, Resolution), String> {
    if request.remote.is_empty() {
        let observation = format!(
            "{}: this operation's remote effect was not named at approval, \
             so nothing can confirm it",
            request.name
        );
        // A known remote-writing family with no promise frozen has nothing
        // left to look at, today or ever: that is the dead end #706 opens an
        // explicit exit from. A family nobody classified stays blocked —
        // "unknown" is not "unobservable".
        let resolution = if writes_to_a_known_remote(request.name) {
            Resolution::Unobservable {
                reason: format!(
                    "the {} operation froze no remote expectation at approval, \
                     so there is nothing to compare the remote against",
                    request.name
                ),
            }
        } else {
            Resolution::Unaccounted
        };
        return Ok((observation, resolution));
    }
    // Every promise, and all of them: a batch delete that removed three of four
    // branches is not a confirmed batch delete. One broken promise **dominates**
    // an unobservable sibling — a batch that was half observed and half wrong is
    // wrong, not unobservable (#706).
    let mut broken = false;
    let mut unobservable = Vec::new();
    let mut seen = Vec::new();
    for expectation in &request.remote {
        let (observation, reading) = observe_expectation(&request.path, expectation)?;
        match reading {
            PromiseReading::Kept => {}
            PromiseReading::Unkept => broken = true,
            PromiseReading::Unobservable(reason) => unobservable.push(reason),
        }
        seen.push(observation);
    }
    let resolution = match (broken, unobservable.is_empty()) {
        (true, _) => Resolution::Unaccounted,
        (false, false) if writes_to_a_known_remote(request.name) => Resolution::Unobservable {
            reason: unobservable.join("; "),
        },
        (false, false) => Resolution::Unaccounted,
        (false, true) => Resolution::Accounted,
    };
    Ok((seen.join("; "), resolution))
}

/// What one frozen promise's live read amounts to.
///
/// `Unkept` covers both halves of "the remote answered and it was not this":
/// an observed mismatch, and a question that was asked and came back
/// unanswerable. Both block, because in both the remote *was* addressable and
/// something other than the promise is what kagi has to go on (ADR-0177).
enum PromiseReading {
    Kept,
    Unkept,
    Unobservable(String),
}

/// One promise, checked the way its kind is checked. A ref is `ls-remote`;
/// #701's pull-request kind is a server re-read through the transport.
fn observe_expectation(
    path: &std::path::Path,
    expectation: &kagi_git::backend::remote_ref::RemoteExpectation,
) -> Result<(String, PromiseReading), String> {
    use kagi_git::backend::remote_ref::{PrExpect, RemoteExpectation, RemoteRefObservation};
    match expectation {
        RemoteExpectation::Ref {
            remote,
            refname,
            expect,
        } => match kagi_git::Backend::read_remote_ref(path, remote, refname)
            .map_err(|e| e.to_string())?
        {
            RemoteRefObservation::Observed(live) => {
                let matched = expect.matches(live.as_deref());
                Ok((
                    format!(
                        "{remote}/{refname} expected={} live={} confirmed={matched}",
                        expect.describe(),
                        live.as_deref().unwrap_or("absent"),
                    ),
                    if matched {
                        PromiseReading::Kept
                    } else {
                        PromiseReading::Unkept
                    },
                ))
            }
            // This read could not address the remote. It did not observe a
            // missing ref or a refused transport (#706).
            RemoteRefObservation::Unobservable { reason } => Ok((
                format!(
                    "{remote}/{refname} expected={} live=unobservable ({reason}) \
                     confirmed=false",
                    expect.describe(),
                ),
                PromiseReading::Unobservable(format!(
                    "{remote}/{refname} could not be observed: {reason}"
                )),
            )),
        },
        // The same question `merge_pr` asked to decide the receipt, asked
        // again. `None` is "could not ask" — it confirms nothing, and it must
        // never read as "not merged" (ADR-0177). It is a *communication*
        // failure against an address that resolves, so it blocks rather than
        // qualifying for the explicit release.
        RemoteExpectation::PullRequest {
            base_repo,
            number,
            expect,
        } => {
            let PrExpect::Merged = expect;
            let merged = kagi_git::github::pr_merged_on_server(path, base_repo, *number);
            let live = match merged {
                Some(true) => "merged",
                Some(false) => "open",
                None => "unreadable",
            };
            Ok((
                format!(
                    "{base_repo} pr #{number} expected=merged live={live} confirmed={}",
                    merged == Some(true)
                ),
                if merged == Some(true) {
                    PromiseReading::Kept
                } else {
                    PromiseReading::Unkept
                },
            ))
        }
        // The frozen repository is the address: no remote name, so nothing
        // between approval and this read can redirect it (#701 review 4).
        RemoteExpectation::GithubRef {
            base_repo,
            branch,
            expect,
        } => {
            let live = kagi_git::Backend::read_github_ref(path, base_repo, branch)
                .map_err(|e| e.to_string())?;
            let matched = expect.matches(live.as_deref());
            Ok((
                format!(
                    "{base_repo} heads/{branch} expected={} live={} confirmed={matched}",
                    expect.describe(),
                    live.as_deref().unwrap_or("absent"),
                ),
                if matched {
                    PromiseReading::Kept
                } else {
                    PromiseReading::Unkept
                },
            ))
        }
    }
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
    // Entries in the live list that answer to kagi's own auto-stash message:
    // the only handle on an entry whose identity was never captured.
    let answering_entries = || -> Vec<_> {
        snap.stashes
            .iter()
            .filter(|entry| entry.message.contains(AUTO_STASH_MESSAGE))
            .collect()
    };
    let (stash, resolved) = match stash {
        // No stash evidence at all. For a pull that could not have stashed
        // anything that is simply "none" — but a pull with auto-stash enabled
        // whose report carries no evidence is the abandoned one (#725 review):
        // the task unwound, and *nothing in the report* says whether the stash
        // was taken. An absent receipt is not an absent stash, so the live list
        // is what answers here. An entry answering to kagi's auto-stash leaves
        // the read unresolved: acknowledging would call the operation settled
        // while the user's changes sit in a stash nothing accounted for. The
        // user's way out is the entry itself — pop or drop it and read again.
        None if request.auto_stash => match answering_entries().as_slice() {
            [] => (
                "no entry answers to kagi's auto-stash".to_string(),
                true, // observed absence, not an assumed one
            ),
            many => (
                format!(
                    "{} entry/entries answer to kagi's auto-stash and no receipt \
                     accounts for them",
                    many.len()
                ),
                false,
            ),
        },
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
                let candidates = answering_entries();
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
    // for. Since #703 a panicked job has one of those — the supervisor owns
    // what it spawned — so this refusal is left for a termination that names
    // neither, which is not something the run families can produce.
    if !entry.stopped && entry.remote.is_none() && entry.child.is_none() {
        return Err("execution termination is unconfirmed and nothing is left to probe".into());
    }
    Ok(ReconcileJob {
        id,
        target: entry.target.clone(),
        remote: entry.remote.clone(),
        pull_stash: entry.pull.clone(),
        child: entry.child,
    })
}
pub fn read_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileRead, String> {
    prepare_reconcile(sessions, id)?.run()
}
/// GUI-E2E seam: park one already-resolved local reconcile read so modal
/// replacement can prove an Acknowledge action survives and executes.
#[cfg(feature = "gui-e2e")]
#[doc(hidden)]
pub fn seed_acknowledge_for_test(
    sessions: &mut Sessions,
    session: SessionId,
) -> Option<ReconcileRead> {
    let worktree = sessions.worktree_of(session)?.clone();
    let path = sessions.path_of(&worktree)?;
    let id = OperationId(next_id());
    sessions.reconcile.insert(
        id,
        ReconcileEntry {
            target: ReconcileTarget::Guarded {
                kind: GuardKind::GroupOnly,
                path,
            },
            scope: WriteScope::Local(worktree.repo),
            stopped: true,
            remote: None,
            pull: None,
            child: None,
        },
    );
    Some(ReconcileRead {
        id,
        observation: "resolved GUI-E2E reconcile".to_string(),
        stop_proven: true,
        resolution: Resolution::Accounted,
    })
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
    if !read.resolved() {
        return Err(AdmissionError::NeedsReconcile);
    }
    let scope = entry.scope.clone();
    sessions.reconcile.remove(&read.id);
    sessions.release_lease(&scope, read.id);
    Ok(())
}
