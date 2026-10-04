//! Panic recovery through the actual background writer tasks and host completions.
use std::time::{Duration, Instant};

use gpui::{Focusable, VisualTestAppContext};
use kagi::ui::{FooterStatus, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::macos::{build_fixture, git, mount, unmount};
fn acknowledge_panicked_writer(app: &gpui::Entity<KagiApp>, cx: &mut VisualTestAppContext) {
    app.update(cx, |app, cx| {
        kagi::ui::e2e::poll_app_jobs(app, cx);
        kagi::ui::e2e::present_app_notice(app);
        let ids = app.app_sessions.reconcile_ids();
        assert_eq!(ids.len(), 1, "panic must register one reconcile exit");
        assert!(
            app.app_sessions.has_leases(),
            "Unknown must retain admission"
        );
        assert!(
            kagi::ui::e2e::app_notice_message(app).is_some(),
            "reconcile notice must be offered"
        );
        let read = kagi::app::read_reconcile(&app.app_sessions, ids[0])
            .expect("stopped writer is readable");
        assert!(
            read.stop_proven() && read.resolved(),
            "stopped local writer may be acknowledged"
        );
        kagi::app::acknowledge(&mut app.app_sessions, read).unwrap();
    });
}

pub fn scenario_remote_branch_fetch_panic(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let bare = tempfile::tempdir().unwrap();
    let remote = bare.path().join("origin.git");
    git(&repo, &["init", "--bare", "-q", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "origin", "main"]);
    let (app, window) = mount(cx, &repo);
    KagiApp::panic_next_branch_fetch_for_e2e();
    app.update(cx, |app, cx| {
        app.fetch_remote_branch_async("origin/main".into(), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.app_sessions.has_leases(),
            "panicked branch fetch silently released its writer lease"
        );
        assert!(
            matches!(app.status_footer, FooterStatus::Failed(_)),
            "panicked fetch must not claim success"
        );
    });
    let receipts = read_oplog_tail_for_repo(&repo, 20);
    assert!(
        receipts.iter().any(
            |r| r.op == "fetch-remote-branch" && matches!(r.outcome, OpOutcome::Unknown { .. })
        ),
        "panicked branch fetch needs a durable Unknown receipt"
    );
    acknowledge_panicked_writer(&app, cx);
    app.update(cx, |app, cx| {
        app.fetch_remote_branch_async("origin/main".into(), cx)
    });
    cx.run_until_parked();
    assert!(
        !cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "retry must settle"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_branch_fetch_panic");
}

/// A normal failed fetch still belongs to the repository it was started for,
/// even if another tab is visible before the worker runs.
pub fn scenario_remote_branch_fetch_failed_after_departure(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_repo = other.path().canonicalize().unwrap();
    let missing = repo.join("missing-remote");
    git(
        &repo,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx))
    });
    cx.run_until_parked();
    let before = read_oplog_tail_for_repo(&repo, 100)
        .iter()
        .filter(|entry| entry.op == "fetch-remote-branch")
        .count();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.fetch_remote_branch_async("origin/main".into(), cx);
        app.switch_repo(1, cx);
        app.status_footer = FooterStatus::Idle("other tab sentinel".into());
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.app_sessions.has_leases(), "failed fetch did not settle");
        assert!(
            matches!(&app.status_footer, FooterStatus::Idle(text) if text.as_ref() == "other tab sentinel"),
            "old branch fetch changed the current footer"
        );
        let toasts = app.toast_stack.as_ref().expect("mounted toast stack");
        assert!(
            !toasts.read(cx).toasts().iter().any(|toast| {
                toast.message.as_ref().contains("Fetched origin/main")
                    || toast.message.as_ref().contains("Fetch failed")
            }),
            "old branch fetch displayed a toast on the current tab"
        );
    });
    let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch-remote-branch")
        .collect();
    assert_eq!(
        receipts.len(),
        before + 1,
        "frozen repository has one failure"
    );
    assert!(
        matches!(receipts.last().unwrap().outcome, OpOutcome::Failed { .. }),
        "ordinary transport failure must not become Unknown"
    );
    cx.read(|cx| {
        let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
        assert!(
            panel.entries().iter().any(|entry| {
                entry.id == receipts.last().unwrap().id
                    && entry.op == "fetch-remote-branch"
                    && entry.repo == repo.display().to_string()
            }),
            "departed fetch receipt was not pushed to the live Operation Log"
        );
    });
    assert!(
        !read_oplog_tail_for_repo(&other_repo, 100)
            .iter()
            .any(|entry| entry.op == "fetch-remote-branch"),
        "failure was recorded on the active tab instead of the owner"
    );
    assert!(
        kagi_ui_core::klog::tail().iter().any(|line| {
            line.starts_with("[kagi] fetch-remote-branch: failed origin/main — ")
                && line.contains(missing.to_str().unwrap())
        }),
        "departed failure lost its unchanged klog contract line"
    );
    unmount(cx, app, window);
}

