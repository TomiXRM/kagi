use super::*;
use kagi_domain::{
    conflict_family::ConflictProgress,
    plan_note::{ConflictsNote, PlanNote},
};
use kagi_git::{backend::ExecutionPolicy, oplog::OpOutcome, Backend};
use std::{collections::BTreeMap, path::PathBuf};

// Include Git metadata, objects, refs, raw index and every working-tree byte.
fn repository_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}

fn obstruct_draft_directory() {
    let log = PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    std::fs::create_dir_all(&log).unwrap();
    // A regular file in place of the required directory fails even as root.
    std::fs::write(log.join("conflicts"), b"not a directory").unwrap();
}

#[test]
fn abort_unwritable_draft_directory_is_refused_without_repository_changes() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let backend = Backend::open(tmp.path()).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let plan = Backend::plan_recorded_conflict(tmp.path(), request).unwrap();
    obstruct_draft_directory();
    let before = repository_bytes(tmp.path());
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), None);
    assert_eq!(report.evidence.progress, ConflictProgress::NotStarted);
    assert_eq!(
        report.blocker,
        Some(PlanNote::Conflicts(ConflictsNote::DraftPreservationFailed))
    );
    assert!(
        matches!(report.recording.entry().outcome, OpOutcome::Refused { .. }),
        "{report:?}"
    );
    assert_eq!(repository_bytes(tmp.path()), before);
}

#[test]
fn skip_unwritable_draft_directory_is_blocked_without_repository_changes() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = cherry_pick_conflict_repo();
    let backend = Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();
    let draft = buffer.resolved_text(Path::new("file.txt")).unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let plan = backend.plan_conflict_skip(&session, &expected).unwrap();
    assert!(plan.plan.blockers.is_empty());
    obstruct_draft_directory();
    let before = repository_bytes(tmp.path());
    let error = backend.execute_conflict_skip(&plan, &buffer).unwrap_err();
    assert_eq!(
        error.blocker(),
        Some(&PlanNote::Conflicts(ConflictsNote::DraftPreservationFailed))
    );
    assert_eq!(repository_bytes(tmp.path()), before);
    assert_eq!(buffer.resolved_text(Path::new("file.txt")).unwrap(), draft);
}
