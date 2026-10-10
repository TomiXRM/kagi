use super::*;

/// Use the real Commit Panel inputs and plan button, not a synthetic queue event.
pub(super) fn queue_commit(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    title: &str,
    body: &str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        let panel = app
            .read(cx)
            .ui()
            .commit_panel
            .clone()
            .expect("commit panel");
        let (title_input, body_input) = {
            let panel = panel.read(cx);
            (
                panel.title_input.clone().expect("title input"),
                panel.body_input.clone().expect("body input"),
            )
        };
        window.focus(&title_input.read(cx).focus_handle(cx), cx);
        title_input.update(cx, |input, cx| input.set_value(title, window, cx));
        body_input.update(cx, |input, cx| input.set_value(body, window, cx));
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        let owner = app.active_session().expect("commit owner");
        app.open_commit_plan_modal(owner, cx);
    });
    cx.run_until_parked();
}

fn change_commit_title(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    title: &str,
) {
    cx.update_window(window, |_, window, cx| {
        let panel = app
            .read(cx)
            .ui()
            .commit_panel
            .clone()
            .expect("commit panel");
        let input = panel.read(cx).title_input.clone().expect("title input");
        input.update(cx, |input, cx| input.set_value(title, window, cx));
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn focus_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().expect("root focus");
        window.focus(&root, cx);
        assert!(root.is_focused(window), "root owns focus after blur");
        window.draw(cx).clear();
    })
    .unwrap();
}

fn frozen_modal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    frozen: &str,
) {
    tick_until(cx, app, "queued commit confirmation", |app| {
        app.queued_commit_message_for_e2e().is_some()
    });
    assert_eq!(
        cx.read(|cx| app.read(cx).queued_commit_message_for_e2e()),
        Some(frozen.to_string()),
        "confirmation shows the entire frozen subject and body"
    );
    assert!(drawn(cx, window, "queued-commit-message"));
    assert!(drawn(cx, window, "plan-confirm"));
}

