//! Focused C1 GUI adapters. Run each with its exact KAGI_GUI_E2E_ONLY filter.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Focusable, SharedString, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "conflict_refusal.rs"]
mod refusal;

#[path = "conflict_skip_race.rs"]
mod skip_race;

pub fn scenario_conflict_skip_step_race(cx: &mut VisualTestAppContext) {
    skip_race::run(cx);
}

pub fn scenario_conflict_continue_refusal_result_owner(cx: &mut VisualTestAppContext) {
    refusal::continue_result_owner(cx);
}

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_succeeds;

pub(crate) fn content_fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.name", "Kagi Test"]);
    git(repo, &["config", "user.email", "kagi@example.test"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(repo.join("file.txt"), "feature\n").unwrap();
    git(repo, &["commit", "-qam", "feature"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("file.txt"), "main\n").unwrap();
    git(repo, &["commit", "-qam", "main"]);
    assert!(!git_succeeds(repo, &["merge", "feature"]));
    fixture
}

fn dir_file_fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("base"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "file-side"]);
    std::fs::write(repo.join("thing"), "file side\n").unwrap();
    git(repo, &["add", "thing"]);
    git(repo, &["commit", "-qm", "file"]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["checkout", "-qb", "dir-side"]);
    std::fs::create_dir(repo.join("thing")).unwrap();
    std::fs::write(repo.join("thing/child"), "directory side\n").unwrap();
    git(repo, &["add", "thing/child"]);
    git(repo, &["commit", "-qm", "directory"]);
    git(repo, &["checkout", "-q", "file-side"]);
    let repository = git2::Repository::open(repo).unwrap();
    let other = repository
        .revparse_single("dir-side")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let annotated = repository.find_annotated_commit(other.id()).unwrap();
    repository.merge(&[&annotated], None, None).unwrap();
    drop(annotated);
    drop(other);
    drop(repository);
    fixture
}

pub(crate) fn click_control(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
    name: &str,
) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} control was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

pub(crate) fn wait_idle(cx: &mut VisualTestAppContext, app: &gpui::Entity<kagi::ui::KagiApp>) {
    crate::recovery_operations::wait_idle_with_timeout(
        cx,
        app,
        Duration::from_secs(15),
        "conflict adapter did not settle",
    );
}

pub fn scenario_conflict_save_boundary(cx: &mut VisualTestAppContext) {
    crate::recovery_diff_presentation::assert_conflict_binary_states(cx);
    refusal::save_and_abort(cx);

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("conflict view");
    let path = PathBuf::from("file.txt");
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(&path);
        view.conflict_editor_set_file_side(
            &path,
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        view.conflict_editor_set_result_mode(true);
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let input = cx
        .read(|cx| {
            conflict
                .read(cx)
                .editor_inputs
                .as_ref()
                .map(|inputs| inputs.result.clone())
        })
        .expect("result input");
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    let expected = cx.read(|cx| input.read(cx).value().to_string());
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    assert_eq!(std::fs::read_to_string(repo.join(&path)).unwrap(), expected);
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect();
    assert_eq!(entries.len(), 1, "Backend records accepted Save once");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict Save keystroke → control → exact bytes/index/receipt");
}

pub fn scenario_conflict_dir_file_boundary(cx: &mut VisualTestAppContext) {
    let fixture = dir_file_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("D/F conflict view");
    click_control(cx, window, "conflict-row-0");

    let guard = app.update(cx, |app, _| {
        app.app_sessions.write_lease(Path::new(&repo)).unwrap()
    });
    conflict.update(cx, |view, cx| {
        view.writer_busy = true;
        cx.notify();
    });
    click_control(cx, window, "conflict-keep-directory");
    cx.run_until_parked();
    assert!(read_oplog_tail_for_repo(&repo, 100)
        .iter()
        .all(|entry| !entry.op.starts_with("conflict-dir-file:")));
    guard.complete();
    conflict.update(cx, |view, cx| {
        view.writer_busy = false;
        cx.notify();
    });
    click_control(cx, window, "conflict-keep-directory");
    wait_idle(cx, &app);
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-dir-file:keep-directory")
        .collect();
    assert_eq!(entries.len(), 1, "Backend records accepted D/F once");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    let index = git2::Repository::open(&repo).unwrap().index().unwrap();
    assert!(index.get_path(Path::new("thing"), 0).is_none());
    assert!(index.get_path(Path::new("thing/child"), 0).is_some());
    drop(index);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict D/F disabled while leased → control → exact index/receipt");
}

/// The #704 dead end, built the way the GUI walks into it: a conflicting
/// merge, the resolution staged (what Continue does), then the commit panel's
/// Unstage and Discard all can leave MERGING with no unmerged entries.
/// The accepted observation still owns Continue and Abort in that state.
fn stuck_merging_fixture() -> tempfile::TempDir {
    let fixture = content_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("file.txt"), "resolved\n").unwrap();
    git(repo, &["add", "file.txt"]); // Continue stages the resolution
    git(repo, &["reset", "-q", "--", "file.txt"]); // Unstage
    git(repo, &["checkout", "--", "file.txt"]); // Discard all
    assert!(repo.join(".git/MERGE_HEAD").exists());
    fixture
}

pub fn scenario_operation_strip_startup(cx: &mut VisualTestAppContext) {
    let fixture = stuck_merging_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let hold = crate::conflict_draft_inspection::DetectionHold::new(cx, false);
    let (app, window) = mount(cx, &repo);
    hold.wait(cx, &app);
    // Admission arrives before presentation. Hold the real detector callback
    // to prove #704's fallback stays reachable during that startup interval.
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(cx.read(|cx| app.read(cx).ui().conflict.is_none()));
    assert!(
        cx.read(|cx| app.read(cx).view().operation.is_some()),
        "the read model knows the merge is still in progress"
    );
    assert!(e2e::control_bounds(window.window_id(), "operation-strip").is_some());
    assert!(e2e::control_bounds(window.window_id(), "operation-strip-abort").is_some());
    assert!(
        e2e::control_bounds(window.window_id(), "conflict-abort").is_none(),
        "an unmounted editor never duplicates the strip exit"
    );
    drop(hold);
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "operation-strip");
    e2e::clear_control_bounds(window.window_id(), "operation-strip-abort");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "#1755: the accepted startup read mounts the resolved-merge overview"
    );
    assert!(cx.read(|cx| app.read(cx).view().operation.is_some()));
    assert!(e2e::control_bounds(window.window_id(), "operation-strip").is_none());
    assert!(e2e::control_bounds(window.window_id(), "operation-strip-abort").is_none());
    assert!(e2e::control_bounds(window.window_id(), "conflict-session-header").is_some());
    assert!(e2e::control_bounds(window.window_id(), "conflict-abort").is_some());
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS operation_strip_startup: runtime verified fallback Abort before the detector callback, then sidebar session header after #1755 mounts ConflictView");
}

