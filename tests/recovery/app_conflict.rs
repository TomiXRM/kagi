//! Focused C1 GUI adapters. Run each with its exact KAGI_GUI_E2E_ONLY filter.
use crate::macos::{git, mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn content_fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(repo.join("file.txt"), "feature\n").unwrap();
    git(repo, &["commit", "-qam", "feature"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("file.txt"), "main\n").unwrap();
    git(repo, &["commit", "-qam", "main"]);
    let status = std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(!status.success());
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

fn click_control(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, name: &'static str) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} control was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
}

fn wait_idle(cx: &mut VisualTestAppContext, app: &gpui::Entity<kagi::ui::KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).write_busy_op.is_none()) {
            break;
        }
        assert!(Instant::now() < deadline, "conflict adapter did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn scenario_conflict_save_boundary(cx: &mut VisualTestAppContext) {
    // A marker-bearing owned draft is a Backend-recorded refusal and preserves
    // the long-standing footer/klog contract byte-for-byte.
    {
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        let conflict = cx.read(|cx| app.read(cx).conflict.clone()).unwrap();
        conflict.update(cx, |view, cx| {
            view.conflict_open_editor(Path::new("file.txt"));
            view.conflict_editor_reset_all(Path::new("file.txt"));
            cx.notify();
        });
        assert!(cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().expect("conflict mode");
            matches!(
                mode.buffer.conflict_draft(Path::new("file.txt")),
                Some(kagi_domain::conflict_family::ConflictDraft::Text(bytes))
                    if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
            )
        }));
        click_control(cx, window, "conflict-save");
        cx.run_until_parked();
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "conflict-save:merge")
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Refused { .. }));
        const EXPECTED_REFUSAL_FOOTER: &str = "conflict-save:merge: refused (1 blocker)";
        assert!(cx.read(|cx| matches!(
            &app.read(cx).status_footer,
            kagi::ui::FooterStatus::Failed(message)
                if message.as_ref() == EXPECTED_REFUSAL_FOOTER
        )));
        drop(conflict);
        unmount(cx, app, window);
    }

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx
        .read(|cx| app.read(cx).conflict.clone())
        .expect("conflict view");
    let path = PathBuf::from("file.txt");
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(&path);
        view.conflict_editor_set_file_side(
            &path,
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        view.result_editing = true;
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
        .read(|cx| app.read(cx).conflict.clone())
        .expect("D/F conflict view");

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
/// Unstage and Discard all. The repository is left `MERGING` with a clean
/// index and nothing unmerged — no conflict, no `ConflictView`, and before
/// this no way out but the CLI.
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
    let (app, window) = mount(cx, &repo);
    // No `detect_conflict_mode` call: the strip comes from the tab's first
    // accepted read, which is the whole ownership fix.
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).conflict.is_none()),
        "there is no conflict editor to hang the abort off"
    );
    assert!(
        cx.read(|cx| app.read(cx).view().operation.is_some()),
        "the read model knows the merge is still in progress"
    );
    assert!(
        e2e::control_bounds(window.window_id(), "operation-strip").is_some(),
        "the operation strip is on screen from the first frame"
    );
    assert!(
        e2e::control_bounds(window.window_id(), "operation-strip-abort").is_some(),
        "and it offers the way out"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS operation strip visible at startup on a stuck MERGING repo");
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
        app.apply_conflict_detect(owner.clone(), ConflictDetectOutcome::OpenFailed, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        observed(cx),
        Some(before),
        "a stale detector result must not erase the accepted read's observation"
    );

    // …and Abort still goes through, which is what the erasure used to break.
    click_control(cx, window, "operation-strip-abort");
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
            .conflict
            .as_ref()
            .and_then(|view| view.read(cx).mode.as_ref().map(|m| m.revision.clone()))
    });
    assert_ne!(
        mode_revision.as_ref(),
        Some(&r2_revision),
        "a stale payload must not be given the accepted read's revision"
    );
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