fn finish_queued_commit(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    tick_until(cx, app, "queued commit to settle", |app| {
        !app.app_sessions.has_leases()
            && app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
}

/// A clean queued commit uses the message and staged content selected behind
/// the held checkout, executes on its resulting branch, and verifies its HEAD.
pub fn scenario_queue_commit_runs_after_checkout(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = rev_parse(&repo, &["HEAD"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    std::fs::write(repo.join("first.txt"), "staged for a\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "frozen subject", "frozen body");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("commit frozen subject".into(), "waiting: write".into())]
    );
    assert!(drawn(cx, window, "queue-strip"));
    cx.read(|cx| {
        let toasts = app.read(cx).toast_stack.as_ref().unwrap().read(cx);
        assert!(toasts
            .toasts()
            .iter()
            .any(|toast| toast.message.as_ref() == "Queued: commit frozen subject"));
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), before);
    release.send(());
    finish_queued_commit(cx, &app);
    assert_eq!(head(&repo), "a");
    assert_ne!(rev_parse(&repo, &["HEAD"]), before);
    assert_eq!(
        rev_parse(&repo, &["HEAD^{tree}:first.txt"]),
        rev_parse(&repo, &[":first.txt"])
    );
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "frozen subject\n\nfrozen body"
    );
    assert!(cx.read(|cx| app.read(cx).queued_commit_message_for_e2e().is_none()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_runs_after_checkout");
}

/// Q5b: editing the draft after queueing must ask, and approval must use the
/// old message while preserving the new text in the panel.
pub fn scenario_queue_commit_confirms_changed_draft(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "original subject", "original body");
    change_commit_title(cx, &app, window, "new draft");
    focus_root(cx, &app, window);
    release.send(());
    frozen_modal(cx, &app, window, "original subject\n\noriginal body");
    assert_eq!(head(&repo), "a");
    assert_eq!(rows(&strip(cx, &app))[0].1, "confirming");
    click(cx, window, "plan-confirm");
    finish_queued_commit(cx, &app);
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "original subject\n\noriginal body"
    );
    cx.read(|cx| {
        let state = app.read(cx);
        let panel = state
            .ui()
            .commit_panel
            .as_ref()
            .expect("draft panel")
            .read(cx);
        assert_eq!(
            panel
                .title_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string(),
            "new draft",
            "confirming the frozen intent cannot consume the newer draft"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_confirms_changed_draft");
}

/// The staged-set digest is the frozen identity: a newly staged path asks for
/// approval even with an unchanged message and a clean live plan.
pub fn scenario_queue_commit_confirms_changed_staging(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "same subject", "same body");
    std::fs::write(repo.join("second.txt"), "second\n").unwrap();
    git(&repo, &["add", "second.txt"]);
    focus_root(cx, &app, window);
    release.send(());
    frozen_modal(cx, &app, window, "same subject\n\nsame body");
    assert_eq!(head(&repo), "a");
    assert_eq!(rows(&strip(cx, &app))[0].1, "confirming");
    click(cx, window, "plan-confirm");
    finish_queued_commit(cx, &app);
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "same subject\n\nsame body"
    );
    assert_eq!(
        rev_parse(&repo, &["HEAD^{tree}:second.txt"]),
        rev_parse(&repo, &[":second.txt"])
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_confirms_changed_staging");
}

/// The queued commit consumes the draft from enqueue's branch, not the
/// destination branch reached by the checkout ahead of it.
pub fn scenario_queue_commit_consumes_origin_draft(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "origin subject", "origin body");
    kagi_git::save_draft(&repo, "main", "origin subject\n\norigin body", "plain").unwrap();
    kagi_git::save_draft(&repo, "a", "destination draft", "plain").unwrap();
    release.send(());
    finish_queued_commit(cx, &app);
    assert_eq!(head(&repo), "a");
    assert_eq!(
        git_output(&repo, &["log", "-1", "--format=%B"]),
        "origin subject\n\norigin body"
    );
    assert!(
        kagi_git::load_draft(&repo, "main").is_none(),
        "consumed main's draft"
    );
    assert_eq!(
        kagi_git::load_draft(&repo, "a").unwrap().message,
        "destination draft",
        "a's unrelated draft remains untouched"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_consumes_origin_draft");
}

/// A second `git add` after confirmation opens cannot silently join the
/// already displayed plan; cancel the replacement rather than approving it.
pub fn scenario_queue_commit_rechecks_staging_on_confirm(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = rev_parse(&repo, &["HEAD"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "staging guard", "");
    std::fs::write(repo.join("second.txt"), "second\n").unwrap();
    git(&repo, &["add", "second.txt"]);
    release.send(());
    frozen_modal(cx, &app, window, "staging guard");
    assert!(drawn(cx, window, "queued-commit-staged-changed"));
    std::fs::write(repo.join("other.txt"), "external\n").unwrap();
    git(&repo, &["add", "other.txt"]);
    click(cx, window, "plan-confirm");
    cx.run_until_parked();
    assert_eq!(
        rev_parse(&repo, &["HEAD"]),
        before,
        "stale approval cannot commit other.txt"
    );
    assert_eq!(
        git_output(&repo, &["status", "--short", "other.txt"]),
        "A  other.txt"
    );
    assert_eq!(
        strip(cx, &app).unwrap().2,
        vec![("commit staging guard".into(), "plan failed".into())],
        "the confirmation became stale before admission, not during execution"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_rechecks_staging_on_confirm");
}

/// An externally started merge after queue confirmation must retain its
/// MERGE_HEAD and not make a single-parent commit.
pub fn scenario_queue_commit_refuses_late_merge(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["checkout", "-q", "-b", "merge-source"]);
    std::fs::write(repo.join("merge-source.txt"), "theirs\n").unwrap();
    git(&repo, &["add", "merge-source.txt"]);
    git(&repo, &["commit", "-q", "-m", "merge source"]);
    git(&repo, &["checkout", "-q", "main"]);
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "must not commit merge", "");
    std::fs::write(repo.join("second.txt"), "second\n").unwrap();
    git(&repo, &["add", "second.txt"]);
    release.send(());
    frozen_modal(cx, &app, window, "must not commit merge");
    let before = rev_parse(&repo, &["HEAD"]);
    // Git requires a clean index to start the external merge; keep the
    // queued draft's files as untracked work rather than deleting them.
    git(&repo, &["restore", "--staged", "first.txt", "second.txt"]);
    git(&repo, &["merge", "--no-commit", "--no-ff", "merge-source"]);
    let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
    click(cx, window, "plan-confirm");
    tick_until(cx, &app, "merge cancellation", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now())
            .is_some_and(|strip| strip.2.iter().any(|(_, reason)| reason == "merge started"))
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), before);
    assert_eq!(
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_refuses_late_merge");
}

/// A detached checkout followed by a commit is verified by new HEAD OID,
/// allowing the next queued checkout through the same success chain.
pub fn scenario_queue_commit_detached_successor(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let target = kagi_git::CommitId(rev_parse(&repo, &["HEAD~1"]));
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_checkout_commit_modal(target, cx));
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "detached\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "detached commit", "");
    app.update(cx, |app, cx| app.open_plan_modal("b", cx));
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    release.send(());
    tick_until(cx, &app, "detached checkout confirmation", |app| {
        app.plan_modal().is_some_and(|modal| modal.queued.is_some())
    });
    click(cx, window, "plan-confirm");
    frozen_modal(cx, &app, window, "detached commit");
    click(cx, window, "plan-confirm");
    tick_until(
        cx,
        &app,
        "successor checkout after detached commit",
        |app| {
            app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
                && !app.app_sessions.has_leases()
        },
    );
    assert_eq!(
        head(&repo),
        "b",
        "verified detached commit permits queued successor"
    );
    assert_eq!(
        git_output(&repo, &["show", "-s", "--format=%B", "HEAD@{1}"]),
        "detached commit"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_commit_detached_successor");
}

/// A focused Input holds the queue even after the preceding write settles.
/// The next observation after a real blur admits the waiting head.
pub fn scenario_queue_waits_while_input_focused(cx: &mut VisualTestAppContext) {
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.dblclick_checkout_branch("b", cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        let panel = app.read(cx).ui().commit_panel.clone().unwrap();
        let input = panel.read(cx).title_input.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        input.update(cx, |input, cx| input.set_value("still typing", window, cx));
        window.draw(cx).clear();
        assert!(
            window.has_focused_input(cx),
            "the real Commit Input owns focus"
        );
        let root = app.read(cx).root_focus.clone().expect("root focus");
        assert!(
            root.contains(&input.read(cx).focus_handle(cx), window),
            "visible Commit Input belongs to the drawn dispatch tree"
        );
    })
    .unwrap();
    advance(cx, 2);
    release.send(());
    tick_until(cx, &app, "first checkout to settle", |app| {
        !app.app_sessions.has_leases()
    });
    assert!(
        cx.read(|cx| app.read(cx).ui().commit_panel.is_some()),
        "the input remains drawn after checkout reload"
    );
    advance(cx, 4);
    assert_eq!(head(&repo), "a", "b must not run while typing");
    assert_eq!(
        rows(&strip(cx, &app)),
        vec![("checkout b".into(), "waiting: typing".into())]
    );
    assert!(!cx.read(|cx| e2e::active_modal_present(app.read(cx))));
    focus_root(cx, &app, window);
    tick_until(cx, &app, "checkout b after input blur", |app| {
        app.queue_strip_for_e2e(std::time::Instant::now()).is_none()
    });
    assert_eq!(head(&repo), "b");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS queue_waits_while_input_focused");
}

/// #1126: both immediate Commit approval and the queued confirmation retain
/// the approved index even when another git add happens before execution.
pub fn scenario_commit_index_identity_refusal(cx: &mut VisualTestAppContext) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_domain::plan_note::{CommitNote, PlanNote};
    use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
    use kagi_ui_core::i18n::{self, Lang, Msg};

    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        commit_planning_refusal(cx);
        commit_identity_unavailable_footer(cx, lang);
        for queued in [false, true] {
            let fixture = branches_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            let (hold, release) = deferred::<()>(cx);
            KagiApp::hold_next_run_for_e2e(hold);
            if queued {
                app.update(cx, |app, cx| app.dblclick_checkout_branch("a", cx));
                cx.run_until_parked();
            }
            std::fs::write(repo.join("approved.txt"), "APPROVED\n").unwrap();
            git(&repo, &["add", "approved.txt"]);
            queue_commit(cx, &app, window, "approved identity", "");
            // Force the queued head to present its existing staging-change
            // confirmation, then hold the resulting commit after approval.
            let release = if queued {
                std::fs::write(repo.join("another.txt"), "also approved\n").unwrap();
                git(&repo, &["add", "another.txt"]);
                release.send(());
                frozen_modal(cx, &app, window, "approved identity");
                let (hold, release) = deferred::<()>(cx);
                KagiApp::hold_next_run_for_e2e(hold);
                click(cx, window, "plan-confirm");
                cx.run_until_parked();
                release
            } else {
                release
            };
            let head = rev_parse(&repo, &["HEAD"]);
            std::fs::write(repo.join("approved.txt"), "NOT_APPROVED\n").unwrap();
            git(&repo, &["add", "approved.txt"]);
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            release.send(());
            tick_until(cx, &app, "stale commit refusal", |app| {
                !app.app_sessions.has_leases() && app.write_busy_op.is_none()
            });
            let reason = Msg::AdviceCommitStagedContentChanged.t();
            assert_eq!(
                reason,
                i18n::plan_note_text(&PlanNote::Commit(CommitNote::StagedContentChanged))
            );
            cx.read(|cx| {
                let app = app.read(cx);
                assert!(matches!(&app.status_footer, FooterStatus::Failed(message) if message.contains(reason)),
                    "localized reason must reach the footer: {:?}", app.status_footer);
                let toast = app.toast_stack.as_ref().unwrap().read(cx).toasts().last().unwrap();
                assert!(matches!(toast.kind, ToastKind::Error) && toast.message.contains(reason),
                    "localized reason must reach the bounded error toast: {}", toast.message);
            });
            assert_eq!(rev_parse(&repo, &["HEAD"]), head);
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read_to_string(repo.join("approved.txt")).unwrap(),
                "NOT_APPROVED\n"
            );
            let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "commit")
                .collect();
            assert_eq!(receipts.len(), 1);
            assert!(
                matches!(&receipts[0].outcome, OpOutcome::Failed { error } if error == &CommitNote::StagedContentChanged.message_en())
            );
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
    eprintln!("[gui-e2e] PASS commit_index_identity_refusal");
}

/// #1132: a real rejecting hook reaches the localized failure surfaces.
#[cfg(unix)]
pub fn scenario_commit_hook_failure(cx: &mut VisualTestAppContext) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
    use kagi_ui_core::i18n::{self, Lang};
    use std::os::unix::fs::PermissionsExt;
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let fixture = branches_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        git(&repo, &["config", "core.hooksPath", ".git/hooks"]);
        let hook = repo.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\necho project-policy-rejected >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(repo.join("approved.txt"), "approved\n").unwrap();
        git(&repo, &["add", "approved.txt"]);
        let head = rev_parse(&repo, &["HEAD"]);
        let (app, window) = mount(cx, &repo);
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        queue_commit(cx, &app, window, "hook policy", "");
        tick_until(cx, &app, "hook failure settlement", |app| {
            !app.app_sessions.has_leases() && app.write_busy_op.is_none()
        });
        cx.read(|cx| {
            let app = app.read(cx);
            let failure = match lang {
                Lang::En => "failed",
                Lang::Ja => "失敗",
            };
            assert!(
                matches!(&app.status_footer, FooterStatus::Failed(message)
                if message.contains(failure) && message.contains("project-policy-rejected")),
                "localized failed footer: {:?}",
                app.status_footer
            );
            let toast = app
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert!(matches!(toast.kind, ToastKind::Error));
            assert!(
                toast.message.contains(failure)
                    && toast.message.contains("project-policy-rejected"),
                "localized failed toast: {}",
                toast.message
            );
        });
        assert_eq!(rev_parse(&repo, &["HEAD"]), head);
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "commit")
            .collect();
        assert_eq!(receipts.len(), 1);
        assert!(matches!(&receipts[0].outcome, OpOutcome::Failed { error }
            if error.contains("project-policy-rejected")));
        unmount(cx, app, window);
    }
    i18n::set_lang(language);
    eprintln!("[gui-e2e] PASS commit_hook_failure");
}