/// While the conflict sidebar is visible its session header owns Abort. Hiding
/// the sidebar exposes #704 instead; both dispatch the same confirmation.
/// Admission remains in the read model regardless of control placement.
pub fn scenario_operation_strip_hidden_in_editor(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "precondition: the conflict editor is mounted"
    );
    assert!(
        cx.read(|cx| app.read(cx).view().operation.is_some()),
        "admission is still the read model's operation, editor or no editor"
    );

    // Forget the last frame's bounds, so the next draw is what decides.
    let id = window.window_id();
    e2e::clear_control_bounds(id, "operation-strip");
    e2e::clear_control_bounds(id, "operation-strip-abort");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        e2e::control_bounds(id, "operation-strip").is_none(),
        "the strip is redundant while the editor is mounted"
    );
    assert!(
        e2e::control_bounds(id, "operation-strip-abort").is_none(),
        "so its Abort is not on screen either"
    );

    // …and the dashboard's Abort opens exactly the same confirmation.
    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "the dashboard Abort opens the plan confirmation"
    );
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "and the first stage mutates nothing"
    );
    app.update(cx, |app, _| app.cancel_conflict_abort());
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS strip hidden in the editor; dashboard Abort opens the same modal");
}

/// #704 review P1: the legacy conflict detector is keyed on the repository
/// path alone — no `SessionId`, no read revision — so a `Cleared` it observed
/// before a newer read can marshal back after that read was accepted. While it
/// was a writer of `Sessions`' conflict observation, that erased the revision
/// the strip was still showing and turned the next Abort into a
/// `StaleApproval`: the #704 dead end from the other end.
pub fn scenario_operation_strip_stale_detector(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e::ConflictDetectOutcome;

    let fixture = stuck_merging_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    // Match the neighboring mounted-editor scenario: finish the initial
    // detector before injecting obsolete results, rather than racing startup.
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "precondition: the staged overview is mounted before stale results land"
    );
    let observed = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.conflict_state(session).cloned())
        })
    };
    let before = observed(cx).expect("the accepted read gave the owner its observation");

    // A detector job that started before that read now lands.
    let owner = cx
        .read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.attachment(session))
        })
        .expect("the tab is attached");
    app.update(cx, |app, cx| {
        app.apply_conflict_detect(owner.clone(), ConflictDetectOutcome::Cleared, cx)
    });
    app.update(cx, |app, cx| {
        app.apply_conflict_detect(owner.clone(), ConflictDetectOutcome::OpenFailed(None), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        observed(cx),
        Some(before),
        "a stale detector result must not erase the accepted read's observation"
    );

    // …and the mounted sidebar's Abort still goes through, which is what
    // the erasure used to break.
    assert!(cx.read(|cx| app.read(cx).ui().conflict.is_some()));
    e2e::clear_control_bounds(window.window_id(), "operation-strip-abort");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(e2e::control_bounds(window.window_id(), "operation-strip-abort").is_none());
    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
    wait_idle(cx, &app);
    assert!(
        !repo.join(".git/MERGE_HEAD").exists(),
        "Abort admission survived the stale detector"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a stale conflict detector cannot revoke Abort (#704)");
}

/// #707 re-review P1: a detector payload read at one observation must never be
/// re-labelled with another's revision.
///
/// R1 is read while `file.txt` is still conflicted. The repository then moves
/// to R2 and that read is accepted. Landing R1 afterwards used to stamp R1's
/// session and resolution buffer with R2's revision — after which a Save froze
/// R1's draft under an identity that both `Sessions` and the Backend accept,
/// and wrote R1's resolution onto R2.
pub fn scenario_conflict_detect_no_revision_laundering(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    // R1: read while the conflict is live.
    let r1 = e2e::detect_payload_for_test(&repo, "main");
    let r1_revision = cx
        .read(|cx| app.read(cx).view().operation.clone())
        .map(|op| op.revision().clone())
        .expect("the mounted read observed the merge");

    // R2: the repository moves on and that read is accepted.
    std::fs::write(repo.join("file.txt"), "resolved\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    app.update(cx, |app, cx| app.reload(cx));
    wait_idle(cx, &app);
    cx.run_until_parked();
    let r2_revision = cx
        .read(|cx| app.read(cx).view().operation.clone())
        .map(|op| op.revision().clone())
        .expect("still merging");
    assert_ne!(r1_revision, r2_revision, "precondition: the state moved");

    // R1 lands late.
    let owner = cx
        .read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.attachment(session))
        })
        .expect("attached");
    app.update(cx, |app, cx| app.apply_conflict_detect(owner, r1, cx));
    cx.run_until_parked();

    let mode_revision = cx.read(|cx| {
        app.read(cx)
            .ui()
            .conflict
            .as_ref()
            .and_then(|view| view.read(cx).mode.as_ref().map(|m| m.revision.clone()))
    });
    assert_eq!(mode_revision.as_ref(), Some(&r2_revision));
    cx.read(|cx| {
        let view = app.read(cx).ui().conflict.as_ref().unwrap().read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert!(
            mode.session.files.is_empty(),
            "the accepted staged read must not regain R1's unmerged files"
        );
        assert!(
            !mode.buffer.has_resolution(Path::new("file.txt")),
            "a late R1 payload cannot supply a draft under R2's authority"
        );
    });
    let observed = cx.read(|cx| {
        let app = app.read(cx);
        app.active_session()
            .and_then(|session| app.app_sessions.conflict_state(session).cloned())
    });
    assert!(
        matches!(
            observed,
            Some(kagi::app::ConflictOwnerState::Observed(ref o)) if o.revision == r2_revision
        ),
        "the accepted read stays the authoritative observation: {observed:?}"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a stale detector payload keeps its own revision (#707)");
}

/// S5: a conflict detector completion may retain its frozen background owner,
/// but it must never construct a pane in the tab currently on screen.
pub fn scenario_conflict_background_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = content_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let fixture_b = build_fixture();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    let (owner_a, payload) = cx.read(|cx| {
        let state = app.read(cx);
        let owner = state
            .active_session()
            .and_then(|session| state.app_sessions.attachment(session))
            .expect("A attachment");
        (owner, e2e::detect_payload_for_test(&repo_a, "main"))
    });

    let owner_b = app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open clean B");
        state.active_session().expect("B owner")
    });
    cx.run_until_parked();
    app.update(cx, |state, _| {
        state.status_footer =
            kagi::ui::FooterStatus::Idle(SharedString::from("B conflict sentinel"));
    });
    app.update(cx, |state, cx| {
        state.apply_conflict_detect(owner_a.clone(), payload, cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_session(), Some(owner_b));
        assert!(
            state.ui().conflict.is_none(),
            "conflict-background-owner-isolation: A completion built B's pane",
        );
        assert!(
            state.ui[&owner_a.session].conflict.is_some(),
            "conflict-background-owner-retained: A completion did not land in A",
        );
        assert!(
            matches!(
                &state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == "B conflict sentinel"
            ),
            "conflict-background-footer-isolation: A completion changed B footer"
        );
        assert!(
            !e2e::active_modal_present(state),
            "conflict-background-modal-isolation: A completion opened a modal on B",
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_background_owner");
}

/// Returning to a retained conflict owner first invalidates its pane, then
/// re-detects against the repository. An external abort cannot leave the old
/// editor actionable.
pub fn scenario_conflict_revalidates_after_external_abort(cx: &mut VisualTestAppContext) {
    let fixture_a = content_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let fixture_b = build_fixture();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |state, cx| state.detect_conflict_mode(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().conflict.is_some()));
    let (old_visit, old_payload) = cx.read(|cx| {
        let state = app.read(cx);
        let owner = state
            .active_session()
            .and_then(|session| state.app_sessions.attachment(session))
            .expect("A attachment");
        (owner, e2e::detect_payload_for_test(&repo_a, "main"))
    });

    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open clean B");
    });
    cx.run_until_parked();
    git(&repo_a, &["merge", "--abort"]);
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    // #722 P2: the retained pane survives the switch — undo/redo, the selected
    // file/hunk and scroll all live in this entity — but it is *paused*: not
    // authoritative, and every mutation it offers is refused until the
    // activation read lands.
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "conflict-activation-retained: the pane was destroyed instead of paused",
    );
    assert!(
        cx.read(|cx| app.read(cx).ui().panes_revalidating()),
        "conflict-activation-paused: a stale pane stayed actionable before the read",
    );
    app.update(cx, |state, cx| {
        state.apply_conflict_detect(old_visit, old_payload, cx);
    });
    assert!(
        cx.read(|cx| app.read(cx).ui().panes_revalidating()),
        "conflict-old-visit-rejected: a departed completion re-enabled the stale pane",
    );
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.ui().conflict.is_none(),
            "conflict-external-abort: stale editor returned"
        );
        assert!(
            state.view().operation.is_none(),
            "conflict-external-abort: stale operation remained"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_revalidates_after_external_abort");
}

