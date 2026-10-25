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

struct HunkCase<'a> {
    current: &'a str,
    base: &'a str,
    incoming: &'a str,
    choices: &'a [HunkChoice],
    expected: &'a str,
    width: usize,
}

fn assert_hunk_save(case: &HunkCase<'_>, recorded: bool) {
    let (tmp, repo) = source_fixture(case.current, case.base, case.incoming);
    let path = Path::new("file.txt");
    let before_file = std::fs::read(tmp.path().join(path)).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    assert_eq!(markers.marker_width, case.width);
    assert!(buffer.ensure_hunks(path, &markers.text, markers.marker_width));
    assert_eq!(buffer.hunk_count(path), case.choices.len());
    for (hunk, choice) in case.choices.iter().enumerate() {
        assert!(buffer.apply_hunk_choice(path, hunk, choice.clone()));
        if hunk + 1 < case.choices.len() {
            assert!(
                buffer.cached_marker_residue(path),
                "unresolved hunks still block"
            );
        }
    }
    assert_eq!(buffer.resolved_text(path).as_deref(), Some(case.expected));
    assert!(buffer.has_resolution(path) && !buffer.cached_marker_residue(path));
    assert!(!buffer.has_marker_residue(path));
    let mut refreshed = ResolutionBuffer::from_repo(&repo).unwrap();
    refreshed.retain_drafts(buffer);
    assert!(refreshed.hunk_model(path).is_some());
    refreshed.autosave().unwrap();
    let buffer = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(buffer.hunks_all_resolved(path));
    assert_eq!(buffer.resolved_text(path).as_deref(), Some(case.expected));
    assert!(!buffer.has_marker_residue(path));
    assert!(buffer.files_with_marker_residue().is_empty());
    assert_eq!(buffer.recovery_drafts().count(), 0);
    let session = detect_conflict_session(&repo).unwrap();
    assert!(continue_blockers(&repo, &session, &buffer)
        .unwrap()
        .is_empty());
    assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), before_file);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
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
        std::fs::read(tmp.path().join(path)).unwrap(),
        case.expected.as_bytes()
    );
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    assert!(!index.has_conflicts());
    assert_eq!(
        index.get_path(path, 0).unwrap().id,
        repo.blob(case.expected.as_bytes()).unwrap()
    );
}

#[test]
fn crlf_materialization_round_trips_choices_through_save() {
    if !test_support::run_isolated() {
        return;
    }
    for recorded in [false, true] {
        for incoming in ["incoming\r\n", "incoming\n"] {
            for (choice, body) in [
                (HunkChoice::AcceptCurrent, "current\r\n".to_string()),
                (HunkChoice::AcceptIncoming, incoming.to_string()),
                (
                    HunkChoice::BothCurrentFirst,
                    format!("current\r\n{incoming}"),
                ),
                (
                    HunkChoice::BothIncomingFirst,
                    format!("{incoming}current\r\n"),
                ),
                (HunkChoice::Manual("manual\r\n".into()), "manual\r\n".into()),
            ] {
                assert_hunk_save(
                    &HunkCase {
                        current: "before\r\ncurrent\r\nafter\r\n",
                        base: "before\r\nbase\r\nafter\r\n",
                        incoming: &format!("before\r\n{incoming}after\r\n"),
                        choices: &[choice],
                        expected: &format!("before\r\n{body}after\r\n"),
                        width: 7,
                    },
                    recorded,
                );
            }
        }
    }
}

#[test]
fn literal_separator_source_is_content_in_both_hunk_save_paths() {
    if !test_support::run_isolated() {
        return;
    }
    let current = "b\n=======\nc\n";
    for recorded in [false, true] {
        for (choice, expected) in [
            (HunkChoice::AcceptCurrent, current),
            (HunkChoice::AcceptIncoming, "d\n"),
        ] {
            assert_hunk_save(
                &HunkCase {
                    current,
                    base: "a\n",
                    incoming: "d\n",
                    choices: &[choice],
                    expected,
                    width: 8,
                },
                recorded,
            );
        }
    }
}

#[test]
fn literal_incoming_opener_is_content_in_both_hunk_save_paths() {
    if !test_support::run_isolated() {
        return;
    }
    let incoming = "<<<<<<< x\n";
    for recorded in [false, true] {
        for (choice, expected) in [
            (HunkChoice::AcceptCurrent, "b\n"),
            (HunkChoice::AcceptIncoming, incoming),
        ] {
            assert_hunk_save(
                &HunkCase {
                    current: "b\n",
                    base: "a\n",
                    incoming,
                    choices: &[choice],
                    expected,
                    width: 8,
                },
                recorded,
            );
        }
    }
}

