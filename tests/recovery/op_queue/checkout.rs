use super::*;
use kagi_git::oplog::{read_oplog_tail_for_repo, FailureCode, OpOutcome};
use kagi_ui_core::i18n::{queue_text, QueueText};

fn hold_checkout(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    branch: &str,
) -> crate::evidence_support::Reply<()> {
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch(branch, cx));
    cx.run_until_parked();
    release
}

fn assert_static_refusal(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, active: bool) {
    // #1698 keeps the complete blocker in the receipt. A short active-window
    // result belongs in the footer; an inactive one belongs in one notification.
    let expected = match kagi_ui_core::i18n::lang() {
        kagi_ui_core::i18n::Lang::En => "Checkout: The operation could not be completed. Review its details before continuing. See the Operation Log for details.",
        kagi_ui_core::i18n::Lang::Ja => "checkout: 操作を完了できませんでした。続行する前に詳細を確認してください。 詳しくは操作ログを確認してください。",
    };
    cx.read(|cx| {
        let state = app.read(cx);
        if active {
            match &state.status_footer {
                kagi::ui::FooterStatus::Failed(text) => assert_eq!(text.as_ref(), expected),
                other => panic!("expected the bounded checkout refusal, got {other:?}"),
            }
        } else {
            assert!(matches!(&state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.is_empty()));
        }
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert_eq!(
            toasts.len(),
            usize::from(!active),
            "one result surface only"
        );
        if !active {
            let repo_name = state
                .repo_path
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            assert_eq!(
                toasts[0].message.as_ref(),
                format!("{expected} — {repo_name}")
            );
            assert!(matches!(toasts[0].kind, kagi::ui::ToastKind::Error));
        }
    });
}

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
        vec![(
            "checkout blocked".into(),
            queue_text(QueueText::WaitWrite).into()
        )]
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
        vec![(
            "checkout blocked".into(),
            queue_text(QueueText::ReasonPlanError).into()
        )]
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
        vec![("checkout b".into(), queue_text(QueueText::WaitWrite).into())]
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

/// Both idle double-click and queued checkout quietly preserve unrelated staged,
/// unstaged and untracked work; concrete blockers and unknown cautions stop them.
pub fn scenario_queue_busy_checkout_preserves_local_changes(cx: &mut VisualTestAppContext) {
    checkout_untracked_blockers(cx);
    checkout_unknown_cautions(cx);
    checkout_staged_admission(cx);
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
    git(&repo, &["add", "README.md"]);
    std::fs::write(
        repo.join("README.md"),
        "# fixture\nlocal edit\nunstaged edit\n",
    )
    .unwrap();
    std::fs::write(repo.join("queue-untracked.txt"), "keep untracked\n").unwrap();
    let index_before = git_output(&repo, &["ls-files", "--stage"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_none()),
        "idle double-click quietly admits unrelated local work"
    );

    app.update(cx, |app, cx| app.open_plan_modal("b", cx));
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), queue_text(QueueText::WaitWrite).into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    release.send(());
    tick_until(cx, &app, "the safe queued checkout to finish", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(head(&repo), "b");
    assert_eq!(git_output(&repo, &["ls-files", "--stage"]), index_before);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "# fixture\nlocal edit\nunstaged edit\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("queue-untracked.txt")).unwrap(),
        "keep untracked\n"
    );
    assert!(
        klog_index("[kagi] queue: run checkout b (clean plan)").is_some(),
        "a safe carry-over plan must run without a caution modal"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_busy_checkout_preserves_local_changes");
}

