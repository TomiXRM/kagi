//! Exercise the real UI completion before its foreground callback can adopt it.
use super::*;
use std::sync::atomic::Ordering;

// A failed assertion must also release the detached foreground task. Keep the
// shared deferred helper unchanged: other consumers intentionally control theirs.
struct CompletionRelease(Option<crate::evidence_support::Reply<()>>);

impl Drop for CompletionRelease {
    fn drop(&mut self) {
        KagiApp::clear_continue_completion_hold_for_e2e();
        if let Some(reply) = self.0.take() {
            reply.send(());
        }
    }
}

pub(super) fn confirm_first_continue(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    fixture: &RebaseFixture,
    identity: &ConflictRevision,
) {
    let repo = fixture.path();
    let (saved_path, revision, builds) = cx.read(|cx| {
        let state = app.read(cx);
        let mode = conflict_read(app, cx).mode.as_ref().unwrap();
        (
            mode.buffer.autosave().unwrap(),
            mode.revision.clone(),
            state.reads.counters().builds,
        )
    });
    let saved_bytes = std::fs::read(&saved_path).unwrap();
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    let release = CompletionRelease(Some(release));
    let reached = KagiApp::hold_next_continue_completion_for_e2e(hold);
    click_control(cx, window, "plan-confirm");
    crate::busy_dismiss::wait(
        cx,
        app,
        "real Continue reaches its completion boundary",
        |_| reached.load(Ordering::Acquire),
    );

    assert_second_stage(fixture, head(&repo));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.conflict_continue_modal().is_some());
        assert_eq!(state.reads.counters().builds, builds);
        let mode = conflict_read(app, cx).mode.as_ref().unwrap();
        assert_eq!(mode.revision, revision, "foreground adoption is still held");
        assert_eq!(mode.buffer.file_identity(Path::new(FILE)), Some(identity));
        assert_eq!(
            mode.buffer.resolved_text(Path::new(FILE)).as_deref(),
            Some(FIRST)
        );
    });
    // This is the regression oracle: the actual background success path has
    // returned, including any completion cleanup, but no callback can repair a
    // deleted document by retaining the old in-memory Result.
    assert_eq!(std::fs::read(&saved_path).unwrap(), saved_bytes);
    let durable = ResolutionBuffer::load(&repo).expect("first draft remains durable");
    assert_eq!(durable.file_identity(Path::new(FILE)), Some(identity));
    assert_eq!(
        durable.resolved_text(Path::new(FILE)).as_deref(),
        Some(FIRST)
    );
    assert!(durable.recovery_drafts().next().is_none());
    assert_reopened_second(&repo, identity);
    assert_eq!(std::fs::read(&saved_path).unwrap(), saved_bytes);

    // Continue through the normal foreground receipt/reload/adoption path.
    // advance_first retains its existing receipt assertions for both a successful
    // append and a real locked append; its caller completes the second Continue.
    drop(release);
    wait_idle(cx, app);
    paint(cx, app, window);
    assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_none()));
}

fn assert_reopened_second(repo: &Path, identity: &ConflictRevision) {
    let before = RepositoryState::read(repo);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let backend =
        kagi_git::Backend::open_with_policy(repo, kagi_git::backend::ExecutionPolicy::human(false))
            .unwrap();
    let session = backend.detect_conflict_session().expect("second conflict");
    let restored = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_ne!(restored.file_identity(Path::new(FILE)), Some(identity));
    assert!(!restored.has_resolution(Path::new(FILE)));
    assert_eq!(restored.resolved_text(Path::new(FILE)), None);
    assert_recovery(&restored, Some(identity));
    assert!(backend
        .continue_blockers(&session, &restored)
        .unwrap()
        .iter()
        .any(|blocker| blocker.code() == "unresolved-files"));
    assert_eq!(before, RepositoryState::read(repo));
    assert_eq!(index, std::fs::read(repo.join(".git/index")).unwrap());
}