/// #707 re-review P1: a detector launched for one tab must not land on another,
/// even when the path is identical — a close and reopen of the same repository
/// is a new `SessionId` and a new visit.
pub fn scenario_conflict_detect_wrong_owner_is_dropped(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let payload = e2e::detect_payload_for_test(&repo, "main");

    // The owner the job would have frozen, had it launched one visit earlier.
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

    assert!(cx.read(|cx| app.read(cx).conflict.is_none()));
    app.update(cx, |app, cx| {
        app.apply_conflict_detect(stale_owner, payload, cx)
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict.is_none()),
        "a payload from another visit must not build this tab's conflict editor"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a detector result for another owner is dropped (#707)");
}

/// #707 re-review P2: a stale "there is no conflict" answer must not tear down
/// the projection the accepted read still says exists.
///
/// Only `Detected` was compared against the accepted observation, so a late
/// `Cleared` / `MergeResolvedReady` / `OpenFailed` from the same owner dropped
/// the live `ConflictView` (and, for `OpenFailed`, the stash identity) and left
/// nothing until the next reload — with the detector not even re-armed.
pub fn scenario_conflict_detect_stale_clear_is_dropped(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e::ConflictDetectOutcome;

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let view = cx
        .read(|cx| app.read(cx).conflict.clone())
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
    let observation = cx
        .read(|cx| app.read(cx).view().operation.clone())
        .map(|op| op.observation)
        .expect("the accepted read says an operation is in progress");

    // `Cleared` / `OpenFailed` carry no observation: they disagree with an
    // accepted read that says an operation is in progress, full stop. A
    // `MergeResolvedReady` is stale when its own observation is not the
    // accepted one.
    let older = kagi_domain::conflict_family::ConflictObservation {
        revision: kagi_domain::conflict_family::ConflictRevision::from_fingerprint(
            "an-older-observation".to_string(),
        ),
        ..observation.clone()
    };
    for stale in [
        ConflictDetectOutcome::Cleared,
        ConflictDetectOutcome::OpenFailed,
        ConflictDetectOutcome::MergeResolvedReady(older),
    ] {
        app.update(cx, |app, cx| {
            app.conflict_detected_for = Some(repo.clone());
            app.apply_conflict_detect(owner.clone(), stale, cx);
        });
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).conflict.is_some()),
            "a stale no-conflict answer must not drop the editor"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).conflict.as_ref().and_then(|v| v
                .read(cx)
                .mode
                .as_ref()
                .map(|m| m.revision.clone()))),
            Some(revision.clone()),
            "and must not replace what it shows"
        );
        assert!(
            cx.read(|cx| app.read(cx).conflict_detected_for.is_none()),
            "dropping a stale outcome re-arms the detector"
        );
    }

    // The same `MergeResolvedReady` is applied once the read agrees with it:
    // the guard is about disagreement, not about the variant.
    std::fs::write(repo.join("file.txt"), "resolved\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    app.update(cx, |app, cx| app.reload(cx));
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict.is_none()),
        "an agreeing outcome still applies"
    );
    drop(view);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS a stale no-conflict outcome keeps the projection (#707)");
}

pub fn scenario_operation_strip_abort(cx: &mut VisualTestAppContext) {
    let fixture = stuck_merging_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    // ADR-0196 Wave 3: the abort is a write, so both its stages consult the one
    // gate. A plan in flight takes no lease, so only `planning` refuses it —
    // and nothing may reach the family while it does.
    app.update(cx, |app, cx| {
        app.planning = Some("merge-plan");
        app.open_conflict_abort_modal(cx);
        assert!(
            app.conflict_abort_modal().is_none(),
            "a plan in flight must refuse the abort confirmation (#283)"
        );
        app.confirm_conflict_abort(cx);
        app.planning = None;
    });
    cx.run_until_parked();
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "and a refused abort mutates nothing"
    );
    click_control(cx, window, "operation-strip-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "the first click confirms nothing — it opens the plan"
    );
    assert!(repo.join(".git/MERGE_HEAD").exists(), "and mutates nothing");
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
    // #704 review P2: an abort saves nothing, so it must not be announced with
    // the Save family's wording.
    let toast = cx
        .read(|cx| {
            app.read(cx).toast_stack.as_ref().map(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .last()
                    .map(|toast| toast.message.to_string())
                    .unwrap_or_default()
            })
        })
        .unwrap_or_default();
    assert_eq!(
        toast,
        kagi::ui::i18n::Msg::ConflictAborted.t(),
        "the success toast names the abort, not a saved resolution"
    );
    assert_ne!(toast, kagi::ui::i18n::Msg::EditorSavedResolved.t());
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS header Abort → confirm → MERGE_HEAD gone (#704)");
}
