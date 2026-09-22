//! #706: the explicit release of a stopped remote write kagi could not observe.
//!
//! The ordinary acknowledgement is unchanged and still means the strictest
//! thing kagi says about a remote write: "I looked, and the remote holds what
//! was promised". These tests are about the *other* answer — the one where
//! looking is not currently possible — and about everything that
//! must keep refusing to be confused with it: a writer that has not stopped, a
//! promise the remote disagrees with, a transport that could not be reached,
//! and a release nobody could record.
//!
//! Fault injection is unavailable at this level, so the job closure stands in
//! for the executor exactly as the neighbouring app tests do. What it returns
//! is what a real `git push` whose child could not be reaped builds.
use kagi::app::*;
use kagi_git::backend::recording::{self, Recording, RunReport};
use kagi_git::backend::remote_ref::{RemoteExpect, RemoteExpectation};
use kagi_git::oplog::{read_oplog_tail, OpLogEntry, OpOutcome};
use kagi_git::{Backend, GitError, StateSummary, Termination};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    root: PathBuf,
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
    /// A repository with a real `origin` it has already pushed `main` to, so a
    /// frozen expectation can be compared against something that answers.
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
            root,
            repo,
            old_log,
        }
    }
    fn request(
        &self,
        s: &mut Sessions,
        name: &'static str,
        remote: Vec<RemoteExpectation>,
    ) -> RunRequest {
        let session = s.attach(self.repo.clone());
        let owner = s.attachment(session).expect("attached");
        let backend = Backend::open(&self.repo).unwrap();
        RunRequest {
            owner,
            name,
            path: self.repo.clone(),
            repo: backend.write_repo_id().unwrap(),
            plan: Arc::new(backend.plan_push().expect("plan push")),
            remote,
        }
    }
    /// What `finish_run` freezes for a real push: the local tip, named before
    /// the write, compared against the remote afterwards.
    fn frozen_push_promise(&self) -> Vec<RemoteExpectation> {
        let backend = Backend::open(&self.repo).unwrap();
        let plan = backend.plan_push().expect("plan push");
        let frozen = backend.remote_expectation("push", &plan);
        assert!(!frozen.is_empty(), "the fixture must name a remote effect");
        frozen
    }
    fn tip(&self) -> String {
        git(&self.repo, &["rev-parse", "refs/heads/main"])
    }
}

/// The receipt a push whose child could not be accounted for leaves behind:
/// `Unknown`, durable, and the reason the lease is retained.
fn unknown_receipt(repo: &Path, op: &str, before: StateSummary, end: Termination) -> RunReport {
    let evidence = format!("git {op}: the child could not be accounted for");
    RunReport {
        result: Err(GitError::TerminationUnknown(end)),
        recording: recording::finalize(OpLogEntry::new(
            op,
            repo.display().to_string(),
            before.clone(),
            OpOutcome::Unknown {
                after: StateSummary {
                    head: before.head,
                    dirty: "unknown".to_string(),
                },
                evidence,
            },
        )),
        stash: None,
    }
}

/// Admit the write, settle it as an unproven termination, and hand back the
/// operation the reconcile requirement is parked under.
fn park(s: &mut Sessions, request: RunRequest, end: Termination) -> OperationId {
    let op = request.name;
    let path = request.path.clone();
    let before = request.plan.current.clone();
    let approved = approve_run(s, request).expect("the owner is attached and unchanged");
    let job = prepare_run(
        s,
        approved,
        Box::new(move || Ok(unknown_receipt(&path, op, before, end))),
    )
    .expect("admitted");
    let id = job.id();
    apply(s, job.run());
    id
}

/// Whether the scope admits another write of the same family — the thing an
/// open reconcile requirement refuses and a release has to give back.
fn admits_another_write(f: &Fixture, s: &mut Sessions) -> Result<(), AdmissionError> {
    let request = f.request(s, "push", Vec::new());
    let approved = approve_run(s, request).expect("the owner is still attached");
    prepare_run(s, approved, Box::new(|| Err("never run".to_string()))).map(|_| ())
}

/// The kagi operation id as the audit evidence spells it.
fn operation_number(id: OperationId) -> String {
    format!("{id:?}")
        .chars()
        .filter(char::is_ascii_digit)
        .collect()
}

