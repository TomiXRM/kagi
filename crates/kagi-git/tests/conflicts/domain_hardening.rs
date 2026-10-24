use super::*;
use kagi_git::Backend;

#[test]
fn chosen_literal_marker_is_ready_and_saves_directly_and_recorded() {
    if !test_support::run_isolated() {
        return;
    }
    for recorded in [false, true] {
        let tmp = merge_conflict_repo();
        let repo = Repository::open(tmp.path()).unwrap();
        let path = Path::new("file.txt");
        let literal = "<<<<<<< user\n=======\n>>>>>>> user\n";
        let oid = repo.blob(literal.as_bytes()).unwrap();
        let mut index = repo.index().unwrap();
        let mut entry = index.get_path(path, 2).unwrap();
        entry.id = oid;
        index.add(&entry).unwrap();
        index.write().unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .apply_choice(path, ResolutionChoice::Current)
            .unwrap();
        assert!(
            buffer.has_resolution(path) && !buffer.cached_marker_residue(path),
            "draft readiness must accept literal source markers"
        );
        assert!(!buffer.has_marker_residue(path));
        assert!(buffer.files_with_marker_residue().is_empty());
        let session = detect_conflict_session(&repo).unwrap();
        assert!(continue_blockers(&repo, &session, &buffer)
            .unwrap()
            .is_empty());
        if recorded {
            let snapshot = Backend::open(tmp.path())
                .unwrap()
                .conflict_snapshot()
                .unwrap()
                .unwrap();
            let request = Backend::conflict_save_request(
                snapshot.observation.revision,
                &buffer,
                path,
                snapshot.observation.kind,
                "",
            )
            .unwrap();
            let mut tampered = request.clone();
            if let kagi_domain::conflict_family::ConflictRequest::Save {
                draft: kagi_domain::conflict_family::ConflictDraft::Text { origins, .. },
                ..
            } = &mut tampered
            {
                origins.fill(LineOrigin::Manual);
            }
            assert!(
                Backend::plan_recorded_conflict(tmp.path(), tampered).is_err(),
                "provenance-only tampering must invalidate the payload binding"
            );
            let plan = Backend::plan_recorded_conflict(tmp.path(), request).unwrap();
            let report = Backend::run_recorded_conflict(
                &plan,
                kagi_git::backend::ExecutionPolicy::human(false),
                None,
            );
            assert_eq!(
                report.evidence.progress,
                kagi_domain::conflict_family::ConflictProgress::Verified
            );
        } else {
            execute_conflict_save(&repo, &buffer, path).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(path)).unwrap(),
            literal
        );
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert!(!index.has_conflicts());
        assert_eq!(index.get_path(path, 0).unwrap().id, oid);
    }
}
