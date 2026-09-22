//! Skip classification against real app admission/reconciliation, without GPUI.
use super::{isolated, skip_settlement};
use crate::app::{settle_conflict_write, AdmissionError, Sessions};
use kagi_git::{GitError, OpOutcome, SkipOutcome, SkipProgress, StateSummary, Termination};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    dir
}

fn outcome(progress: SkipProgress) -> SkipOutcome {
    SkipOutcome {
        head: None,
        buffer_preserved_at: None,
        progress,
        after: StateSummary {
            head: "observed HEAD".into(),
            dirty: "observed state".into(),
        },
        error: Some(GitError::Other("sequencer observation unavailable".into())),
    }
}

#[test]
fn unclear_skip_retains_lease_rejects_writer_and_registers_reconcile() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    let mut sessions = Sessions::new();
    let result = Ok(outcome(SkipProgress::Unclear));
    let guard = sessions
        .write_lease(dir.path())
        .unwrap()
        .for_op("conflict-skip");
    let settled = settle_conflict_write(
        guard,
        &skip_settlement(&result),
        result.as_ref().unwrap().after.clone(),
    );
    assert!(matches!(settled, Some(OpOutcome::Unknown { .. })));
    assert!(sessions.has_leases());
    assert!(matches!(
        sessions.write_lease(dir.path()),
        Err(AdmissionError::Busy)
    ));
    let parked = sessions.drain_unaccounted();
    let [(id, _, _)] = parked.as_slice() else {
        panic!("the retained Skip must have one reconciliation requirement");
    };
    assert_eq!(sessions.reconcile_ids(), vec![*id]);
    assert!(
        sessions.has_leases(),
        "registering reconcile must not release the writer"
    );
}

#[test]
fn termination_unknown_skip_stays_unknown_and_retains_the_writer() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    // These are distinct admission states: a stopped sequencer is still
    // unresolved; a live group additionally cannot be read safely.
    for termination in [
        Termination::stopped("skip result unaccounted"),
        Termination::Unaccounted {
            reason: "skip process group unaccounted".into(),
            group: std::process::id(),
        },
    ] {
        let mut sessions = Sessions::new();
        let result = Err(GitError::TerminationUnknown(termination));
        let classified = skip_settlement(&result);
        let guard = sessions
            .write_lease(dir.path())
            .unwrap()
            .for_op("conflict-skip");
        let settled =
            settle_conflict_write(guard, &classified, outcome(SkipProgress::Unclear).after);
        assert!(matches!(settled, Some(OpOutcome::Unknown { .. })));
        assert!(sessions.has_leases());
        let parked = sessions.drain_unaccounted();
        let [(id, _, _)] = parked.as_slice() else {
            panic!("typed termination uncertainty must remain reconcilable");
        };
        assert_eq!(sessions.reconcile_ids(), vec![*id]);
    }
}

#[test]
fn known_skip_results_release_the_writer_without_reconcile() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    for result in [
        Ok(outcome(SkipProgress::Finished)),
        Ok(outcome(SkipProgress::Advanced)),
        Ok(outcome(SkipProgress::NoProgress)),
        Err(GitError::Other("skip refused before execution".into())),
    ] {
        let mut sessions = Sessions::new();
        let guard = sessions
            .write_lease(dir.path())
            .unwrap()
            .for_op("conflict-skip");
        let settled = settle_conflict_write(
            guard,
            &skip_settlement(&result),
            outcome(SkipProgress::NoProgress).after,
        );
        assert!(settled.is_none());
        assert!(!sessions.has_leases());
        assert!(sessions.drain_unaccounted().is_empty());
        assert!(sessions.reconcile_ids().is_empty());
        sessions
            .write_lease(dir.path())
            .expect("a known result admits the next writer")
            .complete();
    }
}
