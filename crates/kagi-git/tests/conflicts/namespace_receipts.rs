use super::*;
use kagi_domain::conflict_family::{ConflictProgress, ConflictRequest};
use kagi_git::{
    backend::{
        conflict_ops::{ConflictFaultPoint, ConflictPlan},
        ExecutionPolicy,
    },
    Backend, DirFileChoice, OpOutcome,
};

#[derive(Clone, Copy, Debug)]
enum Route {
    Delete,
    KeepDirectory,
    KeepFile,
}

fn fixture(route: Route) -> (TempDir, ConflictPlan) {
    if matches!(route, Route::Delete) {
        let tmp = deletion::fixture(false, false);
        let plan = deletion::plan(tmp.path(), Path::new("renamed-src.ts"));
        return (tmp, plan);
    }
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    init_repo(root, "main");
    std::fs::create_dir(root.join("thing")).unwrap();
    write_file(root, "thing/child", "base child\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["checkout", "-qb", "incoming"]);
    write_file(root, "thing/child", "kept child\n");
    git(root, &["commit", "-qam", "directory"]);
    git(root, &["checkout", "-q", "main"]);
    git(root, &["rm", "-r", "thing"]);
    write_file(root, "thing", "kept file\n");
    git(root, &["add", "thing"]);
    git(root, &["commit", "-qm", "file"]);
    assert!(!git_allow_fail(root, &["merge", "incoming"]));
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let request = ConflictRequest::ResolveDirFile {
        path: "thing".into(),
        revision: snapshot.observation.revision,
        choice: match route {
            Route::KeepDirectory => DirFileChoice::KeepDirectory,
            Route::KeepFile => DirFileChoice::KeepFile,
            Route::Delete => unreachable!(),
        },
    };
    let plan = Backend::plan_recorded_conflict(root, request).unwrap();
    (tmp, plan)
}

fn measured_state(root: &Path) -> kagi_git::ops::StateSummary {
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    // Planning Continue exposes the same real current_state_summary used by
    // the receipt without inferring a successful result from the request.
    backend
        .plan_conflict_continue(&snapshot.session, &buffer, &snapshot.observation.revision)
        .unwrap()
        .current
}

fn assert_worktree_mutated(route: Route, root: &Path) {
    match route {
        Route::Delete => assert!(!root.join("renamed-src.ts").exists()),
        Route::KeepDirectory => {
            assert_eq!(
                std::fs::read(root.join("thing/child")).unwrap(),
                b"kept child\n"
            );
            assert!(!root.join("thing~HEAD").exists());
        }
        Route::KeepFile => {
            assert_eq!(std::fs::read(root.join("thing")).unwrap(), b"kept file\n");
            assert!(!root.join("thing~HEAD").exists());
        }
    }
}

fn check_receipt(fault: Option<ConflictFaultPoint>) {
    for route in [Route::Delete, Route::KeepDirectory, Route::KeepFile] {
        let (tmp, plan) = fixture(route);
        let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), fault);
        assert_worktree_mutated(route, tmp.path());
        let entry = report.recording.entry();
        match fault {
            None => {
                assert_eq!(report.evidence.progress, ConflictProgress::Verified);
                let OpOutcome::Success { after } = &entry.outcome else {
                    panic!("{route:?}: expected Success, got {:?}", entry.outcome);
                };
                assert_eq!(*after, measured_state(tmp.path()));
            }
            Some(ConflictFaultPoint::AfterWorktreeWrite) => {
                assert_eq!(report.evidence.progress, ConflictProgress::WorktreeWritten);
                let OpOutcome::Partial { after, .. } = &entry.outcome else {
                    panic!(
                        "{route:?}: measured failed mutation is Partial: {:?}",
                        entry.outcome
                    );
                };
                assert_eq!(*after, measured_state(tmp.path()));
                if matches!(route, Route::Delete) {
                    assert!(Repository::open(tmp.path())
                        .unwrap()
                        .index()
                        .unwrap()
                        .has_conflicts());
                    assert!(after.dirty.contains("conflict"));
                    assert_ne!(after.head, "deleted from index and worktree");
                }
            }
            Some(
                ConflictFaultPoint::AfterStateUnreadable
                | ConflictFaultPoint::AfterWorktreeWriteUnreadable,
            ) => {
                assert_ne!(report.evidence.progress, ConflictProgress::NotStarted);
                let OpOutcome::Unknown { after, evidence } = &entry.outcome else {
                    panic!(
                        "{route:?}: unmeasurable mutated state is Unknown: {:?}",
                        entry.outcome
                    );
                };
                assert_eq!(after.dirty, "unknown");
                assert!(after.head.contains("unreadable"));
                assert!(evidence.contains("became unreadable"));
            }
            _ => unreachable!(),
        }
        let receipts: Vec<_> = kagi_git::oplog::read_oplog_tail(100)
            .into_iter()
            .filter(|record| record.repo == entry.repo)
            .collect();
        assert_eq!(receipts.len(), 1, "one terminal receipt for {route:?}");
        assert_eq!(receipts[0].id, entry.id);
        assert_eq!(
            std::mem::discriminant(&receipts[0].outcome),
            std::mem::discriminant(&entry.outcome),
        );
    }
}

#[test]
fn namespace_success_receipts_use_measured_repository_state() {
    if test_support::run_isolated() {
        check_receipt(None);
    }
}

#[test]
fn namespace_failure_after_worktree_mutation_records_measured_partial() {
    if test_support::run_isolated() {
        check_receipt(Some(ConflictFaultPoint::AfterWorktreeWrite));
    }
}

#[test]
fn namespace_completed_mutation_with_unreadable_state_records_unknown() {
    if test_support::run_isolated() {
        check_receipt(Some(ConflictFaultPoint::AfterStateUnreadable));
    }
}

#[test]
fn namespace_failed_mutation_with_unreadable_state_records_unknown() {
    if test_support::run_isolated() {
        check_receipt(Some(ConflictFaultPoint::AfterWorktreeWriteUnreadable));
    }
}
