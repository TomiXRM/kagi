//! #643 Wave 4: fetch coordination and presentation are owned by `SessionId`.

use std::path::Path;

use gpui::{SharedString, VisualTestAppContext};
use kagi::ui::{FooterStatus, ToastKind};

use crate::macos::{build_fixture, git, mount, unmount};

fn dirty(repo: &Path) {
    std::fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
}

fn local_remote_with_change(repo: &Path, root: &Path) {
    let bare = root.join("origin.git");
    let other = root.join("other");
    git(repo, &["init", "--bare", "-q", bare.to_str().unwrap()]);
    git(repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(
        root,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
}

/// A dirty Pull on the launch owner joins the running fetch and is delivered by
/// that completion rather than being planned immediately from stale refs.
pub fn scenario_fetch_same_owner_piggybacks(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().expect("remote root");
    local_remote_with_change(&repo, remote.path());
    dirty(&repo);

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        let owner = app.active_session().expect("mounted owner");
        assert!(app.fetch_async_for(false, None, cx), "launch fetch");
        app.open_pull_modal(cx);
        let flight = app.fetch_in_flight.as_ref().expect("fetch flight");
        assert_eq!(flight.owner, owner);
        assert!(
            app.pull_modal().is_none(),
            "piggybacked Pull waits for the real fetch completion"
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none(), "fetch terminalized");
        assert!(
            app.pull_modal().is_some(),
            "same-owner completion delivers the positive Pull effect"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_same_owner_piggybacks");
}

/// A fetch launched by A cannot satisfy B's freshness promise, even if both
/// tabs are active in the same window while the operation-owned flight lives.
pub fn scenario_fetch_different_owner_does_not_piggyback(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let remote_a = tempfile::tempdir().expect("remote A");
    local_remote_with_change(&repo_a, remote_a.path());
    dirty(&repo_b);

    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        let owner_a = app.active_session().expect("A owner");
        assert!(app.fetch_async_for(false, None, cx), "launch A fetch");
        app.switch_repo(1, cx);
        let owner_b = app.active_session().expect("B owner");
        assert_ne!(owner_a, owner_b);
        app.open_pull_modal(cx);
        let flight = app.fetch_in_flight.as_ref().expect("A flight retained");
        assert_eq!(flight.owner, owner_a);
        assert!(
            flight.waiters.is_empty(),
            "different owner must not join A's fetch"
        );
        assert!(
            app.pull_modal().is_none(),
            "B remains refused while A owns the write lease"
        );
    });
    cx.run_until_parked();

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_different_owner_does_not_piggyback");
}

/// A's completion terminalizes while B remains the live tab, but none of A's
/// timestamp, changed-ref reload, footer, or Pull UI is presented on B.
pub fn scenario_fetch_owner_display_isolated(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().expect("remote root");
    local_remote_with_change(&repo_a, remote.path());

    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    let (owner_b, revision, last_fetch) = app.update(cx, |app, cx| {
        // Completion cannot enter the foreground during this update: freeze A,
        // launch its real fetch, then leave for the already-loaded B.
        app.switch_repo(0, cx);
        assert!(app.fetch_async_for(false, None, cx), "launch A fetch");
        app.switch_repo(1, cx);
        let owner = app.active_session().expect("B owner");
        app.status_footer = FooterStatus::Idle(SharedString::from("B sentinel"));
        (
            owner,
            app.reads.revision(owner),
            app.view().status_summary.last_fetch_secs,
        )
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).fetch_in_flight.is_none()),
        "fetch terminalized"
    );

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(owner_b));
        assert_eq!(
            app.reads.revision(owner_b),
            revision,
            "A's changed fetch must not reload B"
        );
        assert_eq!(
            app.view().status_summary.last_fetch_secs,
            last_fetch,
            "A's fetch must not stamp B"
        );
        assert!(
            matches!(&app.status_footer, FooterStatus::Idle(text) if text.as_ref() == "B sentinel"),
            "A's fetch must not overwrite B's footer: {:?}",
            app.status_footer
        );
        assert!(app.pull_modal().is_none(), "A's fetch opened no modal on B");
        assert!(
            !app.app_sessions.has_leases(),
            "completion released the lease"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_owner_display_isolated");
}