#[test]
fn literal_closer_source_is_content_in_both_hunk_save_paths() {
    if !test_support::run_isolated() {
        return;
    }
    let current = "b\n>>>>>>> literal\nc\n";
    for recorded in [false, true] {
        assert_hunk_save(
            &HunkCase {
                current,
                base: "a\n",
                incoming: "d\n",
                choices: &[HunkChoice::AcceptCurrent],
                expected: current,
                width: 8,
            },
            recorded,
        );
    }
}

#[test]
fn adjacent_changes_keep_libgit_hunk_grouping_and_save() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, repo) = source_fixture("a\nY\nb\n", "a\nX\nb\n", "a\nZ\nB\n");
    let path = Path::new("file.txt");
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    assert!(buffer.ensure_hunks(path, &markers.text, markers.marker_width));
    let hunks = buffer.hunk_model(path).unwrap().hunks();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].current, ["Y", "b"]);
    assert_eq!(hunks[0].incoming, ["Z", "B"]);
    for recorded in [false, true] {
        for (choice, expected) in [
            (HunkChoice::AcceptCurrent, "a\nY\nb\n"),
            (HunkChoice::AcceptIncoming, "a\nZ\nB\n"),
        ] {
            assert_hunk_save(
                &HunkCase {
                    current: "a\nY\nb\n",
                    base: "a\nX\nb\n",
                    incoming: "a\nZ\nB\n",
                    choices: &[choice],
                    expected,
                    width: 7,
                },
                recorded,
            );
        }
    }
}

#[test]
fn clean_changes_on_both_sides_survive_mixed_hunk_choices() {
    if !test_support::run_isolated() {
        return;
    }
    for recorded in [false, true] {
        assert_hunk_save(
            &HunkCase {
                base: "left-base\nc1\nc2\nc3\nX\nm1\nm2\nm3\nQ\nr1\nr2\nr3\nright-base\n",
                current: "left-current\nc1\nc2\nc3\nY\nm1\nm2\nm3\nR\nr1\nr2\nr3\nright-base\n",
                incoming: "left-base\nc1\nc2\nc3\nZ\nm1\nm2\nm3\nS\nr1\nr2\nr3\nright-incoming\n",
                choices: &[HunkChoice::AcceptCurrent, HunkChoice::AcceptIncoming],
                expected:
                    "left-current\nc1\nc2\nc3\nY\nm1\nm2\nm3\nS\nr1\nr2\nr3\nright-incoming\n",
                width: 7,
            },
            recorded,
        );
    }
}

#[test]
fn legacy_width_draft_is_preserved_as_recovery_not_live_resolution() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, repo) = source_fixture("b\n=======\nc\n", "a\n", "d\n");
    let path = Path::new("file.txt");
    let before_file = std::fs::read(tmp.path().join(path)).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let mut legacy = ResolutionBuffer::from_repo(&repo).unwrap();
    let old_markers =
        "<<<<<<< Current\nb\n=======\nc\n||||||| Base\na\n=======\nd\n>>>>>>> Incoming\n";
    assert!(legacy.ensure_hunks(path, old_markers, 7));
    let body = "legacy draft 日本語\n";
    legacy.set_manual_text(path, body).unwrap();
    legacy.autosave().unwrap();
    let fresh = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    assert!(!fresh.has_resolution(path));
    assert!(!fresh.hunks_all_resolved(path));
    let recovery: Vec<_> = fresh.recovery_drafts().collect();
    assert_eq!(recovery.len(), 1);
    assert_eq!(recovery[0].path, path);
    assert_eq!(
        kagi_domain::resolution::lines_to_text(recovery[0].result.unwrap()),
        body
    );
    let session = detect_conflict_session(&repo).unwrap();
    assert!(!continue_blockers(&repo, &session, &fresh)
        .unwrap()
        .is_empty());
    assert!(execute_conflict_save(&repo, &fresh, path).is_err());
    assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), before_file);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
}

