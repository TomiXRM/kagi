//! #1721: a Result draft belongs to one conflict revision, not just its path.
use crate::app_conflict::{click_control, wait_idle};
use crate::macos::{mount, unmount};
use crate::repository_state::RepositoryState;
use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::input::InputState;
use kagi::ui::{conflict_view::ConflictView, e2e, KagiApp};
use kagi_domain::conflict_family::ConflictRevision;
use kagi_git::resolution::{lines_to_text, ResolutionBuffer};
use std::path::{Path, PathBuf};

#[path = "conflict_draft_cleared_handoff.rs"]
mod cleared_handoff;
#[path = "conflict_draft_completion.rs"]
mod completion;
#[path = "../support/git_fixture.rs"]
mod fixture_git;
#[path = "../support/oplog.rs"]
mod oplog;
#[path = "conflict_draft_revision_failure.rs"]
mod unrelated_failure;

pub(super) const FILE: &str = "file.txt";
pub(super) const FIRST: &str = "RESOLVED ONE\n";
const SECOND: &str = "DISTINCT SECOND RESULT\n";

pub(super) struct RebaseFixture {
    directory: tempfile::TempDir,
    first: git2::Oid,
    second: git2::Oid,
    main: git2::Oid,
}

impl RebaseFixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        fixture_git::init_repo(repo, "main");
        fixture_git::write_file(repo, FILE, "base\n");
        fixture_git::commit_all(repo, "base");
        fixture_git::git(repo, &["checkout", "-qb", "side"]);
        fixture_git::write_file(repo, FILE, "SIDE ONE\n");
        fixture_git::commit_all(repo, "side step 1");
        let first = head(repo);
        fixture_git::write_file(repo, FILE, "SIDE TWO\n");
        fixture_git::commit_all(repo, "side step 2");
        let second = head(repo);
        fixture_git::git(repo, &["checkout", "-q", "main"]);
        fixture_git::write_file(repo, FILE, "MAIN\n");
        fixture_git::commit_all(repo, "main change");
        let main = head(repo);
        fixture_git::git(repo, &["checkout", "-q", "side"]);
        assert!(!fixture_git::git_succeeds(repo, &["rebase", "main"]));
        Self {
            directory,
            first,
            second,
            main,
        }
    }

    pub(super) fn path(&self) -> PathBuf {
        self.directory.path().canonicalize().unwrap()
    }
}

fn head(repo: &Path) -> git2::Oid {
    git2::Repository::open(repo)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
}

fn replay_head(repo: &Path) -> git2::Oid {
    git2::Repository::open(repo)
        .unwrap()
        .revparse_single("REBASE_HEAD")
        .unwrap()
        .id()
}

fn commit_body(repo: &Path, oid: git2::Oid) -> Vec<u8> {
    let repository = git2::Repository::open(repo).unwrap();
    let tree = repository.find_commit(oid).unwrap().tree().unwrap();
    let entry = tree.get_path(Path::new(FILE)).unwrap();
    let blob = repository.find_blob(entry.id()).unwrap();
    blob.content().to_vec()
}

fn assert_unmerged(repo: &Path) {
    let repository = git2::Repository::open(repo).unwrap();
    let index = repository.index().unwrap();
    assert!(index.get_path(Path::new(FILE), 0).is_none());
    for stage in 1..=3 {
        let entry = index
            .get_path(Path::new(FILE), stage)
            .expect("real unmerged stage");
        assert_eq!(entry.path, FILE.as_bytes());
        assert_eq!(entry.mode, 0o100644);
        assert!(repository.find_blob(entry.id).is_ok());
    }
    assert_eq!(
        fixture_git::git_output(repo, &["status", "--porcelain"]),
        "UU file.txt"
    );
}

