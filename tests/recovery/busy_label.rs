//! Fetch's real admission/dispatch path supplies the snackbar text before completion.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};

#[path = "pr_merge_local.rs"]
mod pr_merge_local;

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

pub fn scenario_fetch_busy_label(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
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
            assert!(app.fetch_in_flight.is_some());
            assert_eq!(app.write_busy_op, Some("fetch"));
            assert_eq!(e2e::busy_snackbar_label(app), Some(expected));
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
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
            assert!(state.fetch_in_flight.is_none());
            assert!(!state.app_sessions.has_leases());
            assert_eq!(state.write_busy_op, None);
            assert_eq!(e2e::busy_snackbar_label(state), None);
        });
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_busy_label: EN/JA fetch snackbar and busy release");
}

/// #646 / #1698: a failed fetch remains durable, with one context-appropriate preview.
pub fn scenario_fetch_failure_reaches_the_oplog(cx: &mut VisualTestAppContext) {
    use kagi::ui::{FooterStatus, ToastKind};
    use kagi_git::oplog::OpOutcome;

    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for active in [true, false] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
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
            app.update(cx, |app, cx| {
                app.set_window_active_for_e2e(active, cx);
                app.bottom_panel_open = false;
                app.fetch_async(false, cx);
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
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
                "exactly one failed-fetch receipt"
            );
            let receipt = entries.last().unwrap();
            let OpOutcome::Failed { error } = &receipt.outcome else {
                panic!(
                    "an unreachable local remote must fail: {:?}",
                    receipt.outcome
                );
            };
            assert!(error.contains(missing.to_str().unwrap()), "{error}");
            assert_eq!(receipt.ref_moves, Some(Vec::new()), "no local ref moved");
            assert!(
                kagi::ui::oplog_panel::entry_clipboard_text(receipt)
                    .contains(missing.to_str().unwrap()),
                "the durable detail keeps the actual failed remote"
            );
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(!state.app_sessions.has_leases());
                assert!(state.app_notice().is_none());
                assert!(!state.bottom_panel_open, "failure never auto-opens details");
                let stack = state.toast_stack.as_ref().unwrap().read(cx);
                let preview = if active {
                    assert!(stack.toasts().is_empty(), "attended failure is footer-only");
                    let FooterStatus::Failed(preview) = &state.status_footer else {
                        panic!("attended failure needs a footer: {:?}", state.status_footer);
                    };
                    preview
                } else {
                    assert!(matches!(state.status_footer, FooterStatus::Idle(_)));
                    assert_eq!(stack.toasts().len(), 1, "one away error notification");
                    let toast = &stack.toasts()[0];
                    assert_eq!(toast.kind, ToastKind::Error);
                    assert!(
                        toast.result_key.is_some(),
                        "away failure has a replacement key"
                    );
                    assert!(toast.action.is_some(), "away failure links to its receipt");
                    &toast.message
                };
                assert!(!preview.is_empty() && preview.chars().count() <= 240);
                for private in [
                    repo.to_str().unwrap(),
                    "no-such-remote",
                    "fatal:",
                    "exit status",
                ] {
                    assert!(!preview.contains(private), "{preview}");
                }
            });
            if !active {
                crate::recovery_oplog_panel::paint(cx, window);
                let action = e2e::control_bounds(window.window_id(), "operation-result-details")
                    .expect("the error notification draws its detail action");
                cx.simulate_click(window, action.center(), gpui::Modifiers::none());
                crate::recovery_oplog_panel::paint(cx, window);
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(state.bottom_panel_open);
                    assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                    let panel = state.op_log.as_ref().unwrap().read(cx);
                    let row = panel.expanded().expect("the failed Fetch is expanded");
                    assert_eq!(panel.entries()[row].id, receipt.id);
                    assert!(panel.scroll_handle().bounds_for_item(row).is_some());
                });
            }
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original_language);
}

