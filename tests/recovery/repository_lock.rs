//! #1746: repository admission probes must not manufacture durable receipts.
use crate::app_conflict::{content_fixture, wait_idle};
use crate::conflict_draft_revision::RebaseFixture;
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;

const BUSY: &str = "Another Kagi window is operating on this repository";

/// Hold a separate descriptor on a scoped thread. The backend's
/// conflict_write_lock_test owns the independent-process/death-release oracle;
/// its child entry point is not available in the native runner binary.
fn with_repository_lock(repo: &Path, action: impl FnOnce()) {
    std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        scope.spawn(move || {
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(repo.join(".git/kagi-write.lock"))
                .unwrap();
            lock.try_lock().unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv();
            drop(lock);
        });
        ready_rx.recv().unwrap();
        action();
        // Disconnect also releases the owner if the scenario panics.
        drop(release_tx);
    });
}

fn assert_one_refusal(repo: &Path, before: usize, operation: &str) {
    let entries = read_oplog_tail_for_repo(repo, 100);
    assert_eq!(
        entries.len(),
        before + 1,
        "one operation receipt, no admission receipt"
    );
    let receipt = entries.iter().find(|entry| entry.op == operation).unwrap();
    assert!(
        matches!(&receipt.outcome, OpOutcome::Refused { blockers }
            if blockers.len() == 1 && blockers[0].contains(BUSY)),
        "the operation must name repository contention: {:?}",
        receipt.outcome
    );
    assert!(entries
        .iter()
        .all(|entry| entry.op != "conflict-write-admission"));
}

pub fn scenario_conflict_repository_lock_continue(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_set_file_side(
            Path::new("file.txt"),
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        cx.notify();
    });
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let worktree = std::fs::read(repo.join("file.txt")).unwrap();
    let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
    with_repository_lock(&repo, || {
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
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "merge-continue");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
        assert_eq!(
            std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
            merge_head
        );
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    });
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_repository_lock_skip(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let worktree = std::fs::read(repo.join("file.txt")).unwrap();
    let head = std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, _| assert!(!view.skip_request_arm()));
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_skip(owner, cx);
        });
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "rebase-skip");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
        assert_eq!(std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap(), head);
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    });
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_repository_lock_silent_auto_fetch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::fetch_owner::local_remote_with_change(&repo, remote.path());
    let (app, window) = mount(cx, &repo);
    wait_idle(cx, &app);
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let toasts = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .map_or(0, |stack| stack.read(cx).toasts().len())
    });
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            assert!(
                !app.fetch_async_for(true, None, cx),
                "busy auto-fetch must not launch"
            );
        });
        wait_idle(cx, &app);
        assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), before);
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state
                    .toast_stack
                    .as_ref()
                    .map_or(0, |stack| stack.read(cx).toasts().len()),
                toasts
            );
            assert!(state.fetch_in_flight.is_none());
            assert!(!state.app_sessions.has_leases());
            assert!(state.app_notice().is_none());
        });
    });
    unmount(cx, app, window);
}

pub fn scenario_repository_lock_stage_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("lock-stage.txt"), "unstaged\n").unwrap();
    let (app, window) = mount(cx, &repo);
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            let owner = app.active_session().unwrap();
            app.do_stage_file_by_path(owner, "lock-stage.txt".into(), cx);
        });
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "stage");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        let entries = read_oplog_tail_for_repo(&repo, 100);
        let receipt = entries.iter().find(|entry| entry.op == "stage").unwrap();
        assert!(matches!(&receipt.outcome, OpOutcome::Refused { blockers }
            if blockers[0].contains("lock-stage.txt")));
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    });
    unmount(cx, app, window);
}
