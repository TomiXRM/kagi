//! Slice 1b: public reservation API, real fixture writers, no window/UI methods.
use kagi::app::*;
use kagi_domain::remove::RemoveFaultPoint;
use kagi_git::{Backend, GitError, OpOutcome, StateSummary};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());
struct TestLog {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    old: Option<std::ffi::OsString>,
}
impl TestLog {
    fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", dir.path());
        Self {
            _guard: guard,
            _dir: dir,
            old,
        }
    }
}
impl Drop for TestLog {
    fn drop(&mut self) {
        match &self.old {
            Some(old) => std::env::set_var("KAGI_LOG_DIR", old),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    linked: PathBuf,
}
fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().canonicalize().unwrap();
        let repo = dir.join("main");
        let linked = dir.join("linked");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("file"), b"original\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(
            &repo,
            &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
        );
        // Entirely local remote: fetch exercises the actual CLI without network.
        git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
        Self {
            _root: root,
            repo,
            linked,
        }
    }
    fn approved(&self, sessions: &mut Sessions) -> Approved {
        let session = sessions.attach(self.repo.clone());
        let owner = sessions.attachment(session).expect("attached");
        let job = plan_remove(
            sessions,
            RemoveRequest {
                owner,
                name: "linked".into(),
                delete_branch: true,
            },
            RemovePolicy::default(),
        );
        assert!(apply_plan(sessions, job.run()));
        let PlanState::Ready { token, .. } = sessions.plan_state() else {
            panic!("ready")
        };
        approve(sessions, token.clone(), RemovePolicy::default()).unwrap()
    }
    fn job(&self, sessions: &mut Sessions) -> RemoveJob {
        let approved = self.approved(sessions);
        prepare_remove(sessions, approved, LegacyBusy(false)).unwrap()
    }
}
#[derive(Clone, Copy, Debug)]
enum Writer {
    Save,
    Stage,
    Snapshot,
    Fetch,
    BranchFetch,
}
const WRITERS: [Writer; 5] = [
    Writer::Save,
    Writer::Stage,
    Writer::Snapshot,
    Writer::Fetch,
    Writer::BranchFetch,
];
fn write(f: &Fixture, writer: Writer, guard: WriteGuard) {
    match writer {
        Writer::Save => {
            guard.run(|| std::fs::write(f.linked.join("file"), b"saved bytes\n").unwrap())
        }
        Writer::Stage => guard.run(|| {
            Backend::open(&f.linked)
                .unwrap()
                .stage_file(Path::new("file"))
                .unwrap()
        }),
        Writer::Snapshot => guard.run(|| {
            Backend::open(&f.linked)
                .unwrap()
                .create_snapshot("admission proof")
                .unwrap();
        }),
        Writer::Fetch | Writer::BranchFetch => {
            let backend = Backend::open(&f.linked).unwrap();
            let result = if matches!(writer, Writer::Fetch) {
                backend.fetch_remote()
            } else {
                backend.fetch_remote_branch("origin/main")
            };
            guard.complete_git(&result);
            result.unwrap();
        }
    }
}

#[test]
fn remove_first_refuses_every_writer_before_bytes_or_index_change() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    for writer in WRITERS {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let job = f.job(&mut sessions);
        let original = std::fs::read(f.linked.join("file")).unwrap();
        // Every production writer obtains this reservation BEFORE its executor.
        match sessions.write_lease(&f.linked, LegacyBusy(false)) {
            Err(error) => assert_eq!(error, AdmissionError::Busy),
            Ok(guard) => {
                write(&f, writer, guard);
                panic!("{writer:?} bypassed remove");
            }
        }
        assert_eq!(std::fs::read(f.linked.join("file")).unwrap(), original);
        assert!(Backend::open(&f.linked)
            .unwrap()
            .working_tree_status()
            .unwrap()
            .staged
            .is_empty());
        drop(job);
        sessions.drain_abandoned();
        assert!(!sessions.has_leases());
    }
}