/// Closing the launch tab prunes its waiter but not the running flight or lease.
/// Reopening the identical path creates a new owner; A's changed fetch must not
/// stamp, reload, or overwrite the footer of that fresh incarnation.
pub fn scenario_fetch_detach_retains_flight_and_isolates_reopen(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().expect("remote root");
    local_remote_with_change(&repo, remote.path());
    dirty(&repo);

    let (app, window) = mount(cx, &repo);
    let (reopened, revision) = app.update(cx, |app, cx| {
        // Launch, detach, and reopen before returning control to the executor.
        // The path is identical, but the owner incarnation is not.
        let closed = app.active_session().expect("launch owner");
        app.open_pull_modal(cx);
        app.close_tab(0, cx);
        let flight = app.fetch_in_flight.as_ref().expect("close retained flight");
        assert_eq!(flight.owner, closed, "operation keeps its frozen owner");
        assert!(flight.waiters.is_empty(), "detach pruned the closed waiter");
        assert!(
            app.app_sessions.has_leases(),
            "close retained the fetch lease"
        );
        assert!(app.open_repository(repo.clone(), cx), "reopen same path");
        let reopened = app.active_session().expect("reopened owner");
        assert_ne!(reopened, closed, "same path has a fresh SessionId");
        app.status_footer = FooterStatus::Idle(SharedString::from("reopened sentinel"));
        (reopened, app.reads.revision(reopened))
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).fetch_in_flight.is_none()),
        "fetch terminalized"
    );

    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(reopened));
        assert_eq!(
            app.reads.revision(reopened),
            revision,
            "old owner's changed fetch must not reload the same-path reopen"
        );
        assert!(
            matches!(&app.status_footer, FooterStatus::Idle(text) if text.as_ref() == "reopened sentinel"),
            "old owner's fetch must not overwrite the reopened footer: {:?}",
            app.status_footer
        );
        assert!(app.pull_modal().is_none(), "closed waiter opened no modal");
        assert!(!app.app_sessions.has_leases(), "completion released the lease");
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_detach_retains_flight_and_isolates_reopen");
}

/// A panicking executor must not strand the flight or silently unlock the
/// repository: its Unknown receipt and reconcile notice are the exit.
pub fn scenario_fetch_panicked_worker_reconciles(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.panic_next_fetch_for_e2e();
        assert!(app.fetch_async_for(false, None, cx));
        assert!(app.fetch_in_flight.is_some());
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        kagi::ui::e2e::poll_app_jobs(app, cx);
        kagi::ui::e2e::present_app_notice(app);
        assert!(
            app.fetch_in_flight.is_none(),
            "panicked fetch stranded its flight"
        );
        let ids = app.app_sessions.reconcile_ids();
        assert_eq!(ids.len(), 1, "panicked fetch has no reconcile exit");
        assert!(
            app.app_sessions.has_leases(),
            "Unknown must retain its lease"
        );
        assert!(
            kagi::ui::e2e::app_notice_message(app).is_some(),
            "reconcile path must be offered"
        );
        let read = kagi::app::read_reconcile(&app.app_sessions, ids[0]).unwrap();
        assert!(read.stop_proven() && read.resolved());
        kagi::app::acknowledge(&mut app.app_sessions, read).unwrap();
        assert!(
            app.fetch_async_for(false, None, cx),
            "ack permits another fetch"
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
    });
    let records: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .collect();
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.outcome, kagi_git::oplog::OpOutcome::Unknown { .. }))
            .count(),
        1,
        "the abandoned attempt retains exactly one Unknown receipt"
    );
    assert_eq!(
        records
            .iter()
            .filter(|entry| matches!(entry.outcome, kagi_git::oplog::OpOutcome::Success { .. }))
            .count(),
        1,
        "the admitted ref-moving fetch after acknowledgement has its own receipt"
    );
    unmount(cx, app, window);
}

