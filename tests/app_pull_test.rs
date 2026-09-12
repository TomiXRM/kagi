//! ADR-0196 Wave 3: the pull workflow through the public application API.
//!
//! Pull is the one family whose job runs up to three recorded children, so the
//! settlement that matters is the *decisive* receipt, not the last one. The
//! cases that decide it — an unconfirmed pull, an `Unknown` stash — cannot be
//! provoked from a real repository (`execute_pull` maps every CLI error to
//! `GitError::Other`), so the job closure is substituted here exactly as the
//! brief allows. What the closure returns is what a real workflow builds.
use kagi::app::*;
use kagi_git::backend::recording::{self, RunReport};
use kagi_git::backend::stash::StashEvidence;
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::{Backend, GitError, OperationOutcome, StateSummary, Termination};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    repo: PathBuf,
    old_log: Option<std::ffi::OsString>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.old_log {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
impl Fixture {
    fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let bare = root.join("origin.git");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["init", "--bare", "-q", bare.to_str().unwrap()]);
        git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", root.join("log"));
        Self {
            _guard: guard,
            _dir: dir,
            repo,
            old_log,
        }
    }
    fn request(&self, s: &mut Sessions) -> PullRequest {
        let session = s.attach(self.repo.clone());
        let owner = s.attachment(session).expect("attached");
        let backend = Backend::open(&self.repo).unwrap();
        PullRequest {
            owner,
            name: "pull",
            path: self.repo.clone(),
            repo: backend.write_repo_id().unwrap(),
            plan: Arc::new(backend.plan_pull().expect("plan pull")),
            auto_stash: true,
            promised_dirty: None,
        }
    }
    /// One durable receipt, exactly as a child of the workflow writes it.
    fn receipt(
        &self,
        op: &str,
        outcome: OpOutcome,
        result: Result<OperationOutcome, GitError>,
    ) -> RunReport {
        let before = StateSummary {
            head: "branch: main".to_string(),
            dirty: "dirty".to_string(),
        };
        RunReport {
            result,
            recording: recording::finalize(OpLogEntry::new(
                op,
                self.repo.display().to_string(),
                before,
                outcome,
            )),
            stash: None,
        }
    }
    fn success(&self, op: &str) -> RunReport {
        self.receipt(
            op,
            OpOutcome::Success {
                after: StateSummary {
                    head: "branch: main".to_string(),
                    dirty: "clean".to_string(),
                },
            },
            Ok(OperationOutcome::Unit),
        )
    }
}

/// The evidence a `StashPush` leaves on its receipt. `oid: None` is the
/// production shape of `StashIdentityUnverified` (#623): the entry exists, but
/// a concurrent external push made it impossible to name.
fn stash_evidence(oid: Option<&str>) -> StashEvidence {
    StashEvidence {
        started: true,
        unknown: oid.is_none(),
        oid: oid.map(str::to_string),
        observations: vec!["the stash entry could not be identified".to_string()],
        ..StashEvidence::default()
    }
}

fn admit(s: &mut Sessions, request: PullRequest, report: PullReport) -> PullJob {
    let approved = approve_pull(s, request).expect("owner attached and identical");
    prepare_pull(s, approved, LegacyBusy(false), Box::new(move || Ok(report))).expect("admitted")
}

fn completed(deliveries: &[Delivery]) -> (OwnerStamp, PullReport, RunReport) {
    deliveries
        .iter()
        .find_map(|d| match d {
            Delivery::Completed { stamp, report, .. } => match &report.evidence {
                FamilyEvidence::Pull(pull) => Some((
                    *stamp,
                    pull.clone(),
                    RunReport {
                        result: Ok(OperationOutcome::Unit),
                        recording: report.recording.clone(),
                        stash: None,
                    },
                )),
                _ => None,
            },
            _ => None,
        })
        .expect("a pull completion is delivered to its owner")
}

