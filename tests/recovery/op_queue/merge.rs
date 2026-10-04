use super::*;

/// Feature changes a separate file; main can make an independent commit.
fn merge_fixture() -> tempfile::TempDir {
    let fixture = branches_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
    git(repo, &["add", "feature.txt"]);
    git(repo, &["commit", "-q", "-m", "feature change"]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["branch", "other"]);
    fixture
}

fn await_merge_modal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    tick_until(cx, app, "queued merge confirmation", |app| {
        app.merge_modal().is_some_and(|m| m.queued.is_some())
    });
}

fn await_merge_settlement(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    tick_until(cx, app, "merge settlement", |app| {
        !app.app_sessions.has_leases()
            && app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
}

/// Q5: a commit changes HEAD after the merge was queued; its live plan must
/// change from fast-forward to a two-parent merge.
pub fn scenario_queue_replans_merge_after_predecessor(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    std::fs::write(repo.join("predecessor.txt"), "first\n").unwrap();
    git(&repo, &["add", "predecessor.txt"]);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    queue_commit(cx, &app, window, "predecessor", "");
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("merge feature → main".into(), "waiting: write".into())]
    );
    assert!(!cx.read(|cx| e2e::active_modal_present(app.read(cx))));
    click(cx, window, "queue-strip");
    release.send(());
    await_merge_modal(cx, &app);
    let finished = klog_index("[kagi] async: commit finished").expect("predecessor finished");
    let planned = klog_index("[kagi] queue: plan merge feature → main blockers=0 warnings=0")
        .expect("merge replan");
    assert!(
        finished < planned,
        "live plan follows predecessor settlement"
    );
    cx.read(|cx| {
        let modal = app.read(cx).merge_modal().unwrap();
        assert_eq!(
            modal.kind,
            kagi_git::MergeKind::MergeCommit,
            "main's new commit diverges from feature; enqueue-time plan was fast-forward"
        );
        assert_eq!(modal.plan.current.head, "branch: main");
        assert!(!modal.off_branch);
    });
    let predecessor = rev_parse(&repo, &["HEAD"]);
    click(cx, window, "plan-confirm");
    await_merge_settlement(cx, &app);
    let merged = rev_parse(&repo, &["HEAD"]);
    assert_ne!(merged, predecessor);
    assert_eq!(rev_parse(&repo, &["HEAD^1"]), predecessor);
    assert_eq!(
        rev_parse(&repo, &["HEAD^2"]),
        rev_parse(&repo, &["feature"])
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_replans_merge_after_predecessor");
}

/// Q10: withdrawing A's confirmation by tab departure is not a rejection;
/// B's queued checkout proceeds and A replans upon return.
pub fn scenario_queue_confirm_departure_requeues(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let other = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), Some("main".into()), cx)
    });
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    release.send(());
    await_merge_modal(cx, &app);
    let queued = cx.read(|cx| app.read(cx).merge_modal().unwrap().queued);
    assert!(queued.is_some());
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    tick_until(cx, &app, "B checkout after A's modal departure", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
            && !app.app_sessions.has_leases()
    });
    assert_eq!(head(other.path()), "b");
    assert!(cx.read(|cx| app.read(cx).merge_modal().is_none()));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    tick_until(cx, &app, "A queued intent on return", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_some()
    });
    let shown = strip(cx, &app).unwrap();
    assert!(shown.2.is_empty(), "departure must not cancel A");
    assert!(shown
        .1
        .iter()
        .any(|(label, _)| label == "merge feature → main"));
    await_merge_modal(cx, &app);
    assert_eq!(
        cx.read(|cx| app.read(cx).merge_modal().unwrap().queued),
        queued
    );
    click(cx, window, "plan-cancel");
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_confirm_departure_requeues");
}

/// A conflicting HEAD merge succeeds in entering Conflict Mode but creates no
/// commit, so its evidence is Unverified and successors cannot run.
pub fn scenario_queue_merge_conflict_trips_successors(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("README.md"), "feature\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "feature edit"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main edit"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    std::fs::write(repo.join("anchor.txt"), "predecessor\n").unwrap();
    git(&repo, &["add", "anchor.txt"]);
    queue_commit(cx, &app, window, "predecessor", "");
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_some()),
        "dirty staged index asks before queuing checkout"
    );
    app.update(cx, |app, cx| app.start_checkout(cx));
    assert_eq!(
        rows(&strip(cx, &app)).len(),
        2,
        "both merge and checkout are queued"
    );
    click(cx, window, "queue-strip");
    release.send(());
    await_merge_modal(cx, &app);
    cx.read(|cx| {
        assert!(matches!(
            app.read(cx).merge_modal().unwrap().kind,
            kagi_git::MergeKind::Conflicts(_)
        ));
    });
    let before = rev_parse(&repo, &["HEAD"]);
    click(cx, window, "plan-confirm");
    tick_until(cx, &app, "conflict trips checkout", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 1)
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), before);
    assert!(repo.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        strip(cx, &app).unwrap().2,
        vec![("checkout b".into(), "previous step failed".into())]
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_merge_conflict_trips_successors");
}

