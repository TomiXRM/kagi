use super::*;
use kagi_git::{backend::recording::Recording, Backend, OpOutcome};

#[test]
fn continue_receipts_survive_discarded_completion_and_next_detection() {
    if !test_support::run_isolated() {
        return;
    }
    for merge in [false, true] {
        let fixture = if merge {
            merge_conflict_repo()
        } else {
            cherry_pick_conflict_repo()
        };
        let path = fixture.path();
        let backend = Backend::open(path).unwrap();
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&Repository::open(path).unwrap()).unwrap();
        buffer
            .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
            .unwrap();
        let before = backend
            .plan_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
            .unwrap()
            .current;
        // Return immediately after execution: no UI callback, no UI receipt writer.
        if merge {
            let report = Backend::run_recorded_conflict_handoff(
                path,
                &snapshot.session,
                &buffer,
                &snapshot.observation.revision,
                before,
            );
            report.result.unwrap();
            drop(report.recording);
        } else {
            let report = Backend::run_recorded_conflict_continue(
                path,
                &snapshot.session,
                &buffer,
                &snapshot.observation.revision,
                before,
            );
            report.result.unwrap();
            drop(report.recording);
        }
        let receipts: Vec<_> = kagi_git::oplog::read_oplog_tail(100)
            .into_iter()
            .filter(|entry| entry.repo == path.display().to_string())
            .collect();
        assert_eq!(receipts.len(), 1);
        assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
        let next = Backend::open(path).unwrap().conflict_snapshot().unwrap();
        if merge {
            let next = next.expect("merge handoff keeps operation available for commit");
            assert!(next.session.files.is_empty());
            assert!(path.join(".git/MERGE_HEAD").exists());
        } else {
            assert!(
                next.is_none(),
                "finished cherry-pick must not reopen Conflict Mode"
            );
        }
    }
}

#[test]
fn skip_receipt_survives_discarded_completion_and_next_detection() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = cherry_pick_conflict_repo();
    let path = fixture.path();
    let backend = Backend::open(path).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = ResolutionBuffer::from_repo(&Repository::open(path).unwrap()).unwrap();
    let plan = backend
        .plan_conflict_skip(&snapshot.session, &snapshot.observation.revision)
        .unwrap();
    let report = Backend::run_recorded_conflict_skip(path, &plan, &buffer, "cherry-pick-skip");
    assert_eq!(report.result.unwrap().progress, SkipProgress::Finished);
    drop(report.recording);
    let receipts = kagi_git::oplog::read_oplog_tail(100);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].op, "cherry-pick-skip");
    assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
    assert!(Backend::open(path)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .is_none());
}

#[test]
fn merge_handoff_reports_append_failure_without_losing_mutation_outcome() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = merge_conflict_repo();
    let path = fixture.path();
    let backend = Backend::open(path).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&Repository::open(path).unwrap()).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();
    let before = backend
        .plan_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
        .unwrap()
        .current;
    let log = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    std::fs::create_dir(log.join("operations.jsonl")).unwrap();
    let report = Backend::run_recorded_conflict_handoff(
        path,
        &snapshot.session,
        &buffer,
        &snapshot.observation.revision,
        before,
    );
    report.result.unwrap();
    assert!(matches!(report.recording, Recording::Failed { .. }));
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert!(!Repository::open(path)
        .unwrap()
        .index()
        .unwrap()
        .has_conflicts());
    assert!(Backend::open(path)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .is_some());
}
