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
    app.update(cx, |app, _| app.open_checkout_commit_modal(target));
    app.update(cx, |app, cx| app.start_checkout(cx));
    cx.run_until_parked();
    std::fs::write(repo.join("first.txt"), "detached\n").unwrap();
    git(&repo, &["add", "first.txt"]);
    queue_commit(cx, &app, window, "detached commit", "");
    app.update(cx, |app, _| app.open_plan_modal("b"));
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