/// Planning's final index check and execute preflight produce the same real
/// typed refusal. Deliver that result through the normal Commit plan-result
/// boundary, without a fabricated error or a timing-dependent planner race.
fn commit_planning_refusal(cx: &mut VisualTestAppContext) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_ui_core::i18n::{self, Msg, Op};
    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("approved.txt"), "APPROVED\n").unwrap();
    git(&repo, &["add", "approved.txt"]);
    let (app, window) = mount(cx, &repo);
    let repository = git2::Repository::open(&repo).unwrap();
    let plan = kagi_git::plan_commit(&repository, "approved identity").unwrap();
    std::fs::write(repo.join("approved.txt"), "NOT_APPROVED\n").unwrap();
    git(&repo, &["add", "approved.txt"]);
    let error = kagi_git::preflight_commit(&repository, &plan).unwrap_err();
    let head = rev_parse(&repo, &["HEAD"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let refs = git_output(&repo, &["show-ref"]);
    let expected = i18n::op_plan_failed(Op::Commit, Msg::AdviceCommitStagedContentChanged.t());
    app.update(cx, |app, cx| app.commit_plan_result_for_e2e(Err(error), cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(&app.status_footer, FooterStatus::Failed(message) if message.as_ref() == expected),
            "planning refusal must reach the localized footer: {:?}", app.status_footer);
        let toast = app.toast_stack.as_ref().unwrap().read(cx).toasts().last().unwrap();
        assert!(matches!(toast.kind, ToastKind::Error) && toast.message.as_ref() == expected,
            "planning refusal must reach the localized error toast: {}", toast.message);
        assert!(!app.app_sessions.has_leases() && app.write_busy_op.is_none());
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), head);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(git_output(&repo, &["show-ref"]), refs);
    assert!(
        kagi_git::oplog::read_oplog_tail_for_repo(&repo, 10).is_empty(),
        "a planning refusal must not start or record execution"
    );
    drop(repository);
    unmount(cx, app, window);
}