/// #707 re-review P1: a detector launched for one tab must not land on another,
/// even when the path is identical — a close and reopen of the same repository
/// is a new `SessionId` and a new visit.
pub fn scenario_conflict_detect_wrong_owner_is_dropped(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    let pane = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("accepted startup detection has settled");
    pane.update(cx, |view, _| {
        view.selected_hunk = 3;
        view.mode
            .as_mut()
            .unwrap()
            .buffer
            .apply_choice(Path::new("file.txt"), kagi_git::ResolutionChoice::Incoming)
            .unwrap();
    });
    let payload = e2e::detect_payload_for_test(&repo, "main");

    // A different visit cannot replace the pane installed by accepted startup.
    let stale_owner = cx
        .read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.attachment(session))
        })
        .map(|mut owner| {
            owner.visit += 1;
            owner
        })
        .expect("attached");

    assert!(cx.read(|cx| app.read(cx).ui().conflict_detected));
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").conflict_detected = true;
        app.apply_conflict_detect(stale_owner.clone(), payload, cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state.ui().conflict.as_ref().map(|view| view.entity_id()),
            Some(pane.entity_id()),
            "a payload from another visit must not replace this tab's conflict editor"
        );
        let view = pane.read(cx);
        assert_eq!(
            view.selected_hunk, 3,
            "wrong-owner results retain navigation"
        );
        assert!(
            view.mode
                .as_ref()
                .unwrap()
                .buffer
                .has_resolution(Path::new("file.txt")),
            "wrong-owner results retain the live resolution"
        );
    });
    assert!(
        !cx.read(|cx| app.read(cx).ui().conflict_detected),
        "a stale callback must re-arm only its frozen owner's detector guard"
    );
    // The unchanged-observation adoption path also preserves the pane above.
    // Distinguish rejection from that path by delivering the wrong visit
    // before the current visit has installed any pane at all.
    let payload = e2e::detect_payload_for_test(&repo, "main");
    app.update(cx, |app, cx| {
        let ui = app.ui_mut().expect("active session");
        ui.conflict = None;
        ui.conflict_detected = true;
        assert!(
            app.view().operation.is_some(),
            "the accepted read is still conflicted"
        );
        app.apply_conflict_detect(stale_owner, payload, cx);
        assert!(
            app.ui().conflict.is_none(),
            "a wrong-visit result must never create the current visit's pane"
        );
        assert!(
            !app.ui().conflict_detected,
            "a wrong-visit result re-arms without falling through to adoption"
        );
    });
    drop(pane);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a detector result for another owner is dropped (#707)");
}