/// #1699: recording failure must not change the operation's actual result.
/// Exercise the real failed-fetch path and persisted success/unknown receipts
/// in both languages while the oplog writer is unable to take its lock.
pub fn scenario_oplog_append_failure_is_visible(cx: &mut VisualTestAppContext) {
    use kagi::ui::FooterStatus;
    use kagi_git::oplog::OpOutcome;

    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    let append_diagnostic = std::fs::TryLockError::WouldBlock.to_string();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for result in ["failed", "success", "unknown"] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let missing = repo.join("no-such-remote");
            git(
                &repo,
                &["remote", "add", "origin", missing.to_str().unwrap()],
            );
            let refs_before = git_output(&repo, &["show-ref"]);
            let status_before = git_output(&repo, &["status", "--porcelain"]);
            let entries_before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len();
            let (app, window) = mount(cx, &repo);
            let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                .join("operations.jsonl.lock");
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path)
                .unwrap();
            lock.lock().unwrap();

            if result == "failed" {
                app.update(cx, |app, cx| app.fetch_async(false, cx));
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
                    cx.run_until_parked();
                    assert!(std::time::Instant::now() < deadline, "fetch did not settle");
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            } else {
                let state = kagi_git::StateSummary {
                    head: "branch: main".into(),
                    dirty: "clean".into(),
                };
                let outcome = if result == "success" {
                    OpOutcome::Success { after: state }
                } else {
                    OpOutcome::Unknown {
                        after: state,
                        evidence: "fixture result unconfirmed".into(),
                    }
                };
                app.update(cx, |app, cx| {
                    e2e::record_persisted_op(app, &repo, outcome, cx);
                });
            }

            cx.read(|cx| {
                let state = app.read(cx);
                let notice = state.app_notice().expect("recording failure notice");
                let message = notice.message.as_str();
                assert!(message.contains(&repo.display().to_string()), "{message}");
                assert!(message.contains(&append_diagnostic));
                assert!(state.toast_stack.as_ref().unwrap().read(cx).toasts().is_empty(),
                    "the mandatory acknowledgment, not another result toast, owns this failure");
                assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.is_empty()),
                    "a result footer must not override required acknowledgment");
                let panel = state.op_log.as_ref().unwrap().read(cx);
                let op = if result == "failed" {
                    "fetch"
                } else {
                    "e2e-record"
                };
                let entry = panel.entries().iter().find(|entry| entry.op == op).unwrap();
                match (&entry.outcome, result) {
                    (OpOutcome::Failed { error }, "failed") => {
                        assert!(error.contains("no-such-remote"));
                        assert!(message.contains(error.as_str()),
                            "the missing durable receipt cannot discard the backend diagnostic");
                    }
                    (OpOutcome::Partial { after, error }, "success") => {
                        assert!(error.contains(&append_diagnostic));
                        assert_eq!(after.head, "branch: main");
                        assert_eq!(after.dirty, "clean");
                    }
                    (OpOutcome::Unknown { evidence, .. }, "unknown") => {
                        assert!(evidence.contains("fixture result unconfirmed"));
                        assert!(message.contains("fixture result unconfirmed"),
                            "uncertainty evidence must remain inspectable without a saved receipt");
                    }
                    (outcome, _) => panic!("recording failure changed {result}: {outcome:?}"),
                }
            });
            assert_eq!(git_output(&repo, &["show-ref"]), refs_before);
            assert_eq!(git_output(&repo, &["status", "--porcelain"]), status_before);
            assert_eq!(
                kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).len(),
                entries_before,
                "the attempted row must not masquerade as a durable entry",
            );
            drop(lock);
            let _ = std::fs::remove_file(&lock_path);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original_language);
}