/// The typed identity refusal uses the same localized planning-failure surface
/// as other Commit notes; this leg isolates presentation from Git configuration.
fn commit_identity_unavailable_footer(
    cx: &mut VisualTestAppContext,
    lang: kagi_ui_core::i18n::Lang,
) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_domain::plan_note::{CommitNote, PlanNote};
    use kagi_ui_core::i18n::{self, Lang, Op};

    let fixture = branches_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let head = rev_parse(&repo, &["HEAD"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let reason = match lang {
        Lang::En => "Git author or committer identity is unavailable. Set user.name and user.email before committing.",
        Lang::Ja => "Git の作者またはコミッターの情報が設定されていません。コミットする前に user.name と user.email を設定してください。",
    };
    let note = PlanNote::Commit(CommitNote::IdentityUnavailable);
    assert_eq!(i18n::plan_note_text(&note), reason);
    let expected = i18n::op_plan_failed(Op::Commit, reason);
    let error = kagi_git::GitError::Blocked(Box::new(note));
    app.update(cx, |app, cx| app.commit_plan_result_for_e2e(Err(error), cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(matches!(&app.status_footer, FooterStatus::Failed(message) if message.as_ref() == expected),
            "identity refusal must reach the localized footer: {:?}", app.status_footer);
        let toast = app.toast_stack.as_ref().unwrap().read(cx).toasts().last().unwrap();
        assert!(matches!(toast.kind, ToastKind::Error) && toast.message.as_ref() == expected,
            "identity refusal must reach the localized error toast: {}", toast.message);
        assert!(!app.app_sessions.has_leases() && app.write_busy_op.is_none());
    });
    assert_eq!(rev_parse(&repo, &["HEAD"]), head);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(&repo, 10).is_empty());
    unmount(cx, app, window);
}
