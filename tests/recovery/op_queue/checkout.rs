use super::*;
use kagi_git::oplog::{read_oplog_tail_for_repo, FailureCode, OpOutcome};
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
            kagi::ui::FooterStatus::Failed(text) if text.contains(&expected)
        ));
        assert!(state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.contains(&expected)));
    });
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "checkout")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0].outcome,
        OpOutcome::Refused { blockers } if blockers.len() == 1
            && blockers[0].contains("missing")
    ));
    assert!(kagi_ui_core::klog::tail()
        .iter()
        .any(|line| { line.starts_with("[kagi] queue: refused checkout missing (static: ") }));
    release.send(());
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_checkout_missing_ref_refuses");
}

/// A branch owned by a linked worktree is refused before entering the queue,
/// with the typed refusal durably recorded even while another write is held.
pub fn scenario_queue_checkout_linked_worktree_refuses(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        &repo,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "b"],
    );
    let linked = linked.canonicalize().unwrap();
    let blocker = kagi_domain::plan_note::PlanNote::Worktree(
        kagi_domain::plan_note::WorktreeNote::BranchInOtherWorktree {
            branch: "b".into(),
            path: linked.display().to_string(),
        },
    );
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();
    assert!(cx.read(|cx| app
        .read(cx)
        .view()
        .worktrees
        .iter()
        .any(|worktree| { worktree.branch.as_deref() == Some("b") && !worktree.is_current })));
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| app.open_plan_modal("b", cx));
    assert!(
        strip(cx, &app).is_none(),
        "linked branch must not enter the queue"
    );
    let expected = kagi::ui::i18n::plan_note_text(&blocker);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(
            &state.status_footer,
            kagi::ui::FooterStatus::Failed(text) if text.contains(&expected)
        ));
        assert!(state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.contains(&expected)));
    });
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "checkout")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        &entries[0].outcome,
        OpOutcome::Refused { blockers } if blockers.as_slice() == [blocker.message_en()]
    ));
    assert!(kagi_ui_core::klog::tail()
        .iter()
        .any(|line| { line.starts_with("[kagi] queue: refused checkout b (static: ") }));
    release.send(());
    cx.run_until_parked();
    assert_eq!(head(&repo), "a");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_checkout_linked_worktree_refuses");
}

/// Direct commit-modal entry while a write is held freezes the OID. The next
/// plan reads the new HEAD and its approval still has a live preflight.
pub fn scenario_queue_commit_checkout_busy_replans(cx: &mut VisualTestAppContext) {
    use kagi::ui::modals::CheckoutPlanTarget;

    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let target = kagi_git::CommitId(rev_parse(&repo, &["HEAD~1"]));
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    app.update(cx, |app, cx| {
        app.open_checkout_commit_modal(target.clone(), cx)
    });
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![(
            format!("checkout {}", target.short()),
            "waiting: write".into()
        )]
    );
    app.update(cx, |app, _| app.select_headless(0));
    release.send(());
    tick_until(cx, &app, "fresh detached checkout plan", |app| {
        app.plan_modal().is_some_and(|modal| modal.queued.is_some())
    });
    cx.read(|cx| {
        let modal = app.read(cx).plan_modal().unwrap();
        assert!(matches!(
            &modal.target,
            CheckoutPlanTarget::Commit(frozen) if frozen == &target
        ));
        assert_eq!(modal.plan.current.head, "branch: a");
        assert!(modal.plan.blockers.is_empty());
        assert!(
            !modal.plan.warnings.is_empty(),
            "detached HEAD requires approval"
        );
    });
    assert_eq!(head(&repo), "a", "confirmation precedes the checkout");

    // Change HEAD after the displayed fresh plan: approval must be refused at
    // preflight, rather than checking out the frozen commit with a stale plan.
    git(&repo, &["checkout", "-q", "b"]);
    click(cx, window, "plan-confirm");
    tick_until(cx, &app, "stale approval failure", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
            && !app.app_sessions.has_leases()
    });
    assert_eq!(head(&repo), "b");
    let entry = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .find(|entry| entry.op == "checkout-commit")
        .expect("stale approval has a durable checkout-commit receipt");
    assert_eq!(entry.failure_code, Some(FailureCode::Preflight));
    assert!(matches!(entry.outcome, OpOutcome::Failed { .. }));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_checkout_busy_replans");
}

/// Enter on a dirty selected commit cannot queue the stash + checkout pair
/// behind a write; the actual native action explains it in both languages.
pub fn scenario_queue_dirty_commit_enter_refuses(cx: &mut VisualTestAppContext) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_ui_core::i18n::{self, Lang, Msg};

    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_lang = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let fixture = branches_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let initial_head = rev_parse(&repo, &["HEAD"]);
        std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
        let (app, window) = mount(cx, &repo);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_run_for_e2e(hold);
        app.update(cx, |app, cx| app.open_plan_modal("a", cx));
        app.update(cx, |app, cx| app.start_checkout(cx));
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
        app.update(cx, |app, _| app.select_headless(1));
        assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));
        crate::recovery_operations::press_enter(cx, &app, window);
        cx.run_until_parked();

        let expected = Msg::CheckoutStashCannotQueue.t();
        assert_ne!(expected, Msg::OpInProgress.t());
        assert!(expected.contains("stash + checkout"));
        assert!(expected.contains(match language {
            Lang::En => "two writes",
            Lang::Ja => "2 回",
        }));
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(matches!(
                &state.status_footer,
                FooterStatus::Idle(text) if text.as_ref() == expected
            ));
            assert!(state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| matches!(toast.kind, ToastKind::Error)
                    && toast.message.as_ref() == expected));
            assert!(
                state.plan_modal().is_none(),
                "no stash-first plan during a write"
            );
        });
        assert!(
            strip(cx, &app).is_none(),
            "two writes cannot enter one queue intent"
        );
        assert_eq!(rev_parse(&repo, &["HEAD"]), initial_head);
        assert_eq!(git_output(&repo, &["stash", "list"]), "");
        release.send(());
        tick_until(cx, &app, "preceding checkout to finish", |app| {
            !app.app_sessions.has_leases()
        });
        assert_eq!(head(&repo), "a");
        assert_eq!(git_output(&repo, &["stash", "list"]), "");
        assert_eq!(
            std::fs::read_to_string(repo.join("README.md")).unwrap(),
            "# fixture\nlocal edit\n"
        );
        unmount(cx, app, window);
    }
    i18n::set_lang(original_lang);
    eprintln!("[gui-e2e] PASS queue_dirty_commit_enter_refuses EN/JA");
}