pub(super) fn paint(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn conflict(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Entity<ConflictView> {
    cx.read(|cx| app.read(cx).ui().conflict.clone())
        .expect("live conflict editor")
}

pub(super) fn detect(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    // Accepted reloads re-arm detection and synchronously adopt its payload.
    // Calling detect_conflict_mode alone can return at its run-once guard.
    let (owner, builds) = app.update(cx, |app, cx| {
        let owner = app.active_session().unwrap();
        let builds = app.reads.counters().builds;
        app.reload(cx);
        (owner, builds)
    });
    crate::busy_dismiss::ready(cx, app);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_session(), Some(owner));
        assert!(
            state.reads.counters().builds > builds,
            "a fresh read was accepted"
        );
        assert!(!state.reads.is_loading(owner));
        assert!(
            state.ui().conflict_detected,
            "accepted reload re-detected conflicts"
        );
        let accepted = state.view().operation.as_ref().unwrap().revision();
        let projected = &conflict_read(app, cx).mode.as_ref().unwrap().revision;
        assert_eq!(
            projected, accepted,
            "detected payload belongs to the accepted read"
        );
    });
    paint(cx, app, window);
}

pub(super) fn result_input(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Entity<InputState> {
    conflict(cx, app).update(cx, |view, cx| {
        view.conflict_open_editor(Path::new(FILE));
        view.conflict_editor_set_result_mode(true);
        cx.notify();
    });
    paint(cx, app, window);
    cx.read(|cx| {
        conflict_read(app, cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    })
}

pub(super) fn conflict_read<'a>(app: &'a Entity<KagiApp>, cx: &'a gpui::App) -> &'a ConflictView {
    app.read(cx).ui().conflict.as_ref().unwrap().read(cx)
}

/// Deliver Change without draining tasks or painting after its callback returns.
fn emit_result_change(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    input: &Entity<InputState>,
    text: &str,
) {
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_input(window, text);
}

pub(super) fn replace_result(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    input: &Entity<InputState>,
    text: &str,
) {
    emit_result_change(cx, window, input, text);
    cx.run_until_parked();
    paint(cx, app, window);
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), text);
    cx.read(|cx| {
        let mode = conflict_read(app, cx).mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer.resolved_text(Path::new(FILE)).as_deref(),
            Some(text)
        );
        assert!(mode.can_continue());
    });
}

pub(super) fn assert_unresolved(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        let view = conflict_read(app, cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(mode.conflicted_count(), 1);
        assert_eq!(mode.resolved_count(), 0);
        assert!(!mode.buffer.has_resolution(Path::new(FILE)));
        assert_eq!(mode.buffer.resolved_text(Path::new(FILE)), None);
        assert!(!mode.can_continue());
        if let Some(inputs) = &view.editor_inputs {
            assert_ne!(inputs.result.read(cx).value().as_str(), FIRST);
        }
    });
}

pub(super) fn open_continue(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let repo = cx.read(|cx| app.read(cx).repo_path.clone()).unwrap();
    let before = RepositoryState::read(&repo);
    click_control(cx, window, "conflict-continue");
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .conflict_continue_modal()
            .expect("actual Continue opens confirmation");
        assert!(modal.plan.blockers.is_empty());
    });
    assert_eq!(
        before,
        RepositoryState::read(&repo),
        "Continue planning must not stage"
    );
}

fn confirm_continue(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    click_control(cx, window, "plan-confirm");
    wait_idle(cx, app);
    paint(cx, app, window);
    assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_none()));
}

fn stable_second_conflict(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
) {
    let before = RepositoryState::read(repo);
    let replay = replay_head(repo);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    for _ in 0..3 {
        paint(cx, app, window);
        assert_unresolved(cx, app);
        detect(cx, app, window);
        assert_unresolved(cx, app);
        // Continue remains visible but secondary and guarded while files remain.
        e2e::clear_control_bounds(window.window_id(), "conflict-continue");
        paint(cx, app, window);
        assert!(e2e::control_bounds(window.window_id(), "conflict-continue").is_some());
        assert_eq!(
            kagi::ui::conflict_view::continue_primary_for_e2e(),
            Some(false)
        );
        click_control(cx, window, "conflict-continue");
        assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_none()));
        assert_eq!(before, RepositoryState::read(repo));
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(replay_head(repo), replay);
        assert_unmerged(repo);
    }
}

