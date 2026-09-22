//! Typed conflict refusals must survive planning and live preflight without mutation.
use super::*;
use kagi_domain::plan_note::{ConflictsNote, PlanNote};

#[test]
fn approved_abort_drift_keeps_the_typed_reason_and_repository() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    fixture.owner_and_snapshot(&mut sessions);
    let job = fixture.job(
        &mut sessions,
        Backend::conflict_abort_request(&fixture.in_progress()),
    );
    std::fs::write(fixture.repo.join("file.txt"), "external resolution\n").unwrap();
    assert!(git(&fixture.repo, &["add", "file.txt"]));
    let index = std::fs::read(fixture.repo.join(".git/index")).unwrap();
    let merge_head = std::fs::read(fixture.repo.join(".git/MERGE_HEAD")).unwrap();
    let completion = job.run();
    let report = completion.report();
    assert!(matches!(
        report.blocker,
        Some(PlanNote::Conflicts(ConflictsNote::PlanChanged))
    ));
    assert!(
        matches!(&report.recording.entry().outcome, OpOutcome::Refused { blockers }
        if blockers.iter().any(|reason| reason.contains("conflict changed since planning")))
    );
    assert_eq!(report.evidence.progress, ConflictProgress::NotStarted);
    sessions.apply(completion);
    assert!(!sessions.has_leases());
    assert_eq!(
        std::fs::read(fixture.repo.join(".git/index")).unwrap(),
        index
    );
    assert_eq!(
        std::fs::read(fixture.repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("file.txt")).unwrap(),
        "external resolution\n"
    );
}
#[test]
fn marker_draft_refusal_preserves_the_session_suffix_in_the_recording() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let (owner, snapshot) = fixture.owner_and_snapshot(&mut sessions);
    let backend = Backend::open(&fixture.repo).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let markers = backend
        .materialized_markers(&buffer, Path::new("file.txt"))
        .expect("text conflict markers");
    assert!(buffer.ensure_hunks(Path::new("file.txt"), &markers));
    assert!(buffer.reset_hunk(Path::new("file.txt"), 0));
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("file.txt"),
        snapshot.observation.kind,
        "",
    )
    .unwrap();
    assert!(matches!(
        &request,
        ConflictRequest::Save {
            draft: ConflictDraft::Text(bytes),
            ..
        } if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
    ));
    let owner = sessions.attachment(owner).unwrap();
    let completion = plan_conflict(
        &mut sessions,
        ConflictAppRequest { owner, request },
        ExecutionPolicy::human(false),
    )
    .run();
    assert!(apply_plan(&mut sessions, completion));
    let PlanState::Error {
        recording: Some(recording),
        blocker: Some(PlanNote::Conflicts(ConflictsNote::ResolutionMarkers)),
        ..
    } = sessions.plan_state()
    else {
        panic!("marker draft must be refused during planning")
    };
    assert_eq!(recording.entry().op, "conflict-save:merge");
}
#[test]
fn an_abort_planned_against_a_stale_revision_is_refused_without_mutation() {
    let fixture = Fixture::content();
    fixture.resolve_and_stage();
    let mut sessions = Sessions::new();
    let (owner, _snapshot) = fixture.owner_and_snapshot(&mut sessions);
    // The user opens the confirmation…
    let frozen = Backend::conflict_abort_request(&fixture.in_progress());
    let frozen_for_backend = frozen.clone();
    // …and the repository moves under it before they confirm.
    std::fs::write(fixture.repo.join("other.txt"), "typed meanwhile\n").unwrap();
    assert!(git(&fixture.repo, &["add", "other.txt"]));

    let fresh = Backend::open(&fixture.repo)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .expect("still merging");
    sessions.observe_conflict(owner, Some(fresh.observation));
    let owner = sessions.attachment(owner).unwrap();
    let plan = plan_conflict(
        &mut sessions,
        ConflictAppRequest {
            owner,
            request: frozen,
        },
        ExecutionPolicy::human(false),
    );
    assert!(apply_plan(&mut sessions, plan.run()));
    assert!(
        matches!(
            sessions.plan_state(),
            PlanState::Error {
                blocker: Some(PlanNote::Conflicts(ConflictsNote::ObservationChanged)),
                ..
            }
        ),
        "a frozen revision that no longer describes the repository is refused"
    );
    assert!(!sessions.has_leases(), "a refused plan admits no write");
    assert!(
        fixture.repo.join(".git/MERGE_HEAD").exists(),
        "the refusal mutated nothing"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("other.txt")).unwrap(),
        "typed meanwhile\n"
    );
    // Belt to that brace: the Backend refuses the frozen request on its own,
    // so a caller that skipped the application boundary is refused too.
    assert!(
        Backend::plan_recorded_conflict(&fixture.repo, frozen_for_backend).is_err(),
        "the Backend re-reads the live revision and refuses a stale abort"
    );
    assert!(fixture.repo.join(".git/MERGE_HEAD").exists());
}