/// A known remote-writing family that never named what it was about to make
/// true — the shape `RunRequest::remote` documents, and what a PR merge against
/// a repository kagi cannot identify actually produces.
///
/// Nothing will ever confirm this write, so the ordinary acknowledgement must
/// keep refusing it forever; the explicit release is the only exit, it records
/// before it releases, and what it records never claims the push landed.
#[test]
fn an_unnamable_remote_effect_is_released_only_through_the_explicit_exit() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s, "push", Vec::new());
    let id = park(&mut s, request, Termination::stopped("git push timed out"));

    assert_eq!(
        s.blocking_reconcile(),
        Some(id),
        "an Unknown push parks the requirement that holds the scope"
    );
    let read = read_reconcile(&s, id).expect("a proven-stopped writer is readable");
    assert!(read.stop_proven());
    assert!(
        !read.resolved() && !read.settled(),
        "an unobservable remote is never resolved, and `settled()` must stay \
         the whole rule a presenter consumes: {}",
        read.observation
    );
    assert!(
        read.can_acknowledge_unobserved(),
        "but it is a dead end the user may be offered a way out of: {}",
        read.observation
    );
    assert_eq!(
        acknowledge(&mut s, read.clone()).err(),
        Some(AdmissionError::NeedsReconcile),
        "the ordinary acknowledgement still means 'kagi looked and it agreed'"
    );
    assert_eq!(
        admits_another_write(&f, &mut s).err(),
        Some(AdmissionError::NeedsReconcile),
        "and until the user acts, the requirement still refuses every write"
    );

    let report = prepare_unobservable_release(&s, read)
        .expect("an unobservable promise is what the explicit exit is for")
        .run();
    let Recording::Appended { entry, .. } = report.recording() else {
        panic!("the audit record must be durable: {:?}", report.recording());
    };
    assert_eq!(entry.op, "reconcile-release-unobservable");
    let OpOutcome::Unknown { evidence, .. } = &entry.outcome else {
        panic!(
            "an administrative release is never a success: {:?}",
            entry.outcome
        );
    };
    assert!(
        evidence.contains("push")
            && evidence.contains(&format!("kagi operation {}", operation_number(id))),
        "the row must name the operation it released, not only itself: {evidence}"
    );
    assert!(
        evidence.contains("did NOT verify"),
        "and must say, in the record itself, that the remote was not verified: \
         {evidence}"
    );
    acknowledge_unobserved(&mut s, report).expect("a recorded release closes the requirement");

    // Two rows: the original Unknown receipt is still exactly what it was.
    // Rewriting it would erase the only durable statement that the push's
    // result is unknown.
    let tail = read_oplog_tail(10);
    assert_eq!(tail.len(), 2, "a release is a second row, never an edit");
    assert_eq!(tail[0].op, "reconcile-release-unobservable");
    assert_eq!(tail[1].op, "push");
    assert!(
        matches!(tail[1].outcome, OpOutcome::Unknown { .. }),
        "the push's own receipt still says its result is unknown"
    );

    assert!(s.reconcile_ids().is_empty());
    assert!(
        admits_another_write(&f, &mut s).is_ok(),
        "and the scope the requirement held is genuinely reopened"
    );
}

/// The typed half of the same answer (#706 backend slice): the promise was
/// named, and the remote it names cannot be addressed from this machine at all.
/// That is not "the ref is absent": explicit release records the lack of an
/// observation without claiming the remote holds what was promised.
#[test]
fn a_remote_that_cannot_be_addressed_is_releasable_but_never_resolved() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let promise = vec![RemoteExpectation::Ref {
        // No such remote in this repository: nothing to resolve to a URL, so
        // there is no question `ls-remote` could even be asked.
        remote: "nowhere".to_string(),
        refname: "refs/heads/main".to_string(),
        expect: RemoteExpect::Oid(f.tip()),
    }];
    let request = f.request(&mut s, "push", promise);
    let id = park(&mut s, request, Termination::stopped("git push timed out"));

    let read = read_reconcile(&s, id).expect("a proven-stopped writer is readable");
    assert!(
        !read.resolved(),
        "an unobservable remote confirms nothing: {}",
        read.observation
    );
    assert!(
        read.observation.contains("unobservable"),
        "and the user is told which promise could not be observed: {}",
        read.observation
    );
    assert!(read.can_acknowledge_unobserved());
    assert_eq!(
        acknowledge(&mut s, read.clone()).err(),
        Some(AdmissionError::NeedsReconcile)
    );

    let report = prepare_unobservable_release(&s, read)
        .expect("a typed unobservable promise qualifies")
        .run();
    acknowledge_unobserved(&mut s, report).expect("recorded, then released");
    assert!(s.reconcile_ids().is_empty());
}