fn finish_second(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    first_replay: git2::Oid,
) {
    let before = RepositoryState::read(repo);
    let input = result_input(cx, app, window);
    replace_result(cx, app, window, &input, SECOND);
    assert_eq!(
        before,
        RepositoryState::read(repo),
        "typing cannot stage the new edit"
    );
    // Save runs through another backend handle. Continue must then inspect the
    // real stage-0 index, not the original UI backend's cached unmerged entry.
    click_control(cx, window, "conflict-save");
    wait_idle(cx, app);
    crate::busy_dismiss::ready(cx, app);
    paint(cx, app, window);
    let saved = git2::Repository::open(repo).unwrap();
    let saved_index = saved.index().unwrap();
    assert!(!saved_index.has_conflicts());
    let staged = saved_index.get_path(Path::new(FILE), 0).unwrap();
    assert_eq!(
        saved.find_blob(staged.id).unwrap().content(),
        SECOND.as_bytes()
    );
    assert_eq!(
        head(repo),
        first_replay,
        "Save stages but does not continue"
    );
    assert!(cx.read(|cx| conflict_read(app, cx)
        .mode
        .as_ref()
        .unwrap()
        .session
        .files
        .is_empty()));
    open_continue(cx, app, window);
    confirm_continue(cx, app, window);
    let repository = git2::Repository::open(repo).unwrap();
    let final_commit = repository.find_commit(head(repo)).unwrap();
    assert_eq!(final_commit.parent_id(0).unwrap(), first_replay);
    assert_eq!(commit_body(repo, final_commit.id()), SECOND.as_bytes());
    assert_eq!(commit_body(repo, first_replay), FIRST.as_bytes());
    let index = repository.index().unwrap();
    assert!(!index.has_conflicts());
    let staged = index.get_path(Path::new(FILE), 0).unwrap();
    assert_eq!(
        repository.find_blob(staged.id).unwrap().content(),
        SECOND.as_bytes()
    );
    assert_eq!(std::fs::read(repo.join(FILE)).unwrap(), SECOND.as_bytes());
    assert_eq!(repository.state(), git2::RepositoryState::Clean);
    assert!(cx.read(|cx| app.read(cx).ui().conflict.is_none()));
    drop(input);
}

pub fn scenario_conflict_draft_cleared_handoff(cx: &mut VisualTestAppContext) {
    cleared_handoff::check_cleared_handoff(cx);
}

pub fn scenario_conflict_draft_stale_handoff(cx: &mut VisualTestAppContext) {
    cleared_handoff::check_stale_handoff(cx);
}

pub fn scenario_conflict_draft_revision_same_revision(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let before = RepositoryState::read(&repo);
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    let original = cx.read(|cx| input.read(cx).value().to_string());
    let file = Path::new(FILE);
    emit_result_change(cx, window, &input, FIRST);
    // DraftNeverLost / draft-crash: capture must commit synchronously. Neither
    // run_until_parked nor paint may be needed to make the returned edit durable.
    let saved = ResolutionBuffer::load(&repo)
        .expect("Change capture must autosave before returning, without deferred work");
    assert_eq!(saved.resolved_text(file).as_deref(), Some(FIRST));
    cx.run_until_parked();
    paint(cx, &app, window);
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), FIRST);
    cx.read(|cx| {
        let mode = conflict_read(&app, cx).mode.as_ref().unwrap();
        assert_eq!(mode.buffer.resolved_text(file).as_deref(), Some(FIRST));
        assert!(mode.can_continue());
    });
    input.update(cx, |input, cx| input.set_selected_range(2..8, cx));
    let revision = cx.read(|cx| {
        conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .revision
            .clone()
    });
    let mut history = cx.read(|cx| {
        conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .clone()
    });
    let edited = history.resolved_text(file);
    assert!(history.undo(file));
    let previous = history.resolved_text(file);
    assert_ne!(previous, edited);
    assert!(history.redo(file));
    assert_eq!(history.resolved_text(file), edited);
    for _ in 0..3 {
        detect(cx, &app, window);
        cx.read(|cx| {
            let view = conflict_read(&app, cx);
            assert_eq!(view.mode.as_ref().unwrap().revision, revision);
            assert_eq!(
                view.editor_inputs.as_ref().unwrap().result.entity_id(),
                input.entity_id()
            );
            assert_eq!(input.read(cx).value().as_str(), FIRST);
            assert_eq!(input.read(cx).selected_range(), 2..8);
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new(FILE))
                    .as_deref(),
                Some(FIRST)
            );
        });
    }
    // Inspect the buffer history before InputState undo adds any new edit.
    conflict(cx, &app).update(cx, |view, _| {
        let buffer = &mut view.mode.as_mut().unwrap().buffer;
        assert!(buffer.undo(file));
        assert_eq!(buffer.resolved_text(file), previous);
        assert!(buffer.redo(file));
        assert_eq!(buffer.resolved_text(file), edited);
    });
    cx.simulate_keystrokes(window, "cmd-z");
    paint(cx, &app, window);
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), original);
    cx.simulate_keystrokes(window, "cmd-shift-z");
    paint(cx, &app, window);
    assert!(cx.read(|cx| input.read(cx).value().as_str() == FIRST));
    assert_eq!(before, RepositoryState::read(&repo));
    drop(input);
    unmount(cx, app, window);
}