fn checkout_unknown_cautions(cx: &mut VisualTestAppContext) {
    use kagi_domain::plan_note::{CommonNote, PlanNote};

    for queued in [false, true] {
        let fixture = branches_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        let release = queued.then(|| hold_checkout(cx, &app, "a"));
        let caution = PlanNote::Common(CommonNote::GitIdentityUnavailable);
        KagiApp::caution_next_checkout_for_e2e(caution.clone());
        app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
        if let Some(release) = release {
            release.send(());
        }
        tick_until(cx, &app, "the unrecognized checkout caution", |app| {
            app.plan_modal().is_some()
        });
        cx.read(|cx| {
            let state = app.read(cx);
            let modal = state.plan_modal().unwrap();
            assert_eq!(modal.queued.is_some(), queued);
            assert_eq!(modal.plan.warnings, vec![caution]);
            assert!(modal.plan.blockers.is_empty());
            assert!(
                !state.app_sessions.has_leases(),
                "confirmation owns no writer"
            );
        });
        assert!(drawn(cx, window, "plan-confirm"));
        let before = crate::macos::repo_fingerprint(&repo);
        let receipts = read_oplog_tail_for_repo(&repo, 100).len();
        assert_eq!(receipts, usize::from(queued), "only the predecessor ran");
        assert_eq!(head(&repo), if queued { "a" } else { "main" });
        advance(cx, 4);
        assert_eq!(crate::macos::repo_fingerprint(&repo), before);
        assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), receipts);
        assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
        click(cx, window, "plan-confirm");
        tick_until(cx, &app, "the explicitly confirmed checkout", |app| {
            app.plan_modal().is_none()
                && !app.app_sessions.has_leases()
                && app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
        });
        assert_eq!(head(&repo), "b");
        let entries = read_oplog_tail_for_repo(&repo, 100);
        assert_eq!(entries.len(), receipts + 1);
        assert!(entries
            .iter()
            .all(|entry| matches!(&entry.outcome, OpOutcome::Success { .. })));
        unmount(cx, app, window);
    }
}

fn checkout_staged_admission(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
        kagi_ui_core::i18n::set_lang(language);
        for queued in [false, true] {
            for path in ["feat.txt/local.txt", "incoming", "README.md"] {
                checkout_staged_blocker_case(cx, path, queued);
            }
            checkout_nonoverlapping_rename(cx, queued);
        }
    }
}

fn staged_checkout_fixture(path: &str, overlap: bool) -> tempfile::TempDir {
    let fixture = branches_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-qb", "staged-target"]);
    std::fs::write(repo.join("feat.txt"), "target file\n").unwrap();
    std::fs::create_dir_all(repo.join("incoming")).unwrap();
    std::fs::write(repo.join("incoming/child.txt"), "target child\n").unwrap();
    let changed = if overlap {
        "README.md"
    } else {
        "README.md-neighbour/child.txt"
    };
    std::fs::create_dir_all(repo.join(changed).parent().unwrap()).unwrap();
    std::fs::write(repo.join(changed), "target bytes\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "staged checkout target"]);
    git(repo, &["checkout", "-q", "main"]);
    let local = if path == "README.md" {
        git(repo, &["mv", "README.md", "renamed-README.md"]);
        "renamed-README.md"
    } else {
        std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        std::fs::write(repo.join(path), "staged bytes\n").unwrap();
        git(repo, &["add", path]);
        path
    };
    // An unstaged rename entry can expose the source separately and mask
    // missing Renamed::from handling. Keep that negative case staged-only.
    if path != "README.md" || !overlap {
        std::fs::write(repo.join(local), "unstaged bytes\n").unwrap();
    }
    fixture
}

fn checkout_staged_blocker_case(cx: &mut VisualTestAppContext, path: &str, queued: bool) {
    use kagi::ui::button_style;
    use kagi_domain::plan_note::{CheckoutNote, PlanNote};

    let fixture = staged_checkout_fixture(path, true);
    let repo = fixture.path().canonicalize().unwrap();
    let initial_oid = rev_parse(&repo, &["HEAD"]);
    let index_before = git_output(&repo, &["ls-files", "--stage"]);
    let diff_before = git_output(&repo, &["diff", "--binary", "HEAD"]);
    let (app, window) = mount(cx, &repo);
    let release = queued.then(|| hold_checkout(cx, &app, "a"));
    app.update(cx, |app, cx| {
        app.dblclick_checkout_branch("staged-target", cx)
    });
    if let Some(release) = release {
        release.send(());
    }
    tick_until(cx, &app, "the staged checkout obstruction", |app| {
        app.plan_modal().is_some()
    });
    cx.read(|cx| {
        let state = app.read(cx);
        let modal = state.plan_modal().unwrap();
        assert_eq!(modal.queued.is_some(), queued);
        assert_eq!(
            modal.plan.blockers,
            vec![PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
                count: 1,
                files: vec![path.into()],
            })]
        );
        assert!(
            !state.app_sessions.has_leases(),
            "no target checkout started"
        );
    });
    assert_eq!(head(&repo), if queued { "a" } else { "main" });
    assert_eq!(rev_parse(&repo, &["HEAD"]), initial_oid);
    assert_eq!(git_output(&repo, &["ls-files", "--stage"]), index_before);
    assert_eq!(
        git_output(&repo, &["diff", "--binary", "HEAD"]),
        diff_before
    );
    let before = crate::macos::repo_fingerprint(&repo);
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(
        receipts.len(),
        usize::from(queued),
        "only the predecessor ran"
    );
    assert!(receipts
        .iter()
        .all(|entry| matches!(&entry.outcome, OpOutcome::Success { .. })));
    button_style::clear_recorded_modal_buttons();
    assert!(drawn(cx, window, &format!("plan-note-path-{path}")));
    assert!(
        button_style::recorded_modal_button("plan-confirm")
            .unwrap()
            .disabled
    );
    click(cx, window, "plan-confirm");
    advance(cx, 4);
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert_eq!(crate::macos::repo_fingerprint(&repo), before);
    assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), receipts.len());
    unmount(cx, app, window);
}