/// The destination is frozen at enqueue even when the preceding checkout
/// makes HEAD point at another branch.
pub fn scenario_queue_merge_into_keeps_frozen_target(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_before = rev_parse(&repo, &["other"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("other", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    assert_eq!(rows(&strip(cx, &app))[0].0, "merge feature → main");
    release.send(());
    await_merge_modal(cx, &app);
    cx.read(|cx| {
        let modal = app.read(cx).merge_modal().unwrap();
        assert!(modal.off_branch);
        assert_eq!(modal.into_branch, "main");
    });
    click(cx, window, "plan-confirm");
    await_merge_settlement(cx, &app);
    assert_eq!(head(&repo), "other");
    assert_eq!(rev_parse(&repo, &["other"]), other_before);
    assert_eq!(rev_parse(&repo, &["main"]), rev_parse(&repo, &["feature"]));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_merge_into_keeps_frozen_target");
}

/// Drag's busy entry must enqueue instead of refusing without touching refs.
pub fn scenario_queue_drag_merge_while_busy(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.start_merge_from_drag("feature".into(), cx)
    });
    cx.run_until_parked();
    assert_eq!(rows(&strip(cx, &app))[0].0, "merge feature → main");
    release.send(());
    await_merge_modal(cx, &app);
    assert!(cx.read(|cx| app.read(cx).merge_modal().unwrap().off_branch));
    click(cx, window, "plan-cancel");
    tick_until(cx, &app, "drag merge decline", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 1)
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_drag_merge_while_busy");
}

/// A busy sidebar menu must leave Merge clickable and route it to the queue.
pub fn scenario_queue_menu_merge_while_busy(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, _| {
        app.open_local_branch_menu("feature".into(), gpui::point(gpui::px(40.), gpui::px(200.)));
    });
    click(cx, window, "branch-menu-item-2-0");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_none()));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("merge feature → main".into(), "waiting: write".into())]
    );
    release.send(());
    await_merge_modal(cx, &app);
    click(cx, window, "plan-cancel");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_menu_merge_while_busy");
}

fn last_toast(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<String> {
    cx.read(|cx| {
        let stack = app.read(cx).toast_stack.as_ref()?;
        stack
            .read(cx)
            .toasts()
            .last()
            .map(|toast| toast.message.to_string())
    })
}

/// Opus review P1 on #1024: Enter still reaches a queued merge whose replan
/// is blocked (the button is hidden). It must be refused and leave the queue,
/// never sit in `Admitting` forever.
pub fn scenario_queue_confirm_blocked_merge_refuses(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    tick_until(cx, &app, "direct merge plan", |app| {
        app.merge_modal().is_some_and(|m| m.queued.is_none())
    });
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.start_merge(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    // The held merge takes `feature` in: the queued one replans as blocked.
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    assert_eq!(rows(&strip(cx, &app))[0].0, "merge feature → main");
    release.send(());
    await_merge_modal(cx, &app);
    let blockers = cx.read(|cx| app.read(cx).merge_modal().unwrap().plan.blockers.len());
    assert!(blockers > 0, "precondition: the replan is blocked");
    let merged = rev_parse(&repo, &["HEAD"]);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "enter");
    tick_until(cx, &app, "the blocked merge to leave the queue", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 1)
    });
    assert_eq!(
        strip(cx, &app).unwrap().2,
        vec![("merge feature → main".into(), "plan failed".into())]
    );
    assert!(klog_index("[kagi] refused: merge plan has blockers, not executing").is_some());
    assert!(cx.read(|cx| app.read(cx).merge_modal().is_none()));
    assert_eq!(rev_parse(&repo, &["HEAD"]), merged);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_confirm_blocked_merge_refuses");
}

/// Opus review P2: a merge plan that finishes after its owner tab left —
/// before any tick told the queue — is discarded: nothing of A's opens or is
/// reported on tab B, and A replans and asks again upon return.
pub fn scenario_queue_merge_plan_after_departure(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let other = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), Some("main".into()), cx)
    });
    let (plan_hold, plan_release) = deferred::<()>(cx);
    KagiApp::hold_next_queued_merge_plan_for_e2e(plan_hold);
    release.send(());
    tick_until(cx, &app, "the merge replan", |app| {
        rows(&app.queue_strip_for_e2e(std::time::Instant::now()))
            == vec![("merge feature → main".to_string(), "planning".to_string())]
    });
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    plan_release.send(());
    cx.run_until_parked();
    assert!(klog_index("[kagi] queue: plan discarded merge feature → main").is_some());
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.merge_modal().is_none(), "A's merge opened on tab B");
        assert_eq!(
            e2e::app_notice_message(state),
            None,
            "nothing reported on B"
        );
    });
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    await_merge_modal(cx, &app);
    assert!(strip(cx, &app).unwrap().2.is_empty());
    click(cx, window, "plan-cancel");
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_merge_plan_after_departure");
}

/// Opus review P2: with intents queued but nothing running, a merge that
/// cannot be queued says why instead of doing nothing.
pub fn scenario_queue_merge_refusal_is_shown(cx: &mut VisualTestAppContext) {
    let fixture = merge_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    // Local changes: b's replan carries a warning and waits for an answer.
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    release.send(());
    tick_until(cx, &app, "b's confirmation", |app| {
        app.plan_modal().and_then(|m| m.queued).is_some()
    });
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    git(&repo, &["checkout", "-q", "--detach"]);
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx)
    });
    let expected = kagi::ui::i18n::plan_note_text(&kagi_domain::plan_note::PlanNote::Common(
        kagi_domain::plan_note::CommonNote::HeadDetached {
            op: kagi_domain::plan_note::PlanOp::Merge,
        },
    ));
    assert_eq!(last_toast(cx, &app), Some(expected.clone()));
    cx.read(|cx| {
        assert!(matches!(
            &app.read(cx).status_footer,
            kagi::ui::FooterStatus::Idle(text) if text.as_ref() == expected
        ));
    });
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".to_string(), "confirming".to_string())]
    );
    app.update(cx, |app, _| app.cancel_modal());
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_merge_refusal_is_shown");
}