/// A successful branch fetch also emits its contract line after departure;
/// only the footer and toast belong to the active tab.
pub fn scenario_remote_branch_fetch_success_after_departure(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_repo = other.path().canonicalize().unwrap();
    let remote_root = tempfile::tempdir().unwrap();
    let remote = remote_root.path().join("origin.git");
    git(&repo, &["init", "--bare", "-q", remote.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "origin", "main"]);

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| assert!(app.open_repository(other_repo, cx)));
    cx.run_until_parked();
    let contract_line = "[kagi] fetch-remote-branch: ok origin/main";
    let before = kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.as_str() == contract_line)
        .count();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.fetch_remote_branch_async("origin/main".into(), cx);
        app.switch_repo(1, cx);
        app.status_footer = FooterStatus::Idle("other tab sentinel".into());
    });
    cx.run_until_parked();
    assert_eq!(
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.as_str() == contract_line)
            .count(),
        before + 1,
        "departed success lost its unchanged klog contract line"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.as_ref() == "other tab sentinel"));
        assert!(!state.app_sessions.has_leases());
        assert!(
            !state.toast_stack.as_ref().unwrap().read(cx).toasts().iter()
                .any(|toast| toast.message.as_ref().contains("Fetched origin/main")),
            "departed success displayed on the other tab"
        );
    });
    unmount(cx, app, window);
}

/// A PR ref fetch started in one visit cannot publish into the next visit;
/// settling the old fetch must start exactly one fresh fetch for the open PR.
pub fn scenario_pr_ref_fetch_restarts_after_revisit(cx: &mut VisualTestAppContext) {
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let fixture = build_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_repo = other.path().canonicalize().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    let file_url = format!("file://{}", remote.display());
    git(
        &repo,
        &["config", &format!("url.{file_url}.insteadOf"), url],
    );
    let head =
        crate::pr_viewed::push_pr_head(&repo, &remote, "main", &[("a.txt", "new PR content\n")]);
    let pr = crate::pr_viewed::pr_at(&head);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| assert!(app.open_repository(other_repo, cx)));
    cx.run_until_parked();
    let contract_line = "[kagi] pr-mode: fetch #7 start";
    let before = kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.as_str() == contract_line)
        .count();
    let previous_completions = kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.as_str() == "[kagi] pr-mode: fetch #7 ok")
        .count();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().unwrap();
        let original_visit = app.app_sessions.attachment(owner).unwrap().visit;
        app.pr_mode_open(&pr, cx);
        assert!(app.pr_mode().unwrap().tabs[0].local_refs_loading);
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        assert_ne!(
            app.app_sessions.attachment(owner).map(|a| a.visit),
            Some(original_visit)
        );
        // #643 S6 clears PR mode on activation. Reopen the same PR before
        // releasing the old worker; this new tab owns its own fetch latch.
        assert!(app.pr_mode().is_none());
        app.pr_mode_open(&pr, cx);
        assert!(app.pr_mode().unwrap().tabs[0].local_refs_loading);
    });
    // Wait for the old completion without advancing the virtual clock: the
    // new visit's fetch is queued behind its 200ms lease retry timer.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.as_str() == "[kagi] pr-mode: fetch #7 ok")
            .count()
            > previous_completions
        {
            break;
        }
        assert!(Instant::now() < deadline, "old PR fetch did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.read(|cx| {
        assert!(
            app.read(cx).pr_mode().unwrap().tabs[0].local_refs_loading,
            "old visit cleared the reopened PR's current-visit loading latch"
        );
    });
    cx.advance_clock(Duration::from_millis(250));
    cx.run_until_parked();
    crate::pr_viewed::wait_loaded(cx, &app, &head);
    assert_eq!(
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.as_str() == contract_line)
            .count(),
        before + 2,
        "old completion did not start exactly one fetch for the new visit"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        let tab = &state.pr_mode().unwrap().tabs[0];
        assert!(!tab.local_refs_loading, "fresh fetch left PR loading");
        assert_eq!(
            tab.head.0, head,
            "the new visit did not receive the PR head"
        );
        assert!(tab
            .files
            .iter()
            .any(|file| file.path.to_string_lossy() == "a.txt"));
    });
    unmount(cx, app, window);
}