#[test]
fn every_writer_first_blocks_remove_then_releases_on_completion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    for writer in WRITERS {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let approved = f.approved(&mut sessions);
        let guard = sessions.write_lease(&f.linked, LegacyBusy(false)).unwrap();
        assert!(sessions.has_leases());
        assert!(!sessions.may_close_host());
        assert!(matches!(
            prepare_remove(&mut sessions, approved, LegacyBusy(false)),
            Err(AdmissionError::Busy)
        ));
        assert_eq!(std::fs::read(f.linked.join("file")).unwrap(), b"original\n");
        write(&f, writer, guard);
        assert!(!sessions.has_leases(), "{writer:?}");
        assert!(sessions.may_close_host());
        let completion = f.job(&mut sessions).run();
        if matches!(writer, Writer::Save) {
            assert!(matches!(
                completion.report().recording.entry().outcome,
                OpOutcome::Refused { .. }
            ));
            assert_eq!(
                std::fs::read(f.linked.join("file")).unwrap(),
                b"saved bytes\n"
            );
        } else {
            assert!(matches!(
                completion.report().recording.entry().outcome,
                OpOutcome::Success { .. }
            ));
        }
        apply(&mut sessions, completion);
        assert!(!sessions.has_leases());
    }
}

#[test]
fn stopped_unknown_blocks_all_writers_until_read_and_ack() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    for _writer in WRITERS {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let job = f
            .job(&mut sessions)
            .with_fault_for_test(RemoveFaultPoint::PanicAfterDeletionStarted);
        let id = job.id();
        let completion = job.run();
        assert!(matches!(
            completion.report().recording.entry().outcome,
            OpOutcome::Unknown { .. }
        ));
        apply(&mut sessions, completion);
        assert!(!sessions.has_leases());
        assert!(matches!(
            sessions.write_lease(&f.repo, LegacyBusy(false)),
            Err(AdmissionError::NeedsReconcile)
        ));
        let read = read_reconcile(&sessions, id).unwrap();
        acknowledge(&mut sessions, read).unwrap();
        sessions
            .write_lease(&f.repo, LegacyBusy(false))
            .unwrap()
            .complete();
    }
}

#[test]
fn dropped_or_panicking_writer_does_not_release_an_unconfirmed_lease() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    for panic in [false, true] {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let guard = sessions.write_lease(&f.linked, LegacyBusy(false)).unwrap();
        if panic {
            assert!(std::panic::catch_unwind(|| guard.run(|| panic!("writer"))).is_err());
        } else {
            drop(guard);
        }
        assert!(sessions.has_leases());
        let approved = f.approved(&mut sessions);
        assert!(matches!(
            prepare_remove(&mut sessions, approved, LegacyBusy(false)),
            Err(AdmissionError::Busy)
        ));
    }
}

#[test]
fn fetch_unknown_retains_but_known_failure_releases() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    for unknown in [false, true] {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
        let error = if unknown {
            GitError::TerminationUnknown(kagi_git::Termination::abandoned("timeout"))
        } else {
            GitError::Other("offline".into())
        };
        guard.complete_git(&Err::<(), _>(error));
        assert_eq!(sessions.has_leases(), unknown);
    }
}

