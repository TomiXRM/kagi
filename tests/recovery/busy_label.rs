//! Fetch's real admission/dispatch path supplies the snackbar text before completion.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};

pub fn scenario_fetch_busy_label(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // Local transport only; no network or delayed child process in the runner.
    git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    let (app, window) = mount(cx, &repo);
    let original_language = i18n::lang();
    for (language, expected) in [(Lang::En, "Fetching…"), (Lang::Ja, "fetch 中…")] {
        i18n::set_lang(language);
        app.update(cx, |app, cx| {
            app.fetch_async(false, cx);
            // Assert in the same UI turn before the completion can be delivered.
            // This is the label consumed by render_busy_snackbar, not the footer.
            assert!(app.fetch_in_flight);
            assert_eq!(app.busy_op, Some("fetch"));
            assert_eq!(e2e::busy_snackbar_label(app), Some(expected));
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).fetch_in_flight) {
            cx.run_until_parked();
            assert!(std::time::Instant::now() < deadline, "fetch did not settle");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        git(
            &repo,
            &["rev-parse", "--verify", "refs/remotes/origin/main"],
        );
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.fetch_in_flight);
            assert!(!state.app_sessions.has_leases());
            assert_eq!(state.busy_op, None);
            assert_eq!(e2e::busy_snackbar_label(state), None);
        });
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_busy_label: EN/JA fetch snackbar and busy release");
}

/// #646: a failed fetch reaches the oplog, not just the footer.
///
/// `AGENTS.md` requires a user-facing error to surface via the oplog **and** a
/// modal. Fetch had only the footer and a toast, so a failure left nothing to
/// recover or audit from — and a silent auto-fetch left no trace at all.
pub fn scenario_fetch_failure_reaches_the_oplog(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // A remote that cannot be reached: a local path with no repository in it.
    let missing = repo.join("no-such-remote");
    git(
        &repo,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );

    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .count();

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.fetch_async(false, cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| app.read(cx).fetch_in_flight) {
        cx.run_until_parked();
        assert!(std::time::Instant::now() < deadline, "fetch did not settle");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let entries: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .collect();
    assert_eq!(
        entries.len(),
        before + 1,
        "a failed fetch must leave exactly one oplog record (#646)"
    );
    let outcome = &entries.last().expect("the record just written").outcome;
    assert!(
        matches!(
            outcome,
            kagi_git::oplog::OpOutcome::Failed { .. } | kagi_git::oplog::OpOutcome::Unknown { .. }
        ),
        "an unreachable remote is a failure, not a success: {outcome:?}"
    );

    // The visible surface must still be there — the oplog record replaces
    // nothing.
    cx.read(|cx| {
        assert!(
            matches!(
                app.read(cx).status_footer,
                kagi::ui::FooterStatus::Failed(_)
            ),
            "the footer must still report the failure"
        );
    });
    unmount(cx, app, window);
}

/// #643 A1: when the oplog append fails, the user is told.
///
/// The operation itself already happened, so the append failure was logged as
/// "non-fatal" and nothing reached the screen. But Kagi's promise includes
/// leaving a record to recover and audit from, and the in-memory panel still
/// shows the entry — so nothing looks wrong until the next launch, when it is
/// simply gone.
pub fn scenario_oplog_append_failure_is_visible(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    // Hold the oplog's lock so the append cannot take it, the same way the
    // staging-failure test induces this.
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    app.update(cx, |app, cx| {
        e2e::record_persisted_op(app, &repo, cx);
    });

    cx.read(|cx| {
        let stack = app
            .read(cx)
            .toast_stack
            .clone()
            .expect("the mounted app has a toast stack");
        let said_so = stack
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.contains("oplog") || toast.message.contains("記録"));
        assert!(
            said_so,
            "a failed oplog append must reach the user: they are about to trust \
             a log that is missing this entry (#643 A1)"
        );
    });

    drop(lock);
    let _ = std::fs::remove_file(&lock_path);
    unmount(cx, app, window);
}

/// A PR the fixture repo can plan a clean merge for. No GitHub remote exists,
/// so `gh` refuses locally: the transport fails fast and offline, and the
/// lease lifecycle — not the merge result — is the oracle.
fn unmergeable_pr() -> kagi_domain::github::PullRequest {
    use kagi_domain::github::{CiState, Mergeable, PullRequest, ReviewState};
    PullRequest {
        number: 7,
        title: "a merge that never reaches GitHub".into(),
        head: "feature".into(),
        head_sha: "0".repeat(40),
        base: "main".into(),
        is_draft: false,
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: "https://github.test/pr/7".into(),
        author: "tester".into(),
        reviewers: Vec::new(),
        body: String::new(),
        checks: Vec::new(),
        mergeable: Mergeable::Clean,
    }
}