/// #707 re-review P2: a stale "there is no conflict" answer must not tear down
/// the projection the accepted read still says exists.
///
/// Every stale detection outcome must preserve the accepted conflict projection.
/// A staged snapshot is not authoritative merely because its file list is empty.
pub fn scenario_conflict_detect_stale_clear_is_dropped(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e::ConflictDetectOutcome;

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let view = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("the live conflict has an editor");
    let revision = cx
        .read(|cx| {
            view.read(cx)
                .mode
                .as_ref()
                .map(|mode| mode.revision.clone())
        })
        .expect("with a revision");
    let owner = cx
        .read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.attachment(session))
        })
        .expect("attached");
    // Capture a real fully staged snapshot, then restore the accepted unmerged
    // state before yielding to the UI. This completion is now stale, just like
    // a background read overtaken by another index observation.
    let index_path = repo.join(".git/index");
    let file_path = repo.join("file.txt");
    let original_index = std::fs::read(&index_path).unwrap();
    let original_text = std::fs::read(&file_path).unwrap();
    std::fs::write(&file_path, "resolved\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    let staged = kagi::ui::KagiApp::detect_conflict_payload(&repo, None, None, "main".into());
    std::fs::write(&index_path, original_index).unwrap();
    std::fs::write(&file_path, original_text).unwrap();
    for stale in [
        ConflictDetectOutcome::Cleared,
        ConflictDetectOutcome::OpenFailed(None),
        staged,
    ] {
        app.update(cx, |app, cx| {
            e2e::await_conflict_revalidation(app);
            app.ui_mut().expect("active session").conflict_detected = true;
            app.apply_conflict_detect(owner.clone(), stale, cx);
        });
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).ui().conflict.is_some()),
            "a stale no-conflict answer must not drop the editor"
        );
        assert_eq!(
            cx.read(|cx| {
                app.read(cx).ui().conflict.as_ref().and_then(|view| {
                    view.read(cx)
                        .mode
                        .as_ref()
                        .map(|mode| mode.revision.clone())
                })
            }),
            Some(revision.clone()),
            "and must not replace what it shows"
        );
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().conflict.as_ref().map(|pane| pane.entity_id()),
                Some(view.entity_id()),
                "stale outcomes retain the exact live pane"
            );
            assert!(
                state.ui().conflict_detected,
                "stale outcomes do not re-arm a settled detector"
            );
            assert!(
                state.ui().panes_revalidating(),
                "stale outcomes cannot acknowledge the checked-read gate"
            );
        });
    }

    // Once the accepted read agrees, the same producer now installs a fresh
    // empty-session Staged page, not an old editable conflict buffer.
    std::fs::write(repo.join("file.txt"), "resolved\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    app.update(cx, |app, cx| app.reload(cx));
    wait_idle(cx, &app);
    cx.run_until_parked();
    cx.read(|cx| {
        let entity = app
            .read(cx)
            .ui()
            .conflict
            .as_ref()
            .expect("staged workspace");
        let view = entity.read(cx);
        assert!(
            view.editing.is_none(),
            "staged workspace has no editable draft"
        );
        let mode = view.mode.as_ref().unwrap();
        assert!(mode.session.files.is_empty());
        assert!(
            !mode.buffer.has_resolution(Path::new("file.txt")),
            "staged projection owns no old active resolution"
        );
        assert!(mode.can_continue());
    });
    drop(view);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a stale no-conflict outcome keeps the projection (#707)");
}

pub fn scenario_operation_strip_abort(cx: &mut VisualTestAppContext) {
    let fixture = stuck_merging_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "#1755: the accepted startup read mounts the resolved-merge overview"
    );
    eprintln!(
        "[gui-e2e] operation_strip_abort: runtime verified mounted ConflictView before CommitPanel"
    );
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    for name in ["cp-body", "cp-commit-action"] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    click_control(cx, window, "conflict-continue");
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.read(|cx| {
        let ui = app.read(cx).ui();
        assert!(ui.conflict_merge_pending);
        assert!(ui.commit_panel_open);
    });
    // The session may retain its entity; pending must remove its rendered body.
    e2e::clear_control_bounds(window.window_id(), "conflict-abort");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(e2e::control_bounds(window.window_id(), "conflict-abort").is_none());
    for name in ["cp-body", "cp-commit-action", "operation-strip-abort"] {
        assert!(
            e2e::control_bounds(window.window_id(), name).is_some(),
            "{name} is painted over the real CommitPanel"
        );
    }
    // ADR-0196 Wave 3: the abort is a write, so both its stages consult the one
    // gate. A plan in flight takes no lease, so only `planning` refuses it —
    // and nothing may reach the family while it does.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.planning = Some("merge-plan");
            app.status_footer =
                kagi::ui::FooterStatus::Idle("Awaiting Abort planning refusal".into());
            let owner = app
                .active_session()
                .and_then(|session| app.app_sessions.attachment(session))
                .expect("abort owner");
            app.open_conflict_abort_modal(owner, window, cx);
            app.confirm_conflict_abort(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    crate::busy_dismiss::wait(
        cx,
        &app,
        "Abort refusal after the flush while planning",
        |app| {
            matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(message)
            if message.as_ref() == kagi::ui::i18n::Msg::OpInProgress.t())
        },
    );
    cx.read(|cx| {
        assert_eq!(app.read(cx).planning, Some("merge-plan"));
        assert!(
            app.read(cx).conflict_abort_modal().is_none(),
            "a plan in flight refuses the completed Abort-open continuation (#283)"
        );
    });
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "and a refused abort mutates nothing"
    );
    app.update(cx, |app, _| app.planning = None);
    click_control(cx, window, "operation-strip-abort");
    cx.run_until_parked();
    crate::busy_dismiss::wait(
        cx,
        &app,
        "Abort plan opens after planning releases",
        |app| app.conflict_abort_modal().is_some(),
    );
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "the first click confirms nothing — it opens the plan"
    );
    assert!(repo.join(".git/MERGE_HEAD").exists(), "and mutates nothing");
    kagi::ui::dialog_a11y::clear_recorded_a11y();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let dialog =
        kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("abort confirmation drawn");
    assert_eq!(dialog.role, gpui::Role::AlertDialog);
    let recovery = cx.read(|cx| {
        kagi_ui_core::i18n::plan_recovery_text(
            app.read(cx)
                .conflict_abort_modal()
                .unwrap()
                .plan
                .recovery
                .as_ref(),
        )
    });
    assert_eq!(
        dialog.description.as_deref(),
        Some(
            format!(
                "{}\n{recovery}",
                kagi_ui_core::i18n::Msg::A11yDialogArmed.t()
            )
            .as_str()
        ),
        "the next Confirm aborts immediately, then recovery is announced"
    );
    app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
    wait_idle(cx, &app);
    assert!(
        !repo.join(".git/MERGE_HEAD").exists(),
        "the second stage ends the merge"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("file.txt")).unwrap(),
        "main\n"
    );
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "merge-abort")
        .collect();
    assert_eq!(entries.len(), 1, "the Backend records the abort once");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "the confirmation closes with the operation it confirmed"
    );
    // The actual abort action, not a copied localized sentence, owns this result.
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    let aborts: Vec<_> = receipts
        .iter()
        .filter(|entry| entry.op == "merge-abort")
        .collect();
    assert_eq!(aborts.len(), 1);
    assert!(matches!(aborts[0].outcome, OpOutcome::Success { .. }));
    assert!(!receipts
        .iter()
        .any(|entry| entry.op.starts_with("conflict-save:")));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.app_notice().is_none());
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].kind, kagi::ui::ToastKind::Success);
        assert!(!matches!(
            state.status_footer,
            kagi::ui::FooterStatus::Success(_)
        ));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS header Abort → confirm → MERGE_HEAD gone (#704)");
}