#[test]
fn materialization_recovery_never_readopts_after_retention_or_reload() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, repo) = source_fixture("b\n=======\nc\n", "a\n", "d\n");
    let path = Path::new("file.txt");
    let before_file = std::fs::read(tmp.path().join(path)).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let mut legacy = ResolutionBuffer::from_repo(&repo).unwrap();
    let identity = legacy.file_identity(path).unwrap().clone();
    let old_markers =
        "<<<<<<< Current\nb\n=======\nc\n||||||| Base\na\n=======\nd\n>>>>>>> Incoming\n";
    assert!(legacy.ensure_hunks(path, old_markers, 7));
    let body = "non-readoptable legacy draft 日本語\n";
    legacy.set_manual_text(path, body).unwrap();
    legacy.autosave().unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let mut previous = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    for _ in 0..3 {
        let recovery: Vec<_> = previous.recovery_drafts().collect();
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].identity, Some(&identity));
        assert_eq!(
            kagi_domain::resolution::lines_to_text(recovery[0].result.unwrap()),
            body
        );
        let mut fresh = ResolutionBuffer::from_repo(&repo).unwrap();
        fresh.retain_drafts(previous);
        assert!(
            !fresh.has_resolution(path),
            "retention cannot adopt a recovery"
        );
        fresh.revalidate_hunk_states();
        assert!(!fresh.has_resolution(path));
        assert_eq!(fresh.recovery_drafts().count(), 1);
        assert!(!continue_blockers(&repo, &session, &fresh)
            .unwrap()
            .is_empty());
        assert!(execute_conflict_save(&repo, &fresh, path).is_err());
        fresh.autosave().unwrap();
        previous = ResolutionBuffer::from_repo_with_autosave(&repo).unwrap();
    }
    assert!(!previous.has_resolution(path));
    assert_eq!(previous.recovery_drafts().count(), 1);
    assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), before_file);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
}

#[test]
fn retained_hunk_state_revalidates_even_when_a_model_was_cached() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, repo) = source_fixture("b\n=======\nc\n", "a\n", "d\n");
    let path = Path::new("file.txt");
    let mut previous = ResolutionBuffer::from_repo(&repo).unwrap();
    let old_markers =
        "<<<<<<< Current\nb\n=======\nc\n||||||| Base\na\n=======\nd\n>>>>>>> Incoming\n";
    assert!(previous.ensure_hunks(path, old_markers, 7));
    let body = "cached stale hunk draft 日本語\n";
    previous.set_manual_text(path, body).unwrap();
    let mut fresh = ResolutionBuffer::from_repo(&repo).unwrap();
    fresh.retain_drafts(previous);
    assert!(
        fresh.hunk_model(path).is_some(),
        "exercise cached-model bypass"
    );
    fresh.revalidate_hunk_states();
    assert!(!fresh.has_resolution(path));
    assert!(!fresh.hunks_all_resolved(path));
    let recovery: Vec<_> = fresh.recovery_drafts().collect();
    assert_eq!(recovery.len(), 1);
    assert_eq!(
        kagi_domain::resolution::lines_to_text(recovery[0].result.unwrap()),
        body
    );
    assert!(recovery[0].identity.is_some(), "keep inspection context");
    assert!(fresh.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
    assert_eq!(
        fresh.resolved_text(path).as_deref(),
        Some("b\n=======\nc\n")
    );
    assert!(!fresh.has_marker_residue(path));
    execute_conflict_save(&repo, &fresh, path).unwrap();
    assert_eq!(fresh.recovery_drafts().count(), 1);
}

#[test]
fn unresolved_wide_hunk_blocks_continue_direct_and_recorded_save() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, repo) = source_fixture(
        "a\nY\n=========\nm1\nm2\nm3\nR\nz\n",
        "a\nX\nm1\nm2\nm3\nQ\nz\n",
        "a\nZ\nm1\nm2\nm3\nS\nz\n",
    );
    let path = Path::new("file.txt");
    let before_file = std::fs::read(tmp.path().join(path)).unwrap();
    let before_index = std::fs::read(repo.path().join("index")).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    assert_eq!(markers.marker_width, 10);
    assert!(buffer.ensure_hunks(path, &markers.text, markers.marker_width));
    assert_eq!(buffer.hunk_count(path), 2);
    assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
    assert!(!buffer.hunks_all_resolved(path));
    assert!(buffer.cached_marker_residue(path));
    assert!(buffer.has_marker_residue(path));
    let session = detect_conflict_session(&repo).unwrap();
    assert!(!continue_blockers(&repo, &session, &buffer)
        .unwrap()
        .is_empty());
    assert!(execute_conflict_save(&repo, &buffer, path)
        .unwrap_err()
        .to_string()
        .contains("conflict markers"));
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
    let error = Backend::plan_recorded_conflict(tmp.path(), request)
        .err()
        .unwrap();
    assert!(matches!(
        error, kagi_git::GitError::Blocked(note)
            if matches!(*note, kagi_domain::plan_note::PlanNote::Conflicts(
                kagi_domain::plan_note::ConflictsNote::ResolutionMarkers
            ))
    ));
    assert_eq!(std::fs::read(tmp.path().join(path)).unwrap(), before_file);
    assert_eq!(
        std::fs::read(repo.path().join("index")).unwrap(),
        before_index
    );
}