pub fn scenario_conflict_draft_revision_unrelated_index(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let file = Path::new(FILE);
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    let original = cx.read(|cx| input.read(cx).value().to_string());
    replace_result(cx, &app, window, &input, FIRST);
    input.update(cx, |input, cx| input.set_selected_range(2..8, cx));
    let (revision, identity, mut history) = cx.read(|cx| {
        let mode = conflict_read(&app, cx).mode.as_ref().unwrap();
        (
            mode.revision.clone(),
            mode.buffer.file_identity(file).unwrap().clone(),
            mode.buffer.clone(),
        )
    });
    let edited = history.resolved_text(file);
    assert!(history.undo(file));
    let previous = history.resolved_text(file);
    assert_ne!(previous, edited);
    let stages = || {
        let repository = git2::Repository::open(&repo).unwrap();
        let index = repository.index().unwrap();
        (1..=3)
            .map(|stage| {
                let entry = index.get_path(file, stage).unwrap();
                (entry.id, entry.mode, entry.path)
            })
            .collect::<Vec<_>>()
    };
    let original_stages = stages();
    // Only this unrelated stage-0 entry changes the global observation.
    fixture_git::write_file(&repo, "unrelated.txt", "unrelated staged content\n");
    fixture_git::git(&repo, &["add", "--", "unrelated.txt"]);
    assert_eq!(stages(), original_stages);
    assert_eq!(head(&repo), fixture.main);
    assert_eq!(replay_head(&repo), fixture.first);
    let before = RepositoryState::read(&repo);
    for _ in 0..3 {
        detect(cx, &app, window);
        cx.read(|cx| {
            let view = conflict_read(&app, cx);
            let mode = view.mode.as_ref().unwrap();
            let inputs = view.editor_inputs.as_ref().unwrap();
            assert_ne!(mode.revision, revision);
            assert_eq!(mode.buffer.file_identity(file), Some(&identity));
            assert_eq!(inputs.revision, mode.revision);
            assert_eq!(inputs.result.entity_id(), input.entity_id());
            assert_eq!(input.read(cx).value().as_str(), FIRST);
            assert_eq!(input.read(cx).selected_range(), 2..8);
            assert_eq!(mode.buffer.resolved_text(file), edited);
            assert!(mode.can_continue());
        });
    }
    conflict(cx, &app).update(cx, |view, _| {
        let buffer = &mut view.mode.as_mut().unwrap().buffer;
        assert!(buffer.undo(file));
        assert_eq!(buffer.resolved_text(file), previous);
        assert!(buffer.redo(file));
        assert_eq!(buffer.resolved_text(file), edited);
    });
    cx.simulate_keystrokes(window, "cmd-z");
    paint(cx, &app, window);
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), original);
    cx.simulate_keystrokes(window, "cmd-shift-z");
    paint(cx, &app, window);
    assert!(cx.read(|cx| input.read(cx).value().as_str() == FIRST));
    assert_eq!(stages(), original_stages);
    assert_eq!(before, RepositoryState::read(&repo));
    drop(input);
    unmount(cx, app, window);
    unrelated_failure::check_unrelated_restore_failure(cx);
}