/// #722 P1-b: a Conflict pane's Abort and two-stage Skip are deferred to the
/// next tick (`spawn_in`). If the user switches tabs before the task runs, the
/// owner frozen at the click must keep it from opening a modal or planning
/// against the tab now on screen (ADR-0197 決定 5). Continue already carried the
/// owner; this proves Abort and Skip do too — the methods now take
/// `owner: Attachment`, so a callback that omits it cannot compile.
pub fn scenario_conflict_deferred_action_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = content_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let fixture_b = content_fixture();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let owner_a = cx.read(|cx| {
        let state = app.read(cx);
        state
            .active_session()
            .and_then(|session| state.app_sessions.attachment(session))
            .expect("A owner")
    });

    let owner_b = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.active_session().expect("B owner")
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "precondition: B is in conflict"
    );
    app.update(cx, |app, _| {
        app.status_footer = kagi::ui::FooterStatus::Idle(SharedString::from("B deferred sentinel"));
    });

    // A's deferred Abort lands after the switch to B.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_conflict_abort_modal(owner_a.clone(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_session(), Some(owner_b));
        assert!(
            state.conflict_abort_modal().is_none(),
            "abort-deferred-owner: A's Abort opened a modal on B",
        );
    });

    // A's deferred Skip lands after the switch to B.
    app.update(cx, |app, cx| app.conflict_skip(owner_a.clone(), cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            matches!(
                &state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == "B deferred sentinel"
            ),
            "skip-deferred-owner: A's Skip acted on B (footer changed)",
        );
        assert!(
            !kagi::ui::e2e::active_modal_present(state),
            "skip-deferred-owner: A's Skip opened a modal on B",
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_deferred_action_owner");
}

/// #722 P2: an activation whose observation is **unchanged** must change
/// nothing. The old code discarded the whole `ConflictView` on every
/// activation, and even the in-place re-detect swapped in a freshly read
/// `mode` — either way the `ResolutionBuffer`'s undo/redo stack, the selected
/// file/hunk and the scroll position were lost on a plain tab round trip,
/// though the repository had not moved. ADR-0197 決定 3 says retained state is
/// non-authoritative until revalidated, not that it must be destroyed.
pub fn scenario_conflict_pane_survives_activation(cx: &mut VisualTestAppContext) {
    let fixture_a = content_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let fixture_b = build_fixture();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |state, cx| state.detect_conflict_mode(cx));
    cx.run_until_parked();
    let pane = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("A conflict pane");
    let path = cx.read(|cx| {
        pane.read(cx).mode.as_ref().expect("A mode").session.files[0]
            .path
            .clone()
    });

    // Do real work in the editor: pick a side (which pushes an undo entry) and
    // move the hunk cursor off its default.
    pane.update(cx, |view, _| {
        view.conflict_open_editor(&path);
        view.editing = Some(path.clone());
        view.selected_hunk = 3;
        view.mode
            .as_mut()
            .expect("mode")
            .buffer
            .apply_choice(&path, kagi_git::ResolutionChoice::Incoming)
            .expect("apply a side choice");
    });
    assert!(
        cx.read(|cx| pane
            .read(cx)
            .mode
            .as_ref()
            .expect("mode")
            .buffer
            .has_resolution(&path)),
        "precondition: the buffer carries the applied resolution",
    );

    // A plain tab round trip. The repository is untouched throughout.
    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open clean B");
    });
    cx.run_until_parked();
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    cx.run_until_parked();

    let same_entity = cx.read(|cx| {
        app.read(cx)
            .ui()
            .conflict
            .as_ref()
            .map(|e| e.entity_id())
            .expect("A conflict pane after the round trip")
            == pane.entity_id()
    });
    assert!(
        same_entity,
        "conflict-unchanged-keeps-entity: an unchanged observation rebuilt the pane",
    );
    assert!(
        !cx.read(|cx| app.read(cx).ui().panes_revalidating()),
        "conflict-unchanged-reenables: the accepted read left the pane refused",
    );
    cx.read(|cx| {
        let view = pane.read(cx);
        assert_eq!(
            view.editing.as_deref(),
            Some(path.as_path()),
            "conflict-unchanged-keeps-selection: the edited file was reset",
        );
        assert_eq!(
            view.selected_hunk, 3,
            "conflict-unchanged-keeps-selection: the hunk cursor was reset",
        );
        assert!(
            view.mode
                .as_ref()
                .expect("mode")
                .buffer
                .has_resolution(&path),
            "conflict-unchanged-keeps-buffer: the applied resolution was discarded",
        );
    });
    // The undo stack is the part a fresh buffer could never carry.
    let undone = pane.update(cx, |view, _| {
        view.mode.as_mut().expect("mode").buffer.undo(&path)
    });
    assert!(
        undone,
        "conflict-unchanged-keeps-undo: the undo stack did not survive activation",
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_pane_survives_activation");
}

/// A stale open failure owns neither the retained pane nor its checked-read
/// gate. A fresh restore failure also retains the pane, but closes the gate
/// until a successful detection can acknowledge the accepted read.
pub fn scenario_conflict_awaited_detection_stale_settles(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e::ConflictDetectOutcome;

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui().conflict.is_some()),
        "precondition: pane"
    );
    let pane = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    let owner = cx
        .read(|cx| {
            let app = app.read(cx);
            app.active_session()
                .and_then(|session| app.app_sessions.attachment(session))
        })
        .expect("attached");

    for awaiting in [false, true] {
        let gate = app.update(cx, |app, cx| {
            if awaiting {
                e2e::await_conflict_revalidation(app);
            }
            let gate = app.ui().pane_revalidation;
            let detected = app.ui().conflict_detected;
            app.apply_conflict_detect(owner.clone(), ConflictDetectOutcome::OpenFailed(None), cx);
            assert_eq!(app.ui().conflict_detected, detected);
            gate
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().pane_revalidation,
                gate,
                "a stale failure leaves the gate unchanged"
            );
            assert_eq!(
                state.ui().conflict.as_ref().map(|view| view.entity_id()),
                Some(pane.entity_id()),
                "a stale failure cannot discard the retained pane"
            );
        });
    }
    // Produce a real restore failure for this accepted operation, not an
    // observation-free OpenFailed(None), which is necessarily stale here.
    use std::os::unix::fs::PermissionsExt;
    let store = kagi_git::Backend::open(&repo)
        .unwrap()
        .resolution_buffer_from_repo()
        .unwrap()
        .autosave()
        .unwrap();
    let permissions = std::fs::metadata(&store).unwrap().permissions();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
    let failure = e2e::detect_payload_for_test(&repo, "main");
    std::fs::set_permissions(&store, permissions).unwrap();
    assert!(matches!(
        failure,
        ConflictDetectOutcome::OpenFailed(Some(_))
    ));
    // First settle the artificial wait with an accepted success, then show
    // that the fresh failure independently closes an otherwise open gate.
    let success = e2e::detect_payload_for_test(&repo, "main");
    app.update(cx, |app, cx| {
        app.apply_conflict_detect(owner.clone(), success, cx);
        assert!(!app.ui().panes_revalidating());
        app.apply_conflict_detect(owner, failure, cx);
    });
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.ui().panes_revalidating(),
            "a fresh restore failure closes the gate"
        );
        assert!(
            state.ui().conflict_detected,
            "failed restore waits for explicit revalidation"
        );
        assert!(state.ui().conflict_restore_failed);
        assert_eq!(
            state.ui().conflict.as_ref().map(|view| view.entity_id()),
            Some(pane.entity_id()),
            "a fresh failure retains the live pane behind the closed gate"
        );
    });
    drop(pane);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_awaited_detection_stale_settles");
}

fn assert_panicked_conflict(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    repo: &Path,
    op: &str,
) {
    cx.run_until_parked();
    let receipts: Vec<_> = read_oplog_tail_for_repo(repo, 20)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect();
    assert_eq!(receipts.len(), 1, "one durable receipt for {op}");
    assert!(matches!(receipts[0].outcome, OpOutcome::Unknown { .. }));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.app_sessions.has_leases(),
            "sequencer lease must be retained"
        );
        assert_eq!(state.app_sessions.reconcile_ids().len(), 1);
        assert!(
            state.conflict_continue_modal().is_none(),
            "a terminal Unknown must not leave a consumed Confirm available"
        );
        assert!(
            state.app_notice().is_some(),
            "the production terminal callback must expose the required reconcile notice"
        );
    });
}