pub fn scenario_pr_ref_fetch_panic(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    KagiApp::panic_next_pr_fetch_for_e2e();
    app.update(cx, |app, cx| {
        app.pr_mode_open(&crate::cleanup_publish_owner::pr(355, "missing"), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.app_sessions.has_leases(),
            "panicked PR fetch silently released its writer lease"
        );
        assert!(
            !app.pr_mode().unwrap().tabs[0].local_refs_loading,
            "panicked PR fetch left its tab loading forever"
        );
    });
    let receipts = read_oplog_tail_for_repo(&repo, 20);
    assert!(
        receipts
            .iter()
            .any(|r| r.op == "fetch-pr" && matches!(r.outcome, OpOutcome::Unknown { .. })),
        "panicked PR fetch needs a durable Unknown receipt"
    );
    acknowledge_panicked_writer(&app, cx);
    app.update(cx, |app, cx| {
        app.pr_mode_open(&crate::cleanup_publish_owner::pr(355, "missing"), cx)
    });
    cx.run_until_parked();
    assert!(
        !cx.read(|cx| app.read(cx).pr_mode().unwrap().tabs[0].local_refs_loading),
        "a fresh PR fetch must be allowed after the panic"
    );
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo, cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    KagiApp::panic_next_pr_fetch_for_e2e();
    let owner = app.update(cx, |app, cx| {
        let owner = app.active_session().unwrap();
        app.pr_mode_open(
            &crate::cleanup_publish_owner::pr(356, "another-missing"),
            cx,
        );
        assert!(app.pr_mode().unwrap().tabs[0].local_refs_loading);
        app.switch_repo(1, cx);
        app.status_footer = FooterStatus::Idle("other tab sentinel".into());
        owner
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.as_ref() == "other tab sentinel"),
            "old PR fetch presented in a different owner's footer");
        assert!(!state.ui.get(&owner).unwrap().pr_mode.as_ref().unwrap().tabs[0].local_refs_loading,
            "old visit left its PR loading latch set in the background tab");
    });
    // A stale completion still owns its receipt and reconciliation, not B's UI.
    assert!(read_oplog_tail_for_repo(&repo, 20)
        .iter()
        .any(|entry| entry.op == "fetch-pr"
            && matches!(entry.outcome, OpOutcome::Unknown { .. })
            && entry.before.head == "PR #356"));
    cx.read(|cx| {
        let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
        assert!(
            panel.entries().iter().any(|entry| {
                entry.op == "fetch-pr"
                    && entry.before.head == "PR #356"
                    && entry.repo == repo.display().to_string()
            }),
            "departed PR fetch receipt was not pushed to the live Operation Log"
        );
    });
    acknowledge_panicked_writer(&app, cx);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_ref_fetch_panic");
}

pub fn scenario_editor_save_panic(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));
    KagiApp::panic_next_editor_save_for_e2e();
    app.update(cx, |app, cx| app.save_editor_file(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "panicked editor save silently released its writer lease"
    );
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "panicked save must leave the buffer dirty"
    );
    assert_eq!(
        std::fs::read(repo.join("README.md")).unwrap(),
        before,
        "panicked save must not claim to have written"
    );
    let receipts = read_oplog_tail_for_repo(&repo, 20);
    assert!(
        receipts
            .iter()
            .any(|r| r.op == "editor-save" && matches!(r.outcome, OpOutcome::Unknown { .. })),
        "panicked editor save needs a durable Unknown receipt"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        let panel = state.op_log.as_ref().unwrap().read(cx);
        let receipt = receipts
            .iter()
            .find(|r| r.op == "editor-save" && matches!(r.outcome, OpOutcome::Unknown { .. }))
            .unwrap();
        assert_eq!(
            panel
                .entries()
                .iter()
                .filter(|entry| entry.id == receipt.id && entry.op == "editor-save")
                .count(),
            1,
            "live editor panic receipt must reach the Operation Log exactly once"
        );
        assert_eq!(
            state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .filter(|toast| toast.message.as_ref().contains("save task unwound"))
                .count(),
            1,
            "live editor panic must show one toast"
        );
    });
    acknowledge_panicked_writer(&app, cx);
    app.update(cx, |app, cx| app.save_editor_file(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !editor.read(cx).dirty && !app.read(cx).app_sessions.has_leases()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor retry did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor_save_panic");
}

/// The completion belongs to the host, not to the pane that initiated the save.
pub fn scenario_editor_save_panic_after_close(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));

    KagiApp::panic_next_editor_save_for_e2e();
    app.update(cx, |app, cx| app.save_editor_file(cx));
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.has_leases()),
        "save must be admitted before closing its pane"
    );
    app.update(cx, |app, _| app.close_editor_workspace());
    drop(editor);
    assert!(cx.read(|cx| app.read(cx).ui().editor_workspace.is_none()));
    cx.run_until_parked();

    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 20)
        .into_iter()
        .filter(|entry| entry.op == "editor-save")
        .collect();
    assert_eq!(
        receipts.len(),
        1,
        "panicked save must have exactly one durable receipt"
    );
    assert!(matches!(receipts[0].outcome, OpOutcome::Unknown { .. }));
    cx.read(|cx| {
        let state = app.read(cx);
        let panel = state.op_log.as_ref().unwrap().read(cx);
        assert_eq!(
            panel
                .entries()
                .iter()
                .filter(|entry| entry.id == receipts[0].id
                    && entry.op == "editor-save"
                    && entry.repo == repo.display().to_string())
                .count(),
            1,
            "closed editor panic receipt was not pushed to the live Operation Log"
        );
        assert_eq!(
            state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .filter(|toast| toast.message.as_ref().contains("save task unwound"))
                .count(),
            1,
            "closed editor panic must show exactly one toast"
        );
    });
    acknowledge_panicked_writer(&app, cx);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor_save_panic_after_close");
}