/// A PR the fixture repo can plan a clean merge for. No GitHub remote exists,
/// so `gh` refuses locally: the transport fails fast and offline, and the
/// lease lifecycle — not the merge result — is the oracle.
fn unmergeable_pr() -> kagi_domain::github::PullRequest {
    use kagi_domain::github::{CiState, IssueState, Mergeable, PullRequest, ReviewState};
    PullRequest {
        state: IssueState::Open,
        number: 7,
        title: "a merge that never reaches GitHub".into(),
        head: "feature".into(),
        head_sha: "0".repeat(40),
        base: "main".into(),
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: "https://github.test/pr/7".into(),
        author: "tester".into(),
        mergeable: Mergeable::Clean,
        cross_repository: false,
        base_repo: "o/r".into(),
        ..Default::default()
    }
}

/// ADR-0196 Wave 3: a PR merge is a write, so it rides the run family — the
/// lease is what holds quit and tab-close, which the legacy busy latch
/// never did. Its two indeterminate terminals have different owners: an
/// `Unknown` receipt parks a reconcile entry (readable and acknowledgeable,
/// because both `gh` children exited), while a `Partial` — merged, deletion
/// unconfirmed — is held by the transport, at settlement, even off-tab.
pub fn scenario_pr_merge_holds_the_write_lease(cx: &mut VisualTestAppContext) {
    pr_merge_local::local_cleanup_notices(cx);
    // Part 1 — the honest fixture terminal. No GitHub remote exists, so the
    // merge fails AND the `gh pr view` re-read cannot say whether it landed:
    // `Unknown`. The children are accounted for, so the lease is released at
    // settlement, but the scope stays parked until the entry is acknowledged.
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "feature", "HEAD~1"]);
    let (app, window) = mount(cx, &repo);
    let pr = unmergeable_pr();
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
        assert!(
            app.pr_merge_modal().is_none(),
            "the plan is built off the UI thread, so nothing is confirmable yet (ADR-0141)"
        );
        assert_eq!(app.planning, Some("merge-plan"));
    });
    pr_merge_local::settle_plan(cx, &app);
    app.update(cx, |app, cx| {
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
        assert_eq!(
            app.write_busy_op,
            Some("pr-merge"),
            "the busy mirror follows it"
        );
        assert!(e2e::op_latched(app), "and the gate is latched");
        // ADR-0196 Wave 3: the *lease* is the evidence, not the mirror. Drop the
        // mirror and the gate must still refuse — this is what deleting
        // the legacy busy field rests on.
        app.write_busy_op = None;
        assert!(
            e2e::op_latched(app),
            "a held lease alone must latch the gate, with no busy mirror at all"
        );
        app.write_busy_op = Some("pr-merge");
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|_| {
        !kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
            .iter()
            .any(|entry| entry.op == "pr-merge")
    }) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the merge never reached its completion"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
    let recorded = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "pr-merge")
        .next_back()
        .expect("the transport recorded its attempt");
    assert!(
        matches!(recorded.outcome, kagi_git::oplog::OpOutcome::Unknown { .. }),
        "gh cannot answer here, so the merge is neither confirmed nor refuted: {:?}",
        recorded.outcome
    );
    let (parked, read) = app.update(cx, |app, cx| {
        // The exit: `apply` parked a reconcile entry for this operation, and
        // because the transport reported a *stopped* child it is readable
        // straight away — not a dead end.
        let parked = app.app_sessions.reconcile_ids();
        assert_eq!(
            parked.len(),
            1,
            "an Unknown receipt must park exactly one reconcile entry"
        );
        let read = kagi::app::read_reconcile(&app.app_sessions, parked[0])
            .expect("both gh processes exited, so the entry is readable");
        assert!(read.stop_proven(), "the transport already proved the stop");
        // Readable is not the same as answerable: this fixture has no GitHub,
        // so the promise frozen at approval — that PR #7 reads *merged* —
        // cannot be checked, and the requirement stays open (#701). The
        // acknowledge that a merged re-read earns is covered by
        // `pr_merge_unknown_resolves_only_on_a_merged_re_read`.
        assert!(
            !read.resolved(),
            "nothing can confirm the merge here: {}",
            read.observation
        );
        // Meanwhile the scope is refused: the reconcile entry, not a lease.
        assert!(
            !app.app_sessions.has_leases(),
            "a stopped child releases the lease at settlement"
        );
        // #718: an async plan lands only in an empty slot, so whatever the
        // settled merge left on screen is dismissed first, exactly as the
        // user would — otherwise the plan is discarded, not shown.
        app.clear_app_notice();
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
        // Planning is read-only, so a parked reconcile entry does not refuse
        // it; the admission the *write* asks for is what refuses.
        assert_eq!(app.planning, Some("merge-plan"));
        (parked, read)
    });
    pr_merge_local::settle_plan(cx, &app);
    app.update(cx, |app, cx| {
        app.start_pr_merge(cx);
        assert!(
            matches!(app.status_footer, kagi::ui::FooterStatus::Failed(_)),
            "a refused admission must reach the footer"
        );
        assert_eq!(
            app.app_sessions.reconcile_ids(),
            parked,
            "and no second operation was started"
        );
        assert!(
            app.pr_merge_modal().is_some(),
            "a refused admission keeps the confirmation"
        );
        // The retained confirmation owns the slot until the user cancels it.
        app.cancel_pr_merge_modal();
        e2e::present_app_notice(app);
        let notice = app
            .app_notice()
            .expect("the refusal must remain actionable");
        assert_eq!(notice.inspect, Some(parked[0]));
        assert!(notice.acknowledge.is_none(), "inspection precedes release");
        // And an unconfirmable promise must not be closed on the user's behalf.
        assert!(
            kagi::app::acknowledge(&mut app.app_sessions, read).is_err(),
            "a merge nobody can confirm stays parked"
        );
        assert_eq!(app.app_sessions.reconcile_ids(), parked);
    });
    unmount(cx, app, window);

    // Part 2 — a known termination releases the lease, and the settle half
    // runs before the stale-tab guard: leaving the tab mid-merge drops the
    // presentation, never the release (#501).
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let (app, window) = mount(cx, &repo);
    e2e::arm_pr_merge_terminal(e2e::PrMergeTerminal::Failed);
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, false, cx);
    });
    pr_merge_local::settle_plan(cx, &app);
    app.update(cx, |app, cx| {
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
        assert_eq!(state.write_busy_op, None);
        assert!(state.app_sessions.may_close_host());
        assert!(
            state.app_sessions.reconcile_ids().is_empty(),
            "a refused merge is not an unknown one"
        );
    });
    unmount(cx, app, window);

    // Part 3 — `Partial`: merged, but the branch deletion never answered.
    // `apply` releases the lease and parks nothing (the merge *is* done), so
    // the transport hold is the only thing standing between the user and a
    // second merge — and it must be registered at settlement, not in the
    // presentation half the stale-tab guard skips.
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let (app, window) = mount(cx, &repo);
    e2e::arm_pr_merge_terminal(e2e::PrMergeTerminal::Partial);
    app.update(cx, |app, cx| {
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, true, cx);
    });
    pr_merge_local::settle_plan(cx, &app);
    app.update(cx, |app, cx| {
        app.start_pr_merge(cx);
        // Leave the tab in the same turn: the presentation is dropped.
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(1, cx);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "a partial merge still settles its lease"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "a merge that happened is not awaiting reconcile"
        );
        app.switch_repo(0, cx);
        // The hold survives the tab move: the button refuses even to *plan*
        // again, so no background plan is started and no modal can land.
        app.open_pr_merge_modal(&pr, kagi_git::github::MergeMethod::Merge, true, cx);
        assert_eq!(
            app.planning, None,
            "the hold refuses before any planning starts"
        );
        assert!(
            app.pr_merge_modal().is_none(),
            "an unfinished merge must not be offered again, even off-tab (#501)"
        );
    });
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pr_merge_write_lease: unknown reconciles, known releases, partial holds off-tab"
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
    });
    pr_merge_local::settle_plan(cx, &app);
    app.update(cx, |app, _| {
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
        assert_eq!(state.write_busy_op, None);
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
/// than a write latch — but it still owns the modal slot it is about to fill, so
/// every gate must refuse a second operation while it runs.
pub fn scenario_merge_plan_latches_planning(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
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
        assert_eq!(app.write_busy_op, None, "planning takes no write latch");
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
        // The other direction, which the lease cannot answer for: a *write*
        // started while a plan is in flight. Only the plan latch refuses it —
        // no lease is held here (ADR-0196 Wave 3).
        assert!(e2e::op_latched(app), "the gate reads the plan latch");
        assert!(
            !app.fetch_async_for(true, None, cx),
            "a write must not start while a plan is in flight"
        );
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
    });
    app.update(cx, |app, _| {
        app.status_footer =
            kagi::ui::FooterStatus::Busy(gpui::SharedString::from("Planning merge…"));
    });
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            e2e::set_remote_browse_host_input(app, "merge-wins", window, cx);
        });
    })
    .unwrap();
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
            state
                .remote_browse()
                .is_some_and(|modal| modal.host_input == "merge-wins"),
            "merge-plan-preserves-remote-input: an async plan must not replace foreground input"
        );
        assert!(
            state.merge_modal().is_none(),
            "merge-plan-not-queued: the raw plan must be discarded on contention"
        );
        assert_eq!(state.planning, None, "the planning latch must be released");
        assert!(
            !matches!(state.status_footer, kagi::ui::FooterStatus::Busy(_)),
            "merge-plan-terminates-footer: discarding a contended plan must end its Busy footer"
        );
        assert!(
            !matches!(
                state.app_sessions.plan_state(),
                kagi::app::PlanState::Ready { .. }
            ),
            "merge-plan-token-invalidated: no confirmable plan token may survive contention"
        );
        assert_eq!(e2e::busy_snackbar_label(state), None);
    });
    app.update(cx, |app, _| {
        app.cancel_remote_browse();
        e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| e2e::app_notice_message(app.read(cx))
            .is_some_and(|message| !message.trim().is_empty())),
        "merge-plan-presents-retry-notice: closing the foreground reveals the retry action"
    );
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS merge_plan_latch: Remote Browse wins; latch releases; retry notice waits"
    );
}