fn sequencer_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(repo.join("README.md"), "feature\n").unwrap();
    git(repo, &["commit", "-qam", "feature"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "main\n").unwrap();
    git(repo, &["commit", "-qam", "main"]);
    assert!(!git_succeeds(repo, &["cherry-pick", "feature"]));
    fixture
}

pub fn scenario_conflict_merge_continue_panic(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
            view.mode
                .as_mut()
                .unwrap()
                .buffer
                .apply_choice(Path::new("file.txt"), kagi_git::ResolutionChoice::Incoming)
                .unwrap();
        });
    });
    kagi::ui::KagiApp::panic_next_continue_merge_for_e2e();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    assert_panicked_conflict(cx, &app, &repo, "merge-continue");
    assert!(!cx.read(|cx| app.read(cx).ui().commit_panel_open));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_merge_continue_panic");
}

pub fn scenario_conflict_confirm_continue_panic(cx: &mut VisualTestAppContext) {
    let fixture = sequencer_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
            view.mode
                .as_mut()
                .unwrap()
                .buffer
                .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                .unwrap();
        });
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    crate::busy_dismiss::wait(cx, &app, "Continue plan after draft flush", |app| {
        app.conflict_continue_modal().is_some()
    });
    assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_some()));
    kagi::ui::KagiApp::panic_next_continue_confirm_for_e2e();
    app.update(cx, |app, cx| app.confirm_conflict_continue(cx));
    assert_panicked_conflict(cx, &app, &repo, "cherry-pick-continue");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_confirm_continue_panic");
}

/// A successful sequencer Continue still refreshes the current owner's view
/// when its confirmation modal has been dismissed before the worker settles.
pub fn scenario_conflict_confirm_dismissed_still_reloads(cx: &mut VisualTestAppContext) {
    let fixture = sequencer_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
            view.mode
                .as_mut()
                .unwrap()
                .buffer
                .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                .unwrap();
        });
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    crate::busy_dismiss::wait(cx, &app, "Continue plan after draft flush", |app| {
        app.conflict_continue_modal().is_some()
    });
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    kagi::ui::KagiApp::hold_next_continue_confirm_for_e2e(hold);
    app.update(cx, |app, cx| app.confirm_conflict_continue(cx));
    crate::busy_dismiss::wait(
        cx,
        &app,
        "Continue write admitted before dismissal",
        |app| app.write_busy_op.is_some(),
    );
    app.update(cx, |app, _| app.cancel_conflict_continue());
    release.send(());
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    assert!(
        !repo.join(".git/CHERRY_PICK_HEAD").exists(),
        "the sequencer must have continued"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.view().status_summary.conflict_count,
            0,
            "current view must be refreshed after a dismissed modal"
        );
        assert!(
            app.ui().conflict.is_none(),
            "old conflict pane must be cleared by redetection"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_confirm_dismissed_still_reloads");
}

/// A failed merge-stage write has one result with canonical Details. The
/// English legacy panel line is retained without opening or switching panels.
pub fn scenario_conflict_merge_failure_contract(cx: &mut VisualTestAppContext) {
    for departed in [false, true] {
        for recording_failure in [false, true] {
            let fixture = content_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let other = build_fixture();
            let other_repo = other.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
            if departed {
                app.update(cx, |app, cx| assert!(app.open_repository(other_repo, cx)));
                cx.run_until_parked();
                app.update(cx, |app, cx| app.switch_repo(0, cx));
                cx.run_until_parked();
            }
            app.update(cx, |app, cx| app.detect_conflict_mode(cx));
            cx.run_until_parked();
            app.update(cx, |app, cx| {
                app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                    view.mode
                        .as_mut()
                        .unwrap()
                        .buffer
                        .apply_choice(Path::new("file.txt"), kagi_git::ResolutionChoice::Incoming)
                        .unwrap();
                });
            });
            let before_footer = kagi_ui_core::klog::tail()
                .iter()
                .filter(|line| line.starts_with("[kagi] footer: merge-continue: failed — "))
                .count();
            let before_panel = kagi_ui_core::klog::tail()
                .iter()
                .filter(|line| line.as_str() == "[kagi] bottom-panel: open (Failed auto-open)")
                .count();
            let (hold, release) = crate::evidence_support::deferred::<()>(cx);
            kagi::ui::KagiApp::hold_next_continue_merge_for_e2e(hold);
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    let owner = app
                        .app_sessions
                        .attachment(app.active_session().unwrap())
                        .unwrap();
                    app.conflict_continue(owner, window, cx);
                });
            })
            .unwrap();
            cx.run_until_parked();
            if departed {
                app.update(cx, |app, cx| {
                    app.switch_repo(1, cx);
                });
            }
            cx.run_until_parked();
            let footer_before = cx.read(|cx| format!("{:?}", app.read(cx).status_footer));
            let panel_before = cx.read(|cx| {
                let state = app.read(cx);
                (state.bottom_panel_open, state.bottom_tab)
            });
            let recording_lock = recording_failure.then(|| {
                let path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                    .join("operations.jsonl.lock");
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                    .unwrap();
                lock.lock().unwrap();
                lock
            });
            std::fs::rename(repo.join(".git"), repo.join(".git-paused")).unwrap();
            release.send(());
            cx.run_until_parked();
            std::fs::rename(repo.join(".git-paused"), repo.join(".git")).unwrap();
            let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "merge-continue")
                .collect();
            assert_eq!(receipts.len(), usize::from(!recording_failure));
            if !recording_failure {
                assert!(matches!(receipts[0].outcome, OpOutcome::Failed { .. }));
                assert_eq!(
                    kagi_ui_core::klog::tail()
                        .iter()
                        .filter(|line| line.starts_with("[kagi] footer: merge-continue: failed — "))
                        .count(),
                    before_footer + 1,
                    "missing unchanged failed footer contract for departed={departed}"
                );
            }
            cx.read(|cx| {
                let state = app.read(cx);
                if departed {
                    assert_eq!(format!("{:?}", state.status_footer), footer_before);
                } else {
                    assert!(!matches!(
                        state.status_footer,
                        kagi::ui::FooterStatus::Failed(_)
                    ));
                }
                assert_eq!((state.bottom_panel_open, state.bottom_tab), panel_before);
                let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                if recording_failure {
                    let notice = state
                        .app_notice()
                        .expect("failed append must remain explicit");
                    assert!(notice
                        .message
                        .contains(&std::fs::TryLockError::WouldBlock.to_string()));
                    assert!(
                        toasts.is_empty(),
                        "no post-recorder raw toast may accompany required acknowledgment"
                    );
                    let panel = state.op_log.as_ref().unwrap().read(cx);
                    let rows: Vec<_> = panel
                        .entries()
                        .iter()
                        .filter(|entry| {
                            entry.op == "merge-continue" && Path::new(&entry.repo) == repo.as_path()
                        })
                        .collect();
                    assert_eq!(rows.len(), 1);
                    let OpOutcome::Failed { error } = &rows[0].outcome else {
                        panic!(
                            "failed persistence must not reclassify an unchanged backend failure"
                        );
                    };
                    assert!(
                        error.contains(repo.to_str().unwrap()),
                        "the real backend diagnostic survives failed persistence"
                    );
                    assert!(
                        rows[0].ref_moves.is_none(),
                        "an unrecorded row cannot advertise durable restoration evidence"
                    );
                } else {
                    assert!(state.app_notice().is_none());
                    assert_eq!(toasts.len(), 1);
                    assert_eq!(toasts[0].kind, kagi::ui::ToastKind::Error);
                    assert!(!toasts[0].message.contains("merge-continue"));
                    assert!(!toasts[0].message.contains(repo.to_str().unwrap()));
                }
            });
            drop(recording_lock);
            if recording_failure {
                click_control(cx, window, "app-notice-confirm");
                cx.run_until_parked();
                assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
                assert!(
                    read_oplog_tail_for_repo(&repo, 100)
                        .into_iter()
                        .all(|entry| entry.op != "merge-continue"),
                    "acknowledgment must not retry append"
                );
            } else {
                assert_eq!(
                    kagi_ui_core::klog::tail()
                        .iter()
                        .filter(
                            |line| line.as_str() == "[kagi] bottom-panel: open (Failed auto-open)"
                        )
                        .count(),
                    before_panel + usize::from(!departed)
                );
                click_control(cx, window, "operation-result-details");
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(state.bottom_panel_open);
                    assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                    let panel = state.op_log.as_ref().unwrap().read(cx);
                    let row = &panel.entries()[panel.expanded().unwrap()];
                    let receipt = &receipts[0];
                    assert_eq!(
                        (row.id, row.timestamp, &row.op, &row.repo),
                        (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
                    );
                });
            }
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS conflict_merge_failure_contract: current/departed × healthy/failed audit append");
}