fn append_lock() -> std::fs::File {
    let path =
        PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("operations.jsonl.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .unwrap();
    lock.lock().unwrap();
    lock
}

fn assert_recovery(buffer: &ResolutionBuffer, identity: Option<&ConflictRevision>) {
    let drafts: Vec<_> = buffer
        .recovery_drafts()
        .filter(|draft| draft.path == Path::new(FILE))
        .collect();
    assert_eq!(
        drafts.len(),
        1,
        "one inspectable old draft, not a parallel live result"
    );
    assert_eq!(drafts[0].identity, identity);
    assert_eq!(drafts[0].result.map(lines_to_text).as_deref(), Some(FIRST));
    assert!(drafts[0].raw_result.is_none());
}

fn assert_second_stage(fixture: &RebaseFixture, first_replay: git2::Oid) {
    let repo = fixture.path();
    assert_eq!(replay_head(&repo), fixture.second);
    assert_ne!(first_replay, fixture.main);
    assert_eq!(head(&repo), first_replay);
    assert_eq!(commit_body(&repo, first_replay), FIRST.as_bytes());
    let repository = git2::Repository::open(&repo).unwrap();
    assert_eq!(
        repository
            .find_commit(first_replay)
            .unwrap()
            .parent_id(0)
            .unwrap(),
        fixture.main
    );
    let index = repository.index().unwrap();
    for (stage, expected) in [(1, "SIDE ONE\n"), (2, FIRST), (3, "SIDE TWO\n")] {
        let entry = index.get_path(Path::new(FILE), stage).unwrap();
        assert_eq!(
            repository.find_blob(entry.id).unwrap().content(),
            expected.as_bytes()
        );
    }
    assert_unmerged(&repo);
}

pub(super) fn advance_first(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    fixture: &RebaseFixture,
    recording_failure: bool,
) -> (git2::Oid, ConflictRevision) {
    let repo = fixture.path();
    assert_eq!(head(&repo), fixture.main);
    assert_eq!(replay_head(&repo), fixture.first);
    assert_unmerged(&repo);
    let input = result_input(cx, app, window);
    let before = RepositoryState::read(&repo);
    replace_result(cx, app, window, &input, FIRST);
    assert_eq!(before, RepositoryState::read(&repo));
    let revision = cx.read(|cx| {
        conflict_read(app, cx)
            .mode
            .as_ref()
            .unwrap()
            .revision
            .clone()
    });
    let identity = cx.read(|cx| {
        conflict_read(app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .file_identity(Path::new(FILE))
            .unwrap()
            .clone()
    });
    open_continue(cx, app, window);
    // Fail only the actual run's append, never plan or test setup.
    let lock = recording_failure.then(append_lock);
    completion::confirm_first_continue(cx, app, window, fixture, &identity);
    let first_replay = head(&repo);
    let next_input = result_input(cx, app, window);
    assert_ne!(next_input.entity_id(), input.entity_id());
    assert_second_stage(fixture, first_replay);
    assert_unresolved(cx, app);
    cx.read(|cx| {
        let mode = conflict_read(app, cx).mode.as_ref().unwrap();
        assert_ne!(mode.revision, revision);
        assert_ne!(mode.buffer.file_identity(Path::new(FILE)), Some(&identity));
        let inputs = conflict_read(app, cx).editor_inputs.as_ref().unwrap();
        assert_eq!(inputs.revision, mode.revision);
    });
    assert_eq!(
        oplog::records(&repo, "rebase-continue", 100, true).len(),
        usize::from(!recording_failure)
    );
    if recording_failure {
        let diagnostic = std::fs::TryLockError::WouldBlock.to_string();
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(state.app_notice().unwrap().message.contains(&diagnostic));
        });
    } else {
        assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
    }
    drop(lock);
    if recording_failure {
        click_control(cx, window, "app-notice-confirm");
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
    }
    drop(input);
    (first_replay, identity)
}