/// The invariant everything else rests on: nothing is read, and nothing is
/// releasable, while the writer may still be running. An unobservable promise
/// does not buy an early exit from it — the remote being unreadable says
/// nothing about whether the push is still going.
#[cfg(unix)]
#[test]
fn a_live_writer_is_never_releasable_however_unobservable_its_promise() {
    use std::os::unix::process::CommandExt;
    let f = Fixture::new();
    let mut s = Sessions::new();
    let mut alive = {
        let mut cmd = Command::new("sleep");
        cmd.arg("30").process_group(0);
        cmd.spawn().expect("spawn /bin/sleep")
    };
    let request = f.request(&mut s, "push", Vec::new());
    let id = park(
        &mut s,
        request,
        Termination::Unaccounted {
            reason: "the push child could not be stopped".to_string(),
            group: alive.id(),
        },
    );

    assert!(s.has_leases(), "a live writer keeps the scope reserved");
    let read = read_reconcile(&s, id).expect("a group is something a read can check");
    assert!(!read.stop_proven());
    assert!(
        read.observation.contains("still running"),
        "nothing is observed while it runs: {}",
        read.observation
    );
    assert!(
        !read.can_acknowledge_unobserved(),
        "an unproven stop is a *later* answer, not a dead end to step out of"
    );
    assert_eq!(
        prepare_unobservable_release(&s, read).err(),
        Some(AdmissionError::NeedsReconcile),
        "and the explicit exit refuses it too — releasing here would report a \
         settled write about one that may still be happening"
    );
    assert!(s.has_leases());
    assert_eq!(s.blocking_reconcile(), Some(id));
    let _ = alive.kill();
    let _ = alive.wait();
}

/// The remote answered, and it does not hold what was promised. This is the
/// opposite of unobservable: kagi knows something, and what it knows is that
/// the push did not land. Releasing it would be the false "settled" ADR-0177
/// exists to prevent.
#[test]
fn a_promise_the_remote_disagrees_with_is_never_releasable() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    // A local commit that was never pushed: the promise names the new tip, the
    // remote still carries the old one.
    git(
        &f.repo,
        &["commit", "-q", "--allow-empty", "-m", "never pushed"],
    );
    let request = f.request(&mut s, "push", f.frozen_push_promise());
    let id = park(&mut s, request, Termination::stopped("git push timed out"));

    let read = read_reconcile(&s, id).expect("the remote answers");
    assert!(
        read.observation.contains("confirmed=false"),
        "the frozen promise is compared against the live ref: {}",
        read.observation
    );
    assert!(!read.resolved());
    assert!(
        !read.can_acknowledge_unobserved(),
        "an observed disagreement is not a dead end: another look may say more"
    );
    assert_eq!(
        acknowledge(&mut s, read.clone()).err(),
        Some(AdmissionError::NeedsReconcile)
    );
    assert_eq!(
        prepare_unobservable_release(&s, read).err(),
        Some(AdmissionError::NeedsReconcile)
    );
    assert_eq!(s.blocking_reconcile(), Some(id));
    assert_eq!(read_oplog_tail(10).len(), 1, "nothing was recorded either");
}

/// A batch of promises where one is unobservable and one was read and broken.
/// The broken one decides: half an observation is not an absence of one, and a
/// batch that is partly wrong is wrong.
#[test]
fn one_broken_promise_dominates_an_unobservable_sibling() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let promise = vec![
        RemoteExpectation::Ref {
            remote: "nowhere".to_string(),
            refname: "refs/heads/main".to_string(),
            expect: RemoteExpect::Oid(f.tip()),
        },
        RemoteExpectation::Ref {
            remote: "origin".to_string(),
            refname: "refs/heads/main".to_string(),
            // Readable, and not what the remote holds.
            expect: RemoteExpect::Oid("f".repeat(40)),
        },
    ];
    let request = f.request(&mut s, "push", promise);
    let id = park(&mut s, request, Termination::stopped("git push timed out"));

    let read = read_reconcile(&s, id).expect("one of the two answers");
    assert!(!read.resolved());
    assert!(
        !read.can_acknowledge_unobserved(),
        "an unobservable sibling must not launder a promise that was read and \
         broken: {}",
        read.observation
    );
    assert_eq!(
        prepare_unobservable_release(&s, read).err(),
        Some(AdmissionError::NeedsReconcile)
    );
    assert_eq!(s.blocking_reconcile(), Some(id));
}