/// A custom Delete Branch planner has the same slot/latch/footer contract as
/// `finish_planning`: a foreground modal wins, and the discarded plan leaves
/// no stale confirmation or Busy presentation behind.
pub fn scenario_delete_branch_plan_latches_planning(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "victim", "HEAD~1"]);
    let (app, window) = mount(cx, &repo);
    let original_language = i18n::lang();
    i18n::set_lang(Lang::En);
    app.update(cx, |app, cx| {
        app.open_delete_branch_modal("victim", cx);
        assert_eq!(app.planning, Some("delete-branch-plan"));
        assert_eq!(
            e2e::busy_snackbar_label(app),
            Some("Planning branch deletion…")
        );
        app.open_remote_browse(cx);
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            e2e::set_remote_browse_host_input(app, "delete-wins", window, cx);
        });
    })
    .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).planning.is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "the Delete Branch plan never settled"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state
                .remote_browse()
                .is_some_and(|modal| modal.host_input == "delete-wins"),
            "delete-plan-preserves-remote-input: the foreground modal and its input must survive"
        );
        assert!(
            state.delete_branch_modal().is_none(),
            "delete-plan-not-queued: the contended raw plan must be discarded"
        );
        assert_eq!(state.planning, None, "the planning latch must be released");
        assert!(
            !matches!(state.status_footer, kagi::ui::FooterStatus::Busy(_)),
            "delete-plan-terminates-footer: latch release must terminate its Busy footer"
        );
    });
    app.update(cx, |app, _| {
        app.cancel_remote_browse();
        e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| e2e::app_notice_message(app.read(cx))
            .is_some_and(|message| !message.trim().is_empty())),
        "delete-plan-presents-retry-notice: closing the foreground reveals the retry action"
    );
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS delete_branch_plan_latch: Remote Browse wins; latch and footer terminate"
    );
}