fn same_path_rebase(cx: &mut VisualTestAppContext, recording_failure: bool) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let (first_replay, identity) = advance_first(cx, &app, window, &fixture, recording_failure);
    stable_second_conflict(cx, &app, window, &repo);
    cx.read(|cx| {
        assert_recovery(
            &conflict_read(&app, cx).mode.as_ref().unwrap().buffer,
            Some(&identity),
        )
    });
    finish_second(cx, &app, window, &repo, first_replay);
    let records = oplog::records(&repo, "rebase-continue", 100, true);
    assert_eq!(records.len(), if recording_failure { 1 } else { 2 });
    unmount(cx, app, window);
}

pub fn scenario_conflict_draft_revision_audit(cx: &mut VisualTestAppContext) {
    same_path_rebase(cx, false);
}

pub fn scenario_conflict_draft_revision_append_failure(cx: &mut VisualTestAppContext) {
    same_path_rebase(cx, true);
}

pub fn scenario_conflict_draft_revision_saved_stale(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    replace_result(cx, &app, window, &input, FIRST);
    let saved_path = cx.read(|cx| {
        conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    let saved = std::fs::read(&saved_path).unwrap();
    drop(input);
    let (first_replay, identity) = advance_first(cx, &app, window, &fixture, false);
    unmount(cx, app, window);
    // Simulate restarting with the first step's persisted draft still on disk.
    std::fs::write(&saved_path, saved).unwrap();
    for _ in 0..2 {
        let before = RepositoryState::read(&repo);
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        let input = result_input(cx, &app, window);
        assert_unresolved(cx, &app);
        stable_second_conflict(cx, &app, window, &repo);
        cx.read(|cx| {
            let buffer = &conflict_read(&app, cx).mode.as_ref().unwrap().buffer;
            assert_recovery(buffer, Some(&identity));
            buffer.autosave().unwrap();
        });
        assert_recovery(&ResolutionBuffer::load(&repo).unwrap(), Some(&identity));
        crate::conflict_draft_inspection::inspect_saved_recovery(cx, &app, window, &repo, FIRST);
        assert_eq!(before, RepositoryState::read(&repo));
        drop(input);
        unmount(cx, app, window);
    }
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    assert_second_stage(&fixture, first_replay);
    finish_second(cx, &app, window, &repo, first_replay);
    unmount(cx, app, window);
}

pub fn scenario_conflict_draft_revision_saved_legacy(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let before = RepositoryState::read(&repo);
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    replace_result(cx, &app, window, &input, FIRST);
    let saved_path = cx.read(|cx| {
        conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    drop(input);
    unmount(cx, app, window);
    let mut saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&saved_path).unwrap()).unwrap();
    for file in saved["files"].as_array_mut().unwrap() {
        file.as_object_mut().unwrap().remove("identity");
    }
    std::fs::write(&saved_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    for _ in 0..2 {
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        let input = result_input(cx, &app, window);
        assert_unresolved(cx, &app);
        stable_second_conflict(cx, &app, window, &repo);
        cx.read(|cx| {
            let buffer = &conflict_read(&app, cx).mode.as_ref().unwrap().buffer;
            assert_recovery(buffer, None);
            buffer.autosave().unwrap();
        });
        assert_recovery(&ResolutionBuffer::load(&repo).unwrap(), None);
        crate::conflict_draft_inspection::inspect_saved_recovery(cx, &app, window, &repo, FIRST);
        assert_eq!(replay_head(&repo), fixture.first);
        assert_eq!(head(&repo), fixture.main);
        assert_eq!(before, RepositoryState::read(&repo));
        drop(input);
        unmount(cx, app, window);
    }
}