/// A pull whose termination is unconfirmed does not pop the auto-stash, keeps
/// the stash as recovery context, retains the lease, and registers exactly one
/// reconcile requirement — which cannot be acknowledged until the writer is
/// proven stopped (ADR-0175 / ADR-0196 決定 5).
#[test]
fn an_unconfirmed_pull_keeps_its_lease_and_its_stash() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let second = request.clone();
    let stash_oid = "1".repeat(40);
    let report = PullReport::settled(
        vec![
            f.success("stash-push"),
            f.receipt(
                "pull",
                OpOutcome::Failed {
                    error: "git fetch deadline expired".to_string(),
                },
                // Kagi lost its own executor mid-write: no proof, and no
                // handle to get one. The hardest case, and the only state that
                // admits it (#702 re-review).
                Err(GitError::TerminationUnknown(Termination::abandoned(
                    "git fetch deadline expired",
                ))),
            ),
        ],
        PullPresentation::Partial {
            error: "pull: termination unconfirmed".to_string(),
        },
        Some(stash_evidence(Some(&stash_oid))),
    );
    let job = admit(&mut s, request, report);
    let id = job.id();

    let deliveries = apply(&mut s, job.run());
    let (_, delivered, _) = completed(&deliveries);
    assert_eq!(
        delivered
            .steps
            .iter()
            .map(|step| step.recording.entry().op.as_str())
            .collect::<Vec<_>>(),
        vec!["stash-push", "pull"],
        "no pop may follow an unconfirmed pull"
    );
    assert_eq!(
        delivered
            .terminal
            .stash
            .as_ref()
            .and_then(|evidence| evidence.oid.as_deref()),
        Some(stash_oid.as_str()),
        "the stash left behind is the recovery context"
    );
    assert!(
        s.has_leases(),
        "an unconfirmed writer keeps the scope reserved"
    );
    assert_eq!(
        prepare_reconcile(&s, id).err().as_deref(),
        Some("execution termination is unconfirmed and nothing is left to probe"),
        "the reconcile requirement is registered and says why it cannot be read"
    );
    // Registered once, and it is what refuses the next pull on this scope.
    let approved = approve_pull(&mut s, second).unwrap();
    assert_eq!(
        prepare_pull(
            &mut s,
            approved,
            LegacyBusy(false),
            Box::new(|| Err("never run".to_string()))
        )
        .err(),
        Some(AdmissionError::NeedsReconcile)
    );
}

/// The same unconfirmed pull, but the executor **did** account for the child:
/// it killed it and collected it. That is the stop proof, so the scope is
/// released at settlement and the `Unknown` receipt is reconcilable rather than
/// parked behind a guard nothing can ever satisfy (#702 review P1).
#[test]
fn a_reaped_child_is_the_stop_proof_that_reopens_the_scope() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let report = PullReport::settled(
        vec![f.receipt(
            "pull",
            OpOutcome::Unknown {
                after: StateSummary {
                    head: "branch: main".to_string(),
                    dirty: "unknown".to_string(),
                },
                evidence: "git fetch timed out after 60s".to_string(),
            },
            Err(GitError::TerminationUnknown(Termination::stopped(
                "git fetch timed out after 60s",
            ))),
        )],
        PullPresentation::Partial {
            error: "pull: termination unconfirmed".to_string(),
        },
        None,
    );
    let job = admit(&mut s, request, report);
    let id = job.id();
    apply(&mut s, job.run());

    assert!(
        !s.has_leases(),
        "a child the executor collected cannot write again: the scope is free"
    );
    let read = read_reconcile(&s, id).expect("a proven-stopped writer is reconcilable");
    assert!(read.stop_proven());
    acknowledge(&mut s, read).expect("acknowledged");
    assert_eq!(
        prepare_reconcile(&s, id).err().as_deref(),
        Some("no reconcile request"),
        "acknowledging clears the requirement"
    );
}