fn checkout_nonoverlapping_rename(cx: &mut VisualTestAppContext, queued: bool) {
    let fixture = staged_checkout_fixture("README.md", false);
    let repo = fixture.path().canonicalize().unwrap();
    let renamed_paths = [
        "ls-files",
        "--stage",
        "--",
        "README.md",
        "renamed-README.md",
    ];
    let index_before = git_output(&repo, &renamed_paths);
    let (app, window) = mount(cx, &repo);
    let release = queued.then(|| hold_checkout(cx, &app, "a"));
    app.update(cx, |app, cx| {
        app.dblclick_checkout_branch("staged-target", cx)
    });
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    if let Some(release) = release {
        release.send(());
    }
    tick_until(
        cx,
        &app,
        "the nonoverlapping staged rename checkout",
        |app| {
            app.plan_modal().is_none()
                && !app.app_sessions.has_leases()
                && app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
        },
    );
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(head(&repo), "staged-target");
    assert_eq!(git_output(&repo, &renamed_paths), index_before);
    assert!(!repo.join("README.md").exists());
    assert_eq!(
        std::fs::read_to_string(repo.join("renamed-README.md")).unwrap(),
        "unstaged bytes\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md-neighbour/child.txt")).unwrap(),
        "target bytes\n"
    );
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(receipts.len(), 1 + usize::from(queued));
    assert!(receipts
        .iter()
        .all(|entry| matches!(&entry.outcome, OpOutcome::Success { .. })));
    unmount(cx, app, window);
}

/// Exercise the idle double-click admission that would otherwise execute a
/// warning-free plan immediately, alongside the shared detached checkout plan.
fn checkout_untracked_blockers(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
        kagi_ui_core::i18n::set_lang(language);
        for (path, content) in [
            ("feat.txt", "untracked local bytes\n"),
            ("feat.txt", "target bytes\n"),
            ("feat.txt/local.txt", "untracked child\n"),
            ("incoming", "untracked file\n"),
        ] {
            checkout_untracked_blocker_case(cx, path, content);
        }
    }
}