/// Returning to the same tab creates a new visit, not a new permission to
/// present the earlier fetch's footer or timestamp.
pub fn scenario_fetch_previous_visit_is_not_presented(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let missing = repo_a.join("no-such-origin");
    git(
        &repo_a,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100).len();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().unwrap();
        let old_visit = app.app_sessions.attachment(owner).unwrap().visit;
        assert!(app.fetch_async_for(false, None, cx));
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        assert_ne!(app.app_sessions.attachment(owner).unwrap().visit, old_visit);
        app.status_footer = FooterStatus::Idle(SharedString::from("new visit sentinel"));
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none(), "old flight must terminalize");
        assert!(
            matches!(&app.status_footer, FooterStatus::Idle(text) if text.as_ref() == "new visit sentinel"),
            "old visit overwrote the new visit's footer: {:?}",
            app.status_footer
        );
    });
    let records = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100);
    assert_eq!(
        records.len(),
        before + 1,
        "old visit retains its durable receipt"
    );
    assert!(matches!(
        records.last().unwrap().outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));
    cx.read(|cx| {
        let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
        assert!(
            panel.entries().iter().any(|entry| {
                entry.id == records.last().unwrap().id
                    && entry.op == "fetch"
                    && entry.repo == repo_a.display().to_string()
            }),
            "old-visit failure was persisted but not pushed to the live panel"
        );
    });
    unmount(cx, app, window);
}

/// A dirty Pull joined to a running fetch belongs to the original visit.
/// Returning to the same tab before the fetch settles must not revive it.
pub fn scenario_fetch_old_visit_drops_pull_waiter(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let missing = repo_a.join("no-such-origin");
    git(
        &repo_a,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    dirty(&repo_a);
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .count();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().unwrap();
        assert!(app.fetch_async_for(false, None, cx), "launch held fetch");
        app.open_pull_modal(cx);
        let flight = app.fetch_in_flight.as_ref().unwrap();
        assert!(app.pull_modal().is_none());
        let old_visit = flight.visit;
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        assert_ne!(
            app.app_sessions.attachment(owner).map(|a| a.visit),
            Some(old_visit)
        );
        app.status_footer = FooterStatus::Idle(SharedString::from("new visit sentinel"));
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(app.pull_modal().is_none(), "old visit opened Pull on return");
        assert!(
            matches!(&app.status_footer, FooterStatus::Idle(text) if text.as_ref() == "new visit sentinel"),
            "old fetch overwrote the new visit"
        );
    });
    let records: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .collect();
    assert_eq!(
        records.len(),
        before + 1,
        "exactly one fetch failure receipt"
    );
    assert!(matches!(
        records.last().unwrap().outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));
    // A successful fetch is the modal-producing mutation oracle: dropping the
    // waiter visit check would open the old Pull confirmation on this new visit.
    git(&repo_a, &["remote", "remove", "origin"]);
    let remote = tempfile::tempdir().unwrap();
    local_remote_with_change(&repo_a, remote.path());
    app.update(cx, |app, cx| {
        let owner = app.active_session().unwrap();
        assert!(app.fetch_async_for(false, None, cx));
        app.open_pull_modal(cx);
        let old_visit = app.fetch_in_flight.as_ref().unwrap().visit;
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        assert_ne!(
            app.app_sessions.attachment(owner).map(|a| a.visit),
            Some(old_visit)
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(
            app.pull_modal().is_none(),
            "a successful old-visit fetch must not open Pull on return"
        );
    });
    unmount(cx, app, window);
}

/// A new-visit Pull joined to an old-visit flight must see that flight's
/// failure even though the old waiter remains stale and the receipt is singular.
pub fn scenario_fetch_new_visit_waiter_sees_old_flight_failure(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let missing = repo_a.join("no-such-origin");
    git(
        &repo_a,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    dirty(&repo_a);

    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100)
        .iter()
        .filter(|entry| entry.op == "fetch")
        .count();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().unwrap();
        assert!(app.fetch_async_for(false, None, cx));
        app.open_pull_modal(cx);
        let old_visit = app.fetch_in_flight.as_ref().unwrap().visit;
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        let new_visit = app.app_sessions.attachment(owner).unwrap().visit;
        assert_ne!(new_visit, old_visit);
        app.open_pull_modal(cx);
        assert!(app.pull_modal().is_none());
    });
    // The dispatcher holds the fetch until now, after both Pull requests.
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(
            app.pull_modal().is_none(),
            "a failed fetch must not open Pull"
        );
        assert!(
            matches!(&app.status_footer, FooterStatus::Failed(_)),
            "the new visit's Pull did not receive the failure: {:?}",
            app.status_footer
        );
        let toast_stack = app.toast_stack.as_ref().expect("mounted toast stack");
        assert_eq!(
            toast_stack
                .read(cx)
                .toasts()
                .iter()
                .filter(|toast| toast.kind == ToastKind::Error)
                .count(),
            1,
            "only the current waiter receives one failure preview"
        );
    });
    let receipts: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo_a, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .collect();
    assert_eq!(
        receipts.len(),
        before + 1,
        "one flight owns one durable failure"
    );
    assert!(matches!(
        receipts.last().unwrap().outcome,
        kagi_git::oplog::OpOutcome::Failed { .. }
    ));
    unmount(cx, app, window);
}