/// A group that could **not** be reaped is not a dead end either: it is the
/// handle, and the reconcile read asks the OS before it reads anything. This
/// process's own group is certainly alive, so the read must come back
/// unresolved *without observing*; a group that is gone must let it through.
#[test]
fn an_unaccounted_group_is_proven_stopped_by_going_away() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let alive = LiveGroup::spawn();
    let report = |pid: u32| {
        PullReport::settled(
            vec![f.receipt(
                "pull",
                OpOutcome::Unknown {
                    after: StateSummary {
                        head: "branch: main".to_string(),
                        dirty: "unknown".to_string(),
                    },
                    evidence: "the local child could not be stopped".to_string(),
                },
                Err(GitError::TerminationUnknown(Termination::Unaccounted {
                    reason: "the local child could not be stopped".to_string(),
                    group: pid,
                })),
            )],
            PullPresentation::Partial {
                error: "pull: termination unconfirmed".to_string(),
            },
            None,
        )
    };

    let request = f.request(&mut s);
    let job = admit(&mut s, request, report(alive.id()));
    let id = job.id();
    apply(&mut s, job.run());
    assert!(s.has_leases(), "a live child keeps the scope reserved");
    let read = read_reconcile(&s, id).expect("a group is something a read can check");
    assert!(
        !read.stop_proven(),
        "the group holds a live process: it has demonstrably not stopped"
    );
    assert!(
        read.observation.contains("still running"),
        "and nothing was observed while it runs — a pre-mutation snapshot must \
         not come back wearing a post-mutation label: {}",
        read.observation
    );
    assert_eq!(
        acknowledge(&mut s, read).err(),
        Some(AdmissionError::NeedsReconcile),
        "an unproven stop must not release the scope"
    );

    // A group nothing answers to: the writer is gone and the read says so.
    let gone = dead_group();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let job = admit(&mut s, request, report(gone));
    let id = job.id();
    apply(&mut s, job.run());
    let read = read_reconcile(&s, id).expect("readable");
    assert!(read.stop_proven(), "a group nothing answers to is stopped");
    acknowledge(&mut s, read).expect("a proven stop releases the scope");
    assert!(!s.has_leases());
}

/// A process group with nothing left in it: spawn a child in its own group,
/// wait for it, and reuse the id. `kill(-pgid, 0)` then answers ESRCH.
fn dead_group() -> u32 {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("true");
    cmd.process_group(0);
    let mut child = cmd.spawn().expect("spawn /usr/bin/true");
    let pid = child.id();
    child.wait().expect("reap it");
    pid
}

/// A process group that is certainly alive: our own child, in its own group,
/// stopped when the guard drops.
struct LiveGroup(std::process::Child);
impl LiveGroup {
    fn spawn() -> Self {
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30").process_group(0);
        Self(cmd.spawn().expect("spawn /bin/sleep"))
    }
    fn id(&self) -> u32 {
        self.0.id()
    }
}
impl Drop for LiveGroup {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The decisive receipt is not the last one run: a pull that failed and whose
/// stash was then restored settles as a failed pull, releases its lease, and
/// needs no reconciliation.
#[test]
fn a_restored_failure_settles_on_the_pull_receipt() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let owner = request.owner.clone();
    let pull = f.receipt(
        "pull",
        OpOutcome::Failed {
            error: "fetch failed".to_string(),
        },
        Err(GitError::Other("fetch failed".into())),
    );
    let report = PullReport::new(
        vec![f.success("stash-push"), pull, f.success("stash-pop")],
        1,
        PullPresentation::Failed {
            error: "pull failed; your changes were restored".to_string(),
        },
        None,
    );
    let job = admit(&mut s, request, report);
    let id = job.id();

    let deliveries = apply(&mut s, job.run());
    let (_, _, presented) = completed(&deliveries);
    assert_eq!(
        presented.recording.entry().op,
        "pull",
        "the workflow is presented through the receipt that decided it"
    );
    assert!(
        matches!(
            presented.recording.entry().outcome,
            OpOutcome::Failed { .. }
        ),
        "a trailing successful pop must not report the workflow as done"
    );
    assert!(!s.has_leases(), "a stopped workflow releases its lease");
    assert_eq!(
        prepare_reconcile(&s, id).err().as_deref(),
        Some("no reconcile request"),
        "a known failure needs no reconciliation"
    );
    assert!(
        deliveries.iter().any(|d| matches!(
            d,
            Delivery::Invalidate(target) if Some(&target.worktree) == owner.worktree.as_ref()
        )),
        "the owner's worktree is invalidated"
    );
}