/// The remote resolves to somewhere, and talking to it failed. That is a
/// communication failure, not an unobservable address: the read itself fails,
/// there is no capability to offer, and the requirement stays exactly where it
/// was until the transport works again.
#[test]
fn a_transport_that_could_not_be_reached_offers_no_release_at_all() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s, "push", f.frozen_push_promise());
    let id = park(&mut s, request, Termination::stopped("git push timed out"));
    // The address still resolves; the thing at the end of it is gone.
    git(
        &f.repo,
        &[
            "remote",
            "set-url",
            "origin",
            f.root.join("vanished.git").to_str().unwrap(),
        ],
    );

    assert!(
        read_reconcile(&s, id).is_err(),
        "a remote that cannot be read is no read at all"
    );
    assert_eq!(
        s.blocking_reconcile(),
        Some(id),
        "so the requirement is left exactly where it was"
    );
    assert_eq!(
        admits_another_write(&f, &mut s).err(),
        Some(AdmissionError::NeedsReconcile),
        "and it still refuses every write on the scope"
    );
    assert_eq!(read_oplog_tail(10).len(), 1, "nothing was recorded");
}

/// A release nobody could record is not a release. The requirement and the
/// lease stay, the cause is handed back, and the second attempt — once the log
/// is writable again — both records and releases. Replaying the first, spent
/// report afterwards changes nothing.
#[test]
fn an_unrecorded_release_keeps_the_requirement_and_says_why() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s, "push", Vec::new());
    let id = park(&mut s, request, Termination::stopped("git push timed out"));
    let read = read_reconcile(&s, id).expect("readable");
    assert!(read.can_acknowledge_unobserved());

    // A log directory that cannot be created: its parent is a file.
    let blocked = f.root.join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    std::env::set_var("KAGI_LOG_DIR", blocked.join("log"));
    let failed = prepare_unobservable_release(&s, read.clone())
        .expect("still eligible")
        .run();
    let Recording::Failed {
        error: recording_error,
        ..
    } = failed.recording()
    else {
        panic!("the fixture must actually break the append");
    };
    let recording_error = recording_error.clone();
    assert_eq!(failed.operation(), id);
    let error = acknowledge_unobserved(&mut s, failed)
        .expect_err("an unauditable release must not release");
    assert!(
        error.contains(&recording_error),
        "the release error must retain the actual persistence failure: {error}"
    );
    assert_eq!(s.blocking_reconcile(), Some(id));
    assert_eq!(
        admits_another_write(&f, &mut s).err(),
        Some(AdmissionError::NeedsReconcile)
    );

    // The log is writable again: record, then release.
    std::env::set_var("KAGI_LOG_DIR", f.root.join("log"));
    let recorded = prepare_unobservable_release(&s, read.clone())
        .expect("the requirement is still eligible")
        .run();
    let spent = prepare_unobservable_release(&s, read)
        .expect("armed twice by a user who clicked twice")
        .run();
    acknowledge_unobserved(&mut s, recorded).expect("recorded, then released");
    assert!(s.reconcile_ids().is_empty());
    assert!(
        acknowledge_unobserved(&mut s, spent).is_err(),
        "a second report for a requirement that is gone releases nothing"
    );
}

/// The allowlist points the safe way round. A remote-writing family nobody has
/// classified yet gets no exit from this path: "kagi does not know what this
/// operation does to a remote" is not "kagi knows it cannot be observed".
#[test]
fn an_operation_kagi_cannot_name_stays_blocked() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s, "some-future-remote-writer", Vec::new());
    let id = park(&mut s, request, Termination::stopped("timed out"));

    let read = read_reconcile(&s, id).expect("readable");
    assert!(read.stop_proven());
    assert!(!read.resolved());
    assert!(
        !read.can_acknowledge_unobserved(),
        "an unclassified writer must not inherit a release nobody reasoned \
         about for it: {}",
        read.observation
    );
    assert_eq!(
        prepare_unobservable_release(&s, read.clone()).err(),
        Some(AdmissionError::NeedsReconcile)
    );
    assert_eq!(
        acknowledge(&mut s, read).err(),
        Some(AdmissionError::NeedsReconcile)
    );
    assert_eq!(s.blocking_reconcile(), Some(id));
}