fn checkout_untracked_blocker_case(cx: &mut VisualTestAppContext, path: &str, content: &str) {
    use kagi::ui::{button_style, dialog_a11y};
    use kagi_domain::plan_note::{CheckoutNote, PlanNote};
    use kagi_ui_core::i18n::Msg;

    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-qb", "collision"]);
    std::fs::write(repo.join("feat.txt"), "target bytes\n").unwrap();
    std::fs::create_dir_all(repo.join("incoming")).unwrap();
    std::fs::write(repo.join("incoming/child.txt"), "target child\n").unwrap();
    git(&repo, &["add", "feat.txt", "incoming/child.txt"]);
    git(&repo, &["commit", "-qm", "checkout targets"]);
    let target = kagi_git::CommitId(rev_parse(&repo, &["HEAD"]));
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
    std::fs::write(repo.join(path), content).unwrap();
    let before = crate::macos::repo_fingerprint(&repo);
    let index_before = git_output(&repo, &["ls-files", "--stage"]);
    let (app, window) = mount(cx, &repo);

    for (attempt, detached) in [false, true].into_iter().enumerate() {
        app.update(cx, |app, cx| {
            if detached {
                app.open_checkout_commit_modal(target.clone(), cx);
            } else {
                app.dblclick_checkout_branch("collision", cx);
            }
        });
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            let modal = state.plan_modal().expect("collision must stop admission");
            assert_eq!(
                modal.plan.blockers,
                vec![PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
                    count: 1,
                    files: vec![path.into()],
                })]
            );
            assert!(!state.app_sessions.has_leases(), "no checkout admitted");
        });
        assert!(drawn(cx, window, &format!("plan-note-path-{path}")));
        assert_eq!(
            dialog_a11y::recorded_note("plan-blocker-0").unwrap().1,
            Msg::PlanOverlapSummary.t().replace("{}", "1"),
            "the actual blocker summary is localized"
        );
        assert!(
            button_style::recorded_modal_button("plan-confirm")
                .unwrap()
                .disabled
        );
        assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), attempt);
        click(cx, window, "plan-confirm");
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
        // Bypass the disabled button only to prove the existing execution
        // safeguard records Refused rather than starting a failing checkout.
        app.update(cx, |app, cx| app.start_checkout(cx));
        cx.run_until_parked();
        let entries = read_oplog_tail_for_repo(&repo, 100);
        assert_eq!(entries.len(), attempt + 1);
        assert!(entries.iter().all(|entry| matches!(
            &entry.outcome,
            OpOutcome::Refused { blockers } if blockers.iter().any(|note| note.contains(path))
        )));
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
        assert_eq!(crate::macos::repo_fingerprint(&repo), before);
        assert_eq!(git_output(&repo, &["ls-files", "--stage"]), index_before);
        assert_eq!(std::fs::read_to_string(repo.join(path)).unwrap(), content);
    }
    unmount(cx, app, window);
}

/// Missing refs are static and refused with a reason, without a queued plan.
pub fn scenario_queue_checkout_missing_ref_refuses(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let release = hold_checkout(cx, &app, "a");
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    for (attempt, active) in [true, false].into_iter().enumerate() {
        app.update(cx, |app, cx| {
            app.set_window_active_for_e2e(active, cx);
            app.open_plan_modal("missing", cx)
        });
        assert!(strip(cx, &app).is_none());
        let blocker = kagi_domain::plan_note::PlanNote::Common(
            kagi_domain::plan_note::CommonNote::BranchMissing {
                name: "missing".into(),
                in_repo: true,
            },
        );
        assert_static_refusal(cx, &app, active);
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "checkout")
            .collect();
        assert_eq!(entries.len(), attempt + 1);
        assert!(matches!(
            &entries[0].outcome,
            OpOutcome::Refused { blockers } if blockers.as_slice() == [blocker.message_en()]
        ));
        assert!(kagi_ui_core::klog::tail()
            .iter()
            .any(|line| { line.starts_with("[kagi] queue: refused checkout missing (static: ") }));
    }
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
    let release = hold_checkout(cx, &app, "a");
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));

    for (attempt, active) in [true, false].into_iter().enumerate() {
        app.update(cx, |app, cx| {
            app.set_window_active_for_e2e(active, cx);
            app.open_plan_modal("b", cx)
        });
        assert!(
            strip(cx, &app).is_none(),
            "linked branch must not enter the queue"
        );
        assert_static_refusal(cx, &app, active);
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "checkout")
            .collect();
        assert_eq!(entries.len(), attempt + 1);
        assert!(matches!(
            &entries[0].outcome,
            OpOutcome::Refused { blockers } if blockers.as_slice() == [blocker.message_en()]
        ));
        assert!(kagi_ui_core::klog::tail()
            .iter()
            .any(|line| { line.starts_with("[kagi] queue: refused checkout b (static: ") }));
    }
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
            queue_text(QueueText::WaitWrite).into()
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
        for internal in [
            "two writes",
            "2 回",
            "書き込み",
            "queue",
            "キュー",
            "plan",
            "preflight",
        ] {
            assert!(!expected.contains(internal), "{language:?}: {expected}");
        }
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
