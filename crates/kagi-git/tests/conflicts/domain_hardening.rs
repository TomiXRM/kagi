use super::*;
use kagi_domain::resolution::HunkChoice;
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

fn source_fixture(current: &str, base: &str, incoming: &str) -> (tempfile::TempDir, Repository) {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let mut index = repo.index().unwrap();
    for (stage, text) in [(1, base), (2, current), (3, incoming)] {
        let mut entry = index.get_path(Path::new("file.txt"), stage).unwrap();
        entry.id = repo.blob(text.as_bytes()).unwrap();
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    (tmp, repo)
}

fn assert_unverified_incoming_is_blocked(current: &str, incoming: &str, expect_empty: bool) {
    let (tmp, repo) = source_fixture(current, "a\n", incoming);
    let path = Path::new("file.txt");
    let before_file = std::fs::read(tmp.path().join(path)).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    assert!(buffer.ensure_hunks(path, &markers));
    assert!(buffer.hunk_count(path) > 0);
    if expect_empty {
        assert!(
            buffer
                .hunk_model(path)
                .unwrap()
                .hunks()
                .iter()
                .all(|hunk| hunk.incoming.is_empty()),
            "exercise a corrupt parse whose chosen side would emit no marker"
        );
    }
    for hunk in 0..buffer.hunk_count(path) {
        assert!(buffer.apply_hunk_choice(path, hunk, HunkChoice::AcceptIncoming));
    }
    assert!(buffer.cached_marker_residue(path), "readiness must block");
    assert_eq!(
        buffer.resolved_text(path).as_deref(),
        Some(markers.as_str())
    );
    assert!(buffer.has_marker_residue(path));
    assert_eq!(buffer.files_with_marker_residue(), vec![path.to_path_buf()]);
    let mut refreshed = ResolutionBuffer::from_repo(&repo).unwrap();
    refreshed.retain_drafts(buffer);
    assert!(refreshed.hunk_model(path).is_some());
    assert!(refreshed.apply_hunk_choice(path, 0, HunkChoice::AcceptIncoming));
    assert_eq!(
        refreshed.resolved_text(path).as_deref(),
        Some(markers.as_str())
    );
    refreshed.autosave().unwrap();
    let mut buffer = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(buffer.ensure_hunks(path, &markers));
    assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptIncoming));
    assert_eq!(
        buffer.resolved_text(path).as_deref(),
        Some(markers.as_str())
    );
    assert!(buffer.cached_marker_residue(path));
    assert!(buffer.has_marker_residue(path));
    let session = detect_conflict_session(&repo).unwrap();
    assert!(!continue_blockers(&repo, &session, &buffer)
        .unwrap()
        .is_empty());
    let direct_error = execute_conflict_save(&repo, &buffer, path).unwrap_err();
    assert!(direct_error.to_string().contains("conflict markers"));
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
    match &request {
        kagi_domain::conflict_family::ConflictRequest::Save {
            draft: kagi_domain::conflict_family::ConflictDraft::Text { bytes, origins },
            ..
        } => {
            assert_eq!(bytes, markers.as_bytes());
            assert!(origins.iter().all(|origin| *origin == LineOrigin::Manual));
        }
        _ => panic!("expected a text Save request"),
    }
    let recorded_error = Backend::plan_recorded_conflict(tmp.path(), request)
        .err()
        .expect("unverified attribution must not authorize recorded Save");
    assert!(matches!(
        recorded_error,
        kagi_git::GitError::Blocked(note)
            if matches!(
                *note,
                kagi_domain::plan_note::PlanNote::Conflicts(
                    kagi_domain::plan_note::ConflictsNote::ResolutionMarkers
                )
            )
    ));
    assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), before_file);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
    buffer
        .apply_choice(path, ResolutionChoice::Incoming)
        .unwrap();
    assert_eq!(buffer.resolved_text(path).as_deref(), Some(incoming));
    assert!(!buffer.cached_marker_residue(path));
    assert!(!buffer.has_marker_residue(path));
}

#[test]
fn corrupt_separator_attribution_blocks_readiness_continue_and_both_saves() {
    if !test_support::run_isolated() {
        return;
    }
    assert_unverified_incoming_is_blocked("b\n=======\nc\n", "d\n", false);
}

#[test]
fn corrupt_incoming_opener_with_empty_parsed_side_still_blocks() {
    if !test_support::run_isolated() {
        return;
    }
    assert_unverified_incoming_is_blocked("b\n", "<<<<<<< x\n", true);
}

#[test]
fn verified_hunk_literal_marker_saves_directly_and_recorded() {
    if !test_support::run_isolated() {
        return;
    }
    for recorded in [false, true] {
        let literal = "b\n>>>>>>> literal\nc\n";
        let (tmp, repo) = source_fixture(literal, "a\n", "d\n");
        let path = Path::new("file.txt");
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        let markers = buffer.materialized_markers(&repo, path).unwrap();
        assert!(buffer.ensure_hunks(path, &markers));
        assert!(buffer.hunk_count(path) > 0);
        for hunk in 0..buffer.hunk_count(path) {
            assert!(buffer.apply_hunk_choice(path, hunk, HunkChoice::AcceptCurrent));
        }
        assert!(buffer.has_resolution(path) && !buffer.cached_marker_residue(path));
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
        assert_eq!(
            index.get_path(path, 0).unwrap().id,
            repo.blob(literal.as_bytes()).unwrap()
        );
    }
}