/// #992 review: a new-visit Pull joined to an old-visit flight that SUCCEEDS
/// opens its confirmation, and the app is notified after that delivery so the
/// modal is drawn now rather than on the next unrelated frame.
pub fn scenario_fetch_new_visit_waiter_success_notifies(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    local_remote_with_change(&repo_a, remote.path());
    dirty(&repo_a);
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        let owner = app.active_session().unwrap();
        assert!(app.fetch_async_for(false, None, cx));
        app.open_pull_modal(cx);
        let old_visit = app.fetch_in_flight.as_ref().unwrap().visit;
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
        let new_visit = app.app_sessions.attachment(owner).unwrap().visit;
        assert_ne!(new_visit, old_visit);
        app.open_pull_modal(cx);
        assert!(app.pull_modal().is_none());
    });
    // Count notifications from here on: the setup's own are already flushed.
    // The first notification that sees the modal must be flushed by the
    // completion's own update (before it returns), not by a later task.
    let completions = kagi::ui::e2e::fetch_completions_returned();
    let first_modal_notify = std::rc::Rc::new(std::cell::Cell::new(None::<u64>));
    {
        let first_modal_notify = first_modal_notify.clone();
        cx.update(|cx| {
            cx.observe(&app, move |app, cx| {
                if first_modal_notify.get().is_none() && app.read(cx).pull_modal().is_some() {
                    first_modal_notify.set(Some(kagi::ui::e2e::fetch_completions_returned()));
                }
            })
            .detach()
        });
    }
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(
            app.pull_modal().is_some(),
            "the new visit's Pull confirmation must open after the old flight succeeds"
        );
    });
    assert_eq!(
        first_modal_notify.get(),
        Some(completions),
        "fetch-new-visit-success-notifies: the delivered Pull confirmation was not notified by the completion itself"
    );
    unmount(cx, app, window);
}

/// #992 review: an auto-fetch whose visit has ended still emits its `[kagi]`
/// contract line (ok / failed), unchanged; only the presentation is withheld.
pub fn scenario_auto_fetch_old_visit_logs_contract(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let missing = repo_a.join("no-such-origin");
    git(
        &repo_a,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );
    let (app, window) = mount(cx, &repo_a);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    cx.run_until_parked();
    let count = |needle: &str| {
        kagi_ui_core::klog::tail()
            .into_iter()
            .filter(|line| line.starts_with(needle))
            .count()
    };

    // Failed: the silent fetch's failure line survives the departure.
    let failed_before = count("[kagi] auto-fetch: failed (silent): ");
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert!(app.fetch_async_for(true, None, cx));
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).fetch_in_flight.is_none()));
    assert_eq!(
        count("[kagi] auto-fetch: failed (silent): "),
        failed_before + 1,
        "auto-fetch-old-visit-contract: the departed failure lost its contract line"
    );

    // Ok: same for a successful silent fetch.
    git(&repo_a, &["remote", "remove", "origin"]);
    let remote = tempfile::tempdir().unwrap();
    local_remote_with_change(&repo_a, remote.path());
    let ok_before = count("[kagi] auto-fetch: ok remote=");
    app.update(cx, |app, cx| {
        assert!(app.fetch_async_for(true, None, cx));
        app.switch_repo(1, cx);
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).fetch_in_flight.is_none()));
    assert_eq!(
        count("[kagi] auto-fetch: ok remote="),
        ok_before + 1,
        "auto-fetch-old-visit-contract: the departed success lost its contract line"
    );
    unmount(cx, app, window);
}