pub fn scenario_snapshot_write_panic(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    KagiApp::panic_next_snapshot_for_e2e();
    cx.dispatch_action(window, kagi::ui::commands::CreateSnapshot);
    cx.run_until_parked();
    assert!(
        crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
            .trim()
            .is_empty(),
        "panicked snapshot did not create a ref"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.app_sessions.reconcile_ids().len(), 1);
        assert!(
            state.app_notice().is_some(),
            "Unknown must offer reconciliation"
        );
        assert!(
            state.app_sessions.has_leases(),
            "unwound snapshot retains its scope until reconciliation"
        );
    });
    acknowledge_panicked_writer(&app, cx);
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS snapshot_write_panic");
}

pub fn scenario_snapshot_write_draws_while_busy(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| assert!(app.open_repository(other_repo, cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    let before_klog = kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.starts_with("[kagi] snapshot: created "))
        .count();
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_snapshot_write_for_e2e(hold);
    cx.dispatch_action(window, kagi::ui::commands::CreateSnapshot);
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.write_busy_op, Some("snapshot"));
        assert!(state.app_sessions.has_leases());
        assert!(kagi::ui::e2e::busy_snackbar_label(state).is_some());
    });
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(
        crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
            .trim()
            .is_empty(),
        "held write must not have run"
    );
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        app.status_footer = FooterStatus::Idle("snapshot other tab sentinel".into());
    });
    release.send(());
    cx.run_until_parked();
    assert!(!crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
        .trim()
        .is_empty());
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert_eq!(
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.starts_with("[kagi] snapshot: created "))
            .count(),
        before_klog + 1,
        "successful snapshot keeps its unchanged contract log after departure"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.as_ref() == "snapshot other tab sentinel"));
        assert!(!state.toast_stack.as_ref().unwrap().read(cx).toasts().iter()
            .any(|toast| toast.message.as_ref() == kagi::ui::i18n::Msg::SnapshotCreated.t()));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS snapshot_write_draws_while_busy");
}

/// An ordinary snapshot failure after leaving its tab must remain visible in
/// the host's notice queue, without changing the replacement tab's footer.
pub fn scenario_snapshot_failed_after_departure(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| assert!(app.open_repository(other_repo, cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    KagiApp::hold_next_snapshot_write_for_e2e(hold);
    cx.dispatch_action(window, kagi::ui::commands::CreateSnapshot);
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        app.status_footer = FooterStatus::Idle("snapshot failure other tab".into());
    });
    // Keep the fixture intact after the worker has observed the absent Git
    // directory; no destructive Git command touches the user's repository.
    std::fs::rename(repo.join(".git"), repo.join(".git-paused")).unwrap();
    release.send(());
    cx.run_until_parked();
    std::fs::rename(repo.join(".git-paused"), repo.join(".git")).unwrap();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(!state.app_sessions.has_leases());
        assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.as_ref() == "snapshot failure other tab"));
        let notice = kagi::ui::e2e::app_notice_message(state)
            .expect("departed snapshot failure must reach the user");
        assert!(notice.contains(repo.to_str().unwrap()), "{notice}");
        assert!(notice.contains("Snapshot failed"), "{notice}");
    });
    assert!(crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
        .trim()
        .is_empty());
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS snapshot_failed_after_departure");
}