pub fn scenario_conflict_skip_panic(cx: &mut VisualTestAppContext) {
    let fixture = sequencer_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let pane = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    pane.update(cx, |view, _| assert!(!view.skip_request_arm()));
    drop(pane);
    kagi::ui::KagiApp::panic_next_conflict_skip_for_e2e();
    app.update(cx, |app, cx| {
        let owner = app
            .app_sessions
            .attachment(app.active_session().unwrap())
            .unwrap();
        app.conflict_skip(owner, cx);
    });
    assert_panicked_conflict(cx, &app, &repo, "cherry-pick-skip");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_skip_panic");
}

/// Real Save / directory-file resolution / Abort consumers share one result
/// owner, including a successful backend operation whose audit append fails.
pub fn scenario_conflict_family_result_owner(cx: &mut VisualTestAppContext) {
    use kagi::ui::{i18n, FooterStatus, ToastKind};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let language = i18n::lang();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for active in [true, false] {
            for recording_failure in [false, true] {
                for action in ["save", "directory", "abort"] {
                    let fixture = if action == "directory" {
                        dir_file_fixture()
                    } else {
                        content_fixture()
                    };
                    let repo = fixture.path().canonicalize().unwrap();
                    let head = git_fixture::git_output(&repo, &["rev-parse", "HEAD"]);
                    i18n::set_lang(lang);
                    let (app, window) = mount(cx, &repo);
                    app.update(cx, |app, cx| {
                        app.set_window_active_for_e2e(active, cx);
                        app.detect_conflict_mode(cx);
                    });
                    cx.run_until_parked();
                    let panel_before = cx.read(|cx| {
                        let state = app.read(cx);
                        (state.bottom_panel_open, state.bottom_tab)
                    });
                    let expected_saved = if action == "save" {
                        let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
                        Some(conflict.update(cx, |view, cx| {
                            let mode = view.mode.as_mut().unwrap();
                            mode.buffer
                                .apply_choice(
                                    Path::new("file.txt"),
                                    kagi_git::ResolutionChoice::Current,
                                )
                                .unwrap();
                            let expected =
                                mode.buffer.resolved_text(Path::new("file.txt")).unwrap();
                            view.conflict_open_editor(&PathBuf::from("file.txt"));
                            view.conflict_show_page(
                                kagi::ui::conflict_view::ConflictPage::Review,
                                cx,
                            );
                            cx.notify();
                            expected
                        }))
                    } else {
                        None
                    };
                    let recording_lock = recording_failure.then(|| {
                        let path =
                            std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                                .join("operations.jsonl.lock");
                        let lock = std::fs::OpenOptions::new()
                            .create(true)
                            .truncate(false)
                            .write(true)
                            .open(path)
                            .unwrap();
                        lock.lock().unwrap();
                        lock
                    });
                    let operation = match action {
                        "save" => {
                            click_control(cx, window, "conflict-save");
                            "conflict-save:merge"
                        }
                        "directory" => {
                            click_control(cx, window, "conflict-row-0");
                            click_control(cx, window, "conflict-keep-directory");
                            "conflict-dir-file:keep-directory"
                        }
                        "abort" => {
                            click_control(cx, window, "conflict-abort");
                            app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
                            "merge-abort"
                        }
                        _ => unreachable!(),
                    };
                    wait_idle(cx, &app);
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    assert_eq!(git_fixture::git_output(&repo, &["rev-parse", "HEAD"]), head);
                    if let Some(expected) = expected_saved {
                        assert_eq!(
                            std::fs::read_to_string(repo.join("file.txt")).unwrap(),
                            expected
                        );
                    }
                    if action == "directory" {
                        let index = git2::Repository::open(&repo).unwrap().index().unwrap();
                        assert!(index.get_path(Path::new("thing"), 0).is_none());
                        assert!(index.get_path(Path::new("thing/child"), 0).is_some());
                    }
                    if action == "abort" {
                        assert!(!repo.join(".git/MERGE_HEAD").exists());
                        assert_eq!(
                            git_fixture::git_output(&repo, &["status", "--porcelain=v1"]),
                            ""
                        );
                    }
                    let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                        .into_iter()
                        .filter(|entry| entry.op == operation)
                        .collect();
                    assert_eq!(receipts.len(), usize::from(!recording_failure));
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(!state.app_sessions.has_leases());
                        assert_eq!((state.bottom_panel_open, state.bottom_tab), panel_before);
                        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                        if recording_failure {
                            let notice = state
                                .app_notice()
                                .expect("failed audit append requires acknowledgment");
                            assert!(notice
                                .message
                                .contains(&std::fs::TryLockError::WouldBlock.to_string()));
                            assert!(toasts.is_empty());
                            assert!(!matches!(
                                state.status_footer,
                                FooterStatus::Success(_) | FooterStatus::Failed(_)
                            ));
                            let panel = state.op_log.as_ref().unwrap().read(cx);
                            let rows: Vec<_> = panel
                                .entries()
                                .iter()
                                .filter(|entry| {
                                    entry.op == operation
                                        && Path::new(&entry.repo) == repo.as_path()
                                })
                                .collect();
                            assert_eq!(rows.len(), 1);
                            assert!(matches!(rows[0].outcome, OpOutcome::Partial { .. }));
                        } else {
                            assert!(state.app_notice().is_none());
                            assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
                            if active {
                                assert!(toasts.is_empty(), "attended result stays in the footer");
                                assert!(matches!(state.status_footer, FooterStatus::Success(_)));
                            } else {
                                assert_eq!(toasts.len(), 1);
                                assert_eq!(toasts[0].kind, ToastKind::Success);
                                assert!(
                                    !matches!(state.status_footer, FooterStatus::Success(_)),
                                    "one unattended result must not be duplicated in the footer"
                                );
                            }
                        }
                    });
                    drop(recording_lock);
                    if recording_failure {
                        click_control(cx, window, "app-notice-confirm");
                        assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
                    }
                    assert_eq!(
                        read_oplog_tail_for_repo(&repo, 100).len(),
                        receipts.len(),
                        "the presenter and acknowledgment never append again"
                    );
                    unmount(cx, app, window);
                }
            }
        }
    }
    i18n::set_lang(language);
}