#[test]
fn conflict_c0_termination_unknown_records_unknown_and_retains_owner_lease() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let mut sessions = Sessions::new();
    let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    let result = Err::<(), _>(GitError::TerminationUnknown(
        kagi_git::Termination::abandoned("git rebase --continue timed out"),
    ));

    let outcome = kagi::app::settle_conflict_write(
        guard,
        &result,
        StateSummary {
            head: "before".into(),
            dirty: "conflict".into(),
        },
    )
    .expect("unconfirmed termination must produce an Unknown outcome");

    assert!(matches!(
        outcome,
        OpOutcome::Unknown { evidence, .. }
            if evidence.contains("do not retry") && evidence.contains("timed out")
    ));
    assert!(sessions.has_leases(), "the owner lease must remain held");
    assert!(matches!(
        sessions.write_lease(&f.linked, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
}

#[test]
fn canonical_identity_and_conservative_global_exclusion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let other = Fixture::new();
    let mut sessions = Sessions::new();
    assert_eq!(
        Backend::open(&f.repo).unwrap().write_repo_id().unwrap(),
        Backend::open(&f.linked).unwrap().write_repo_id().unwrap()
    );
    assert!(matches!(
        sessions.write_lease(&f.repo, LegacyBusy(true)),
        Err(AdmissionError::Busy)
    ));
    assert!(matches!(
        sessions.write_lease(&f.repo.join("missing"), LegacyBusy(false)),
        Err(AdmissionError::Identity(_))
    ));
    assert!(!sessions.has_leases());
    let guard = sessions.write_lease(&f.linked, LegacyBusy(false)).unwrap();
    assert!(matches!(
        sessions.write_lease(&other.repo, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    std::thread::spawn(move || guard.complete()).join().unwrap();
    sessions
        .write_lease(&other.repo, LegacyBusy(false))
        .unwrap()
        .complete();
}

#[test]
fn untrusted_identity_allows_plain_editor_save_but_git_writers_stay_gated() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let mut backend = Backend::open(&f.linked).unwrap();
    // Existing owner-trust seam: fixture ownership cannot be changed to another uid.
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    assert!(
        backend.write_repo_id().is_ok(),
        "identity is a read, even untrusted"
    );
    let mut sessions = Sessions::new();
    let guard = sessions.write_lease(&f.linked, LegacyBusy(false)).unwrap();
    guard.run(|| std::fs::write(f.linked.join("file"), b"plain editor save\n").unwrap());
    assert_eq!(
        std::fs::read(f.linked.join("file")).unwrap(),
        b"plain editor save\n"
    );
    assert!(!sessions.has_leases());
    assert!(backend
        .stage_file(Path::new("file"))
        .unwrap_err()
        .is_untrusted());
    assert!(backend
        .create_snapshot("untrusted")
        .unwrap_err()
        .is_untrusted());
}

#[test]
fn untrusted_fetch_facades_refuse_before_cli_or_ref_changes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let mut backend = Backend::open(&f.linked).unwrap();
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    assert!(backend.fetch_remote().unwrap_err().is_untrusted());
    assert!(backend
        .fetch_remote_branch("origin/main")
        .unwrap_err()
        .is_untrusted());
    assert!(!f.repo.join(".git/refs/remotes/origin/main").exists());
    assert_eq!(std::fs::read(f.linked.join("file")).unwrap(), b"original\n");
}

#[path = "support/isolated.rs"]
mod test_support;
/// #482 stage 1 G: closing the owning tab is not evidence that the writer
/// stopped. `detach` drops the display, never the reservation, so every real
/// writer stays refused while the reserved operation is still in flight — and a
/// tab reopened on the same path gets no privilege the closed one lacked.
#[test]
fn detach_never_releases_a_reservation_or_admits_the_reopened_tab() {
    let _log = TestLog::new();
    for writer in WRITERS {
        let f = Fixture::new();
        let mut sessions = Sessions::new();
        let owner = sessions.attach(f.repo.clone());
        let job = f.job(&mut sessions);
        let original = std::fs::read(f.linked.join("file")).unwrap();

        // The tab goes away while the reserved operation is still in flight.
        sessions.detach(owner);
        let reopened = sessions.attach(f.repo.clone());
        assert_ne!(reopened, owner, "a reopened path is a new owner");
        assert!(sessions.has_leases(), "tab close is not execution cancel");
        assert!(!sessions.may_close_host());

        match sessions.write_lease(&f.linked, LegacyBusy(false)) {
            Err(error) => assert_eq!(error, AdmissionError::Busy),
            Ok(guard) => {
                write(&f, writer, guard);
                panic!("{writer:?} bypassed a reservation whose tab was closed");
            }
        }
        assert_eq!(std::fs::read(f.linked.join("file")).unwrap(), original);
        drop(job);
    }
}

// ── #702 review 6: a guarded write's unproven termination has an exit ────────

/// A process group with nothing left in it: spawn a child in its own group,
/// wait for it, reuse the id. `kill(-pgid, 0)` then answers ESRCH.
#[cfg(unix)]
fn dead_group() -> u32 {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("true");
    cmd.process_group(0);
    let mut child = cmd.spawn().expect("spawn /usr/bin/true");
    let pid = child.id();
    child.wait().expect("reap it");
    pid
}

/// The direct Fetch paths never reach `Sessions::apply`: they hold a
/// `WriteGuard` and call `complete_git`. Once `ops/fetch.rs` returned
/// `TerminationUnknown` typed, that retained the lease with no entry, no
/// inspect id and no exit — every later write `Busy` and host close refused
/// until restart, which main did not do. A group proven empty releases; one
/// that is not parks the requirement that can release it.
#[cfg(unix)]
#[test]
fn a_guarded_write_that_cannot_prove_its_stop_parks_the_way_out() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let mut sessions = Sessions::new();

    // A stop the executor proved: released, exactly as a clean result is.
    let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    guard
        .for_op("fetch")
        .complete_git(&Err::<(), _>(GitError::TerminationUnknown(
            kagi_git::Termination::stopped("git fetch timed out"),
        )));
    assert!(
        !sessions.has_leases(),
        "an empty group cannot write again: the scope is free"
    );

    // A stop it could not: the lease is held *and* the requirement is parked.
    let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    let group = dead_group();
    guard
        .for_op("fetch")
        .complete_git(&Err::<(), _>(GitError::TerminationUnknown(
            kagi_git::Termination::Unaccounted {
                reason: "git fetch: output collection did not finish".into(),
                group,
            },
        )));
    assert!(
        sessions.has_leases(),
        "an unaccounted group holds the scope"
    );
    let parked = sessions.drain_unaccounted();
    let [(id, op, _)] = parked.as_slice() else {
        panic!("the retained lease must come with the entry that releases it");
    };
    assert_eq!(*op, "fetch");
    assert!(sessions.needs_reconcile(*id), "and the UI can reach it");

    // The group is gone, so the read proves the stop and the ack releases.
    let read = read_reconcile(&sessions, *id).expect("a group is something a read can check");
    assert!(read.stop_proven());
    assert!(read.resolved());
    acknowledge(&mut sessions, read).expect("a proven stop releases the scope");
    assert!(
        !sessions.has_leases(),
        "acknowledging is the exit this path never had"
    );
}

/// A sequencer step is not accounted for by a stopped process.
///
/// `git rebase --skip` that advanced past the commit before its termination
/// went unknown leaves a repository the group probe cannot describe: releasing
/// on "the process stopped" would let the user run the same skip again and lose
/// that commit. So a conflict writer retains even on a proven stop, and only a
/// read of the live sequencer state releases it (#702 review 7). Fetch keeps
/// the group-only rule — `a_guarded_write_that_cannot_prove_its_stop_parks_the_way_out`
/// is the other half of this pair.
#[cfg(unix)]
#[test]
fn a_sequencer_step_is_not_released_by_a_stopped_process_alone() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _log = TestLog::new();
    let f = Fixture::new();
    let mut sessions = Sessions::new();

    let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    guard
        .for_op("conflict-skip")
        .complete_git(&Err::<(), _>(GitError::TerminationUnknown(
            // The executor *did* see the process go. For a fetch that would
            // release; here it proves nothing about how far the skip got.
            kagi_git::Termination::stopped("git rebase --skip: deadline expired"),
        )));
    assert!(
        sessions.has_leases(),
        "a proven stop is not an account of what the sequencer advanced past"
    );

    let parked = sessions.drain_unaccounted();
    let [(id, op, _)] = parked.as_slice() else {
        panic!("the retained lease must come with the entry that releases it");
    };
    assert_eq!(*op, "conflict-skip");

    // The read is the live sequencer state, not a process probe.
    let read = read_reconcile(&sessions, *id).expect("the repository can be read");
    assert!(
        read.observation.starts_with("sequencer="),
        "a sequencer step is reconciled by looking at the sequencer: {}",
        read.observation
    );
    assert!(read.stop_proven() && read.resolved());
    acknowledge(&mut sessions, read).expect("an observed sequencer releases the scope");
    assert!(!sessions.has_leases());

    // And the default side of the classification: a writer nobody named is
    // held, not released. Getting that backwards is how a sequencer added
    // tomorrow becomes re-runnable.
    let guard = sessions.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    guard
        .for_op("some-future-writer")
        .complete_git(&Err::<(), _>(GitError::TerminationUnknown(
            kagi_git::Termination::stopped("timed out"),
        )));
    assert!(
        sessions.has_leases(),
        "an unclassified writer settles under the strict rule"
    );
}