/// The production `StashIdentityUnverified` shape: a stash **was** created but
/// no OID came back (#623), so the recovery context is the backend's evidence
/// with `oid: None`. The read has to hunt for the entry by the message the
/// workflow wrote, and until it can point at exactly one, `acknowledge` must
/// not release the scope (#702 review P1).
#[test]
fn an_unidentified_auto_stash_is_hunted_down_before_the_scope_reopens() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    // The stash the workflow created — the one nothing could name at the time.
    std::fs::write(f.repo.join("a.txt"), "work in progress\n").unwrap();
    git(&f.repo, &["stash", "push", "-q", "-m", AUTO_STASH_MESSAGE]);
    // And a second entry answering to the same message: exactly the ambiguity
    // that made the identity unverifiable in the first place.
    std::fs::write(f.repo.join("a.txt"), "more work\n").unwrap();
    git(&f.repo, &["stash", "push", "-q", "-m", AUTO_STASH_MESSAGE]);

    let request = f.request(&mut s);
    let second = request.clone();
    let report = |f: &Fixture| {
        PullReport::settled(
            vec![f.receipt(
                "stash-push",
                OpOutcome::Unknown {
                    after: StateSummary {
                        head: "branch: main".to_string(),
                        dirty: "unknown".to_string(),
                    },
                    evidence: "the stash entry could not be identified".to_string(),
                },
                Err(GitError::StashIdentityUnverified("concurrent push".into())),
            )],
            PullPresentation::Partial {
                error: "auto-stash identity unverified".to_string(),
            },
            // Production hands over evidence, not an OID: there is none.
            Some(stash_evidence(None)),
        )
    };
    let job = admit(&mut s, request, report(&f));
    let id = job.id();
    apply(&mut s, job.run());

    let approved = approve_pull(&mut s, second.clone()).unwrap();
    assert_eq!(
        prepare_pull(
            &mut s,
            approved,
            LegacyBusy(false),
            Box::new(|| Err("never run".to_string()))
        )
        .err(),
        Some(AdmissionError::NeedsReconcile),
        "an unacknowledged Unknown closes the scope to new writes"
    );

    let read = read_reconcile(&s, id).expect("a stopped writer is reconcilable");
    assert!(
        read.observation.contains("head=branch: main")
            && read.observation.contains("upstream=origin/main")
            && read.observation.contains("dirty="),
        "the read must name HEAD, the upstream and the tree: {}",
        read.observation
    );
    assert!(
        read.observation
            .contains("2 entries answer to the auto-stash"),
        "the read must say it cannot tell the entries apart: {}",
        read.observation
    );
    assert!(!read.resolved(), "an ambiguous stash is not accounted for");
    assert_eq!(
        acknowledge(&mut s, read).err(),
        Some(AdmissionError::NeedsReconcile),
        "acknowledging would report 'settled' about work kagi cannot point at"
    );

    // The user resolves the ambiguity themselves — that is the exit. One entry
    // answers to the message now, so the read can name it and let go.
    git(&f.repo, &["stash", "drop", "-q", "stash@{0}"]);
    let read = read_reconcile(&s, id).expect("still reconcilable");
    assert!(
        read.observation
            .contains("unidentified auto-stash resolved to"),
        "the surviving entry must be named: {}",
        read.observation
    );
    assert!(read.resolved());
    acknowledge(&mut s, read).expect("an accounted-for stash releases the scope");
    let approved = approve_pull(&mut s, second).unwrap();
    assert!(
        prepare_pull(
            &mut s,
            approved,
            LegacyBusy(false),
            Box::new(|| Err("never run".to_string()))
        )
        .is_ok(),
        "acknowledging reopens the scope"
    );
}

/// A task that unwinds is not a completion, but it is not nothing either: the
/// write may have happened. The abandonment settles as `Unknown` so the lease
/// it holds has a reconcile entry to be acknowledged against, instead of an
/// operation that can never be settled (#289 / #702 review P1).
#[test]
fn an_abandoned_pull_task_settles_as_unknown_and_keeps_its_lease() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let report = PullReport::settled(
        vec![f.success("pull")],
        PullPresentation::Success {
            summary: "never reached".to_string(),
        },
        None,
    );
    let job = admit(&mut s, request, report);
    let id = job.id();
    let abandonment = job.abandonment();
    // The task unwinds: `job.run()` is never called.
    drop(job);

    let deliveries = apply(&mut s, abandonment.into_completion());
    let (_, _, presented) = completed(&deliveries);
    assert!(
        matches!(
            presented.recording.entry().outcome,
            OpOutcome::Unknown { .. }
        ),
        "a panic is not evidence of termination: {:?}",
        presented.recording.entry().outcome
    );
    assert!(
        s.has_leases(),
        "the lease stays held — nothing proved the write stopped"
    );
    assert_eq!(
        prepare_reconcile(&s, id).err().as_deref(),
        Some("execution termination is unconfirmed and nothing is left to probe"),
        "and it is a registered reconcile requirement that says why it cannot be \
         read: kagi lost its own executor, so there is no handle to probe"
    );
}

