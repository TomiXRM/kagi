use super::*;
/// A blocker derived from the tree during another checkout is not an
/// enqueue-time veto. The fresh head plan still refuses it before any write.
pub fn scenario_queue_refuses_a_blocked_checkout(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-q", "-b", "blocked"]);
    std::fs::write(repo.join("README.md"), "# fixture\nfrom blocked\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "blocked edit"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.open_plan_modal("a", cx));
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| app.open_plan_modal("blocked", cx));
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout blocked".into(), "waiting: write".into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    release.send(());
    tick_until(cx, &app, "the blocked checkout to replan", |app| {
        app.plan_modal().is_some_and(|modal| modal.queued.is_some())
    });
    assert!(cx.read(|cx| { !app.read(cx).plan_modal().unwrap().plan.blockers.is_empty() }));
    let before = rev_parse(&repo, &["HEAD"]);
    app.update(cx, |app, cx| app.start_checkout(cx));
    tick_until(cx, &app, "the blocked checkout to be refused", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|(_, rows, cancelled)| rows.is_empty() && cancelled.len() == 1)
    });
    assert_eq!(
        strip(cx, &app).unwrap().2,
        vec![("checkout blocked".into(), "plan failed".into())]
    );
    assert!(klog_index("[kagi] refused: plan has blockers, not executing").is_some());
    assert_eq!(rev_parse(&repo, &["HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "# fixture\nlocal edit\n"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_refuses_a_blocked_checkout");
}

/// The predecessor commits the staged overlap; the queued checkout must not
/// inherit the pre-commit blocker, and the fresh clean plan can execute.
pub fn scenario_queue_checkout_transient_blocker_clears(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-q", "b"]);
    std::fs::write(repo.join("README.md"), "# fixture\nbranch b\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "edit on b"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "# fixture\nstaged on main\n").unwrap();
    git(&repo, &["add", "README.md"]);
    assert!(
        !kagi_git::Backend::open(&repo)
            .unwrap()
            .plan_checkout("b")
            .unwrap()
            .blockers
            .is_empty(),
        "the enqueue-time staged overlap is blocked"
    );
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    queue_commit(cx, &app, window, "clear staged overlap", "");
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| app.open_plan_modal("b", cx));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), "waiting: write".into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    release.send(());
    tick_until(cx, &app, "the queued checkout to finish", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    assert_eq!(
        git_output(&repo, &["show", "main:README.md"]),
        "# fixture\nstaged on main"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "# fixture\nbranch b\n"
    );
    assert!(
        klog_index("[kagi] queue: run checkout b (clean plan)").is_some(),
        "the fresh checkout plan has no blockers"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_checkout_transient_blocker_clears");
}

/// Non-busy opens the ordinary plan now. Busy skips dirty inspection, shows
/// the strip immediately, then the head's fresh carry-over warning asks.
pub fn scenario_queue_busy_checkout_replans_warning(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_plan_modal("a", cx));
    assert!(cx.read(|cx| app
        .read(cx)
        .plan_modal()
        .is_some_and(|m| m.queued.is_none())));
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| app.open_plan_modal("b", cx));
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), "waiting: write".into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    release.send(());
    tick_until(cx, &app, "the queued warning modal", |app| {
        app.plan_modal().is_some_and(|modal| modal.queued.is_some())
    });
    cx.read(|cx| {
        let modal = app.read(cx).plan_modal().unwrap();
        assert!(modal.plan.blockers.is_empty());
        assert!(!modal.plan.warnings.is_empty(), "fresh carry-over warning");
    });
    assert_eq!(head(&repo), "a", "checkout b still needs confirmation");
    click(cx, window, "plan-cancel");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_busy_checkout_replans_warning");
}

/// Missing refs are static and refused with a reason, without a queued plan.
pub fn scenario_queue_checkout_missing_ref_refuses(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| app.open_plan_modal("missing", cx));
    assert!(strip(cx, &app).is_none());
    let expected = kagi::ui::i18n::plan_note_text(&kagi_domain::plan_note::PlanNote::Common(
        kagi_domain::plan_note::CommonNote::BranchMissing {
            name: "missing".into(),
            in_repo: true,
        },
    ));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(
            &state.status_footer,
            kagi::ui::FooterStatus::Idle(text) if text.as_ref() == expected
        ));
        assert!(state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.as_ref() == expected));
    });
    release.send(());
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_checkout_missing_ref_refuses");
}