/// ADR-0196 Wave 3: a PR merge is a write, so it is admitted through the write
/// lease instead of the legacy `busy_op` latch. The lease is what holds quit
/// and tab-close — the latch never did. `complete_git` releases it on a known
/// termination, even when the tab was left; an unconfirmed one (here: a
/// panicked task) retains it, and the busy mirror with it.
pub fn scenario_pr_merge_holds_the_write_lease(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let pr = unmergeable_pr();
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
        assert!(
            app.pr_merge_modal()
                .is_some_and(|m| m.plan.blockers.is_empty()),
            "the fixture PR must plan cleanly so the merge is actually dispatched"
        );
        app.start_pr_merge(cx);
        // Same UI turn as the dispatch: the lease exists before the transport
        // can answer, which is exactly what the quit guard reads.
        assert!(
            app.app_sessions.has_leases(),
            "a dispatched pr-merge must hold the write lease (ADR-0196 Wave 3)"
        );
        assert!(
            !app.app_sessions.may_close_host(),
            "quit must be held while the merge is in flight"
        );
        assert_eq!(app.busy_op, Some("pr-merge"), "the busy mirror follows it");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the pr-merge lease was never released"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.busy_op, None);
        assert!(
            state.app_sessions.may_close_host(),
            "a settled merge must not keep holding quit"
        );
    });
    unmount(cx, app, window);

    // The settle half runs on arrival, before the stale-tab guard: leaving the
    // tab mid-merge drops the presentation, never the lease release (#501).
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
        app.start_pr_merge(cx);
        assert!(app.app_sessions.has_leases());
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(1, cx);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "a merge whose tab was left must still settle its lease"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state.active_tab, 1,
            "the user is on the tab they switched to"
        );
        assert_eq!(state.busy_op, None);
        assert!(state.app_sessions.may_close_host());
    });
    unmount(cx, app, window);

    // An unconfirmed termination proves nothing about the transport: `gh` may
    // have merged. The lease is retained there — and its busy mirror must be
    // retained with it, or a plan could start against a live writer.
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "feature", "HEAD~1"]);
    let (app, window) = mount(cx, &repo);
    e2e::arm_pr_merge_termination_unknown();
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
        app.start_pr_merge(cx);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| {
        !kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
            .iter()
            .any(|entry| entry.op == "pr-merge")
    }) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the unconfirmed merge never reached its completion"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            app.app_sessions.has_leases(),
            "an unconfirmed termination retains the lease — the merge may still be running"
        );
        assert_eq!(
            app.busy_op,
            Some("pr-merge"),
            "the busy mirror must stay with the retained lease (ADR-0196 Wave 3)"
        );
        assert!(e2e::busy_snackbar_label(app).is_some());
        // Planning must not start against a writer that may still be running.
        app.open_merge_modal("feature".into(), None, cx);
        assert_eq!(app.planning, None, "a retained lease must refuse planning");
        assert!(app.merge_modal().is_none());
        app.open_delete_branch_modal("feature", cx);
        assert_eq!(app.planning, None);
        assert!(app.delete_branch_modal().is_none());
    });
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pr_merge_write_lease: lease taken, settled off-tab, retained when unknown"
    );
}

/// A refused admission must not cost the user their confirmation: the plan
/// they read and the options they chose are only discarded once the lease is
/// actually held, and nothing reaches the transport.
pub fn scenario_pr_merge_admission_keeps_the_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let pr = unmergeable_pr();
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Squash, true, cx);
        assert!(app.pr_merge_modal().is_some());
    });
    // The repository stops being openable between the confirmation and the
    // dispatch, so admission refuses on identity — after `reject_if_busy`.
    std::fs::rename(repo.join(".git"), repo.join("git-unavailable")).unwrap();
    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len();
    app.update(cx, |app, cx| app.start_pr_merge(cx));
    cx.run_until_parked();
    std::fs::rename(repo.join("git-unavailable"), repo.join(".git")).unwrap();
    cx.read(|cx| {
        let state = app.read(cx);
        let modal = state
            .pr_merge_modal()
            .expect("a refused admission must keep the confirmation");
        assert_eq!(modal.number, 7);
        assert!(modal.delete_branch, "the chosen options survive with it");
        assert_eq!(modal.method, kagi_git::github::MergeMethod::Squash);
        assert!(!state.app_sessions.has_leases());
        assert_eq!(state.busy_op, None);
    });
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
        before,
        "a refused admission dispatches no transport, so it records no merge"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_merge_admission_keeps_modal: refusal keeps the confirmed plan");
}

/// ADR-0196 Wave 3: planning is not a write, so it latches `planning` rather
/// than `busy_op` — but it still owns the modal slot it is about to fill, so
/// every gate must refuse a second operation while it runs.
pub fn scenario_merge_plan_latches_planning(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "feature", "HEAD~1"]);
    let (app, window) = mount(cx, &repo);
    let original_language = i18n::lang();
    i18n::set_lang(Lang::En);
    app.update(cx, |app, cx| {
        app.open_merge_modal("feature".into(), None, cx);
        // Same UI turn as the dispatch: nothing has been written, so the write
        // latch is free — the plan latch is what is held.
        assert_eq!(app.planning, Some("merge-plan"));
        assert_eq!(app.busy_op, None, "planning takes no write latch");
        assert!(
            !app.app_sessions.has_leases(),
            "planning takes no write lease either"
        );
        assert_eq!(e2e::busy_snackbar_label(app), Some("Planning merge…"));
        // The gate a second operation consults must see the plan latch: it is
        // refused on the spot, so this plan keeps the latch and the footer says
        // why. (A dispatched delete-branch plan would take the latch itself and
        // land its own modal in the slot this merge plan is about to fill.)
        app.open_delete_branch_modal("feature", cx);
        assert_eq!(
            app.planning,
            Some("merge-plan"),
            "a plan in flight must refuse the next operation (#283)"
        );
        assert!(app.delete_branch_modal().is_none());
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(text)
                if text.as_ref() == i18n::Msg::OpInProgress.t()),
            "the refusal must say an operation is already in progress: {:?}",
            app.status_footer
        );
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the merge plan never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.merge_modal().is_some(),
            "the settled plan opens its modal"
        );
        assert_eq!(e2e::busy_snackbar_label(state), None);
    });
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS merge_plan_latch: planning latches and releases without busy_op");
}