/// #702 re-review — a local snapshot cannot say whether a push landed.
///
/// `oplog_outcome_from` now records `Unknown` for push-shaped operations too,
/// so their reconcile reads have to mean something. Reading HEAD and calling it
/// resolved would let an unconfirmed remote mutation be acknowledged and
/// retried, which is the thing ADR-0177 exists to prevent. The read asks the
/// remote, and only a remote that already carries the local tip is confirmed.
#[test]
fn a_remote_write_is_resolved_only_by_the_remote() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let session = s.attach(f.repo.clone());
    let owner = s.attachment(session).expect("attached");
    let backend = Backend::open(&f.repo).unwrap();
    let request = RunRequest {
        owner,
        name: "push",
        path: f.repo.clone(),
        repo: backend.write_repo_id().unwrap(),
        plan: Arc::new(backend.plan_push().expect("plan push")),
        // ponytail: no per-operation target in `RunRequest`; the read derives
        // the branch from HEAD, which is what `Operation::Push` pushes.
    };
    let second = request.clone();
    let unknown = f.receipt(
        "push",
        OpOutcome::Unknown {
            after: StateSummary {
                head: "branch: main".to_string(),
                dirty: "unknown".to_string(),
            },
            evidence: "git push timed out".to_string(),
        },
        Err(GitError::TerminationUnknown(Termination::stopped(
            "git push timed out",
        ))),
    );
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        Box::new(move || Ok(unknown)),
    )
    .unwrap();
    let id = job.id();
    apply(&mut s, job.run());

    // The fixture pushed `main` already, so the remote *does* carry the local
    // tip: the write is confirmed and the scope can be reopened.
    let read = read_reconcile(&s, id).expect("readable");
    assert!(
        read.observation.contains("origin/main=") && read.observation.contains("confirmed=true"),
        "the read must name the live remote ref, not just HEAD: {}",
        read.observation
    );
    assert!(read.resolved());
    acknowledge(&mut s, read).expect("a confirmed remote state releases the scope");

    // Now the local branch moves ahead of the remote: the same read can no
    // longer say the push landed, so it must not let the scope reopen.
    git(&f.repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    let unknown = f.receipt(
        "push",
        OpOutcome::Unknown {
            after: StateSummary {
                head: "branch: main".to_string(),
                dirty: "unknown".to_string(),
            },
            evidence: "git push timed out".to_string(),
        },
        Err(GitError::TerminationUnknown(Termination::stopped(
            "git push timed out",
        ))),
    );
    let approved = approve_run(&mut s, second).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        Box::new(move || Ok(unknown)),
    )
    .unwrap();
    let id = job.id();
    apply(&mut s, job.run());
    let read = read_reconcile(&s, id).expect("readable");
    assert!(
        read.observation.contains("confirmed=false"),
        "the remote does not carry the local tip: {}",
        read.observation
    );
    assert!(!read.resolved());
    assert_eq!(
        acknowledge(&mut s, read).err(),
        Some(AdmissionError::NeedsReconcile),
        "an unconfirmed remote mutation must never become retryable"
    );
}

/// The completion is routed by the stamp frozen at admission. Leaving the tab
/// while the workflow runs does not re-target it: the visit no longer matches,
/// which is exactly what makes the UI drop the presentation (`op result
/// dropped`) instead of showing it on whatever tab is now on screen.
#[test]
fn a_pull_completion_is_routed_by_the_stamp_frozen_at_admission() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let owner = request.owner.clone();
    let report = PullReport::settled(
        vec![f.success("pull")],
        PullPresentation::Success {
            summary: "already up to date".to_string(),
        },
        None,
    );
    let job = admit(&mut s, request, report);
    let stamp = job.stamp();
    assert_eq!((stamp.session, stamp.visit), (owner.session, owner.visit));

    s.depart(owner.session);
    let deliveries = apply(&mut s, job.run());
    let (delivered, _, _) = completed(&deliveries);
    assert_eq!(delivered, stamp, "routed by the stamp minted at admission");
    assert_ne!(
        s.attachment(owner.session).map(|now| now.visit),
        Some(delivered.visit),
        "the visit moved on, so the presentation is dropped rather than misplaced"
    );
}