/// A real sequencer backend-open failure consumes only its matching, already
/// admitted confirmation and leaves one result (or required audit acknowledgment).
pub fn scenario_conflict_continue_failure_result_owner(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let language = kagi::ui::i18n::lang();
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        for recording_failure in [false, true] {
            for departed in [false, true] {
                let fixture = sequencer_fixture();
                let repo = fixture.path().canonicalize().unwrap();
                let other = build_fixture();
                let other_repo = other.path().canonicalize().unwrap();
                kagi::ui::i18n::set_lang(lang);
                let (app, window) = mount(cx, &repo);
                app.update(cx, |app, cx| {
                    app.set_window_active_for_e2e(false, cx);
                    app.detect_conflict_mode(cx);
                });
                cx.run_until_parked();
                app.update(cx, |app, cx| {
                    app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                        view.mode
                            .as_mut()
                            .unwrap()
                            .buffer
                            .apply_choice(
                                Path::new("README.md"),
                                kagi_git::ResolutionChoice::Incoming,
                            )
                            .unwrap();
                    });
                });
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        let owner = app
                            .app_sessions
                            .attachment(app.active_session().unwrap())
                            .unwrap();
                        app.conflict_continue(owner, window, cx);
                    });
                })
                .unwrap();
                crate::busy_dismiss::wait(cx, &app, "Continue plan after draft flush", |app| {
                    app.conflict_continue_modal().is_some()
                });
                assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_some()));
                let repo_before = crate::macos::repo_fingerprint(&repo);
                let other_before = crate::macos::repo_fingerprint(&other_repo);
                let (hold, release) = crate::evidence_support::deferred::<()>(cx);
                kagi::ui::KagiApp::hold_next_continue_confirm_for_e2e(hold);
                app.update(cx, |app, cx| app.confirm_conflict_continue(cx));
                // Wait for flush admission before leaving or hiding the repository:
                // the existing hold suspends the backend write, not draft persistence.
                crate::busy_dismiss::wait(cx, &app, "Continue confirmation admitted", |app| {
                    app.write_busy_op.is_some()
                });
                if departed {
                    app.update(cx, |app, cx| {
                        app.cancel_conflict_continue();
                        assert!(app.open_repository(other_repo.clone(), cx));
                    });
                    cx.run_until_parked();
                }
                let foreign_before = departed.then(|| {
                    cx.read(|cx| {
                        let state = app.read(cx);
                        (
                            state.active_session(),
                            format!("{:?}", state.status_footer),
                            (state.bottom_panel_open, state.bottom_tab),
                        )
                    })
                });
                let recording_lock = recording_failure.then(|| {
                    let path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                        .join("operations.jsonl.lock");
                    let lock = std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .write(true)
                        .open(path)
                        .unwrap();
                    lock.lock().unwrap();
                    lock
                });
                std::fs::rename(repo.join(".git"), repo.join(".git-paused")).unwrap();
                release.send(());
                crate::busy_dismiss::wait(cx, &app, "Continue backend-open failure", |app| {
                    app.write_busy_op.is_none() && app.conflict_continue_modal().is_none()
                });
                std::fs::rename(repo.join(".git-paused"), repo.join(".git")).unwrap();
                let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                    .into_iter()
                    .filter(|entry| entry.op == "cherry-pick-continue")
                    .collect();
                assert_eq!(receipts.len(), usize::from(!recording_failure));
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(!state.app_sessions.has_leases());
                    let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                    assert!(state.conflict_continue_modal().is_none(),
                        "terminal completion retained a consumed Confirm: error={:?}; ordinary results={:?}",
                        state.conflict_continue_modal().and_then(|modal| modal.error.as_ref()),
                        toasts.iter().map(|toast| format!("{:?}: {}", toast.kind, toast.message)).collect::<Vec<_>>());
                    if let Some(before) = foreign_before.as_ref() {
                        assert_eq!((state.active_session(), format!("{:?}", state.status_footer),
                            (state.bottom_panel_open, state.bottom_tab)), *before);
                    }
                    if recording_failure {
                        let notice = state.app_notice().expect("failed append must reach the released modal slot");
                        assert!(notice.message.contains(&std::fs::TryLockError::WouldBlock.to_string()));
                        assert!(toasts.is_empty());
                    } else {
                        assert!(state.app_notice().is_none());
                        assert_eq!(toasts.len(), 1);
                        assert_eq!(toasts[0].kind, kagi::ui::ToastKind::Error);
                        assert!(!toasts[0].message.contains(repo.to_str().unwrap()));
                        assert!(!toasts[0].message.contains("cherry-pick-continue"));
                        let OpOutcome::Failed { error } = &receipts[0].outcome else {
                            panic!("a backend-open failure is a known failure, not Unknown");
                        };
                        assert!(error.contains(repo.to_str().unwrap()));
                    }
                });
                drop(recording_lock);
                if recording_failure {
                    click_control(cx, window, "app-notice-confirm");
                    cx.run_until_parked();
                    assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
                    assert!(read_oplog_tail_for_repo(&repo, 100)
                        .into_iter()
                        .all(|entry| entry.op != "cherry-pick-continue"));
                } else {
                    click_control(cx, window, "operation-result-details");
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.bottom_panel_open);
                        assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                        assert_eq!(state.repo_path.as_deref(), Some(repo.as_path()));
                        let panel = state.op_log.as_ref().unwrap().read(cx);
                        let row = &panel.entries()[panel.expanded().unwrap()];
                        let receipt = &receipts[0];
                        assert_eq!(
                            (row.id, row.timestamp, &row.op, &row.repo),
                            (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
                        );
                    });
                }
                assert_eq!(crate::macos::repo_fingerprint(&repo), repo_before);
                assert_eq!(crate::macos::repo_fingerprint(&other_repo), other_before);
                unmount(cx, app, window);
            }
        }
    }
    kagi::ui::i18n::set_lang(language);
}
