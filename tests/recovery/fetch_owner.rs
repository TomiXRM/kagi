//! #643 Wave 4: fetch coordination and presentation are owned by `SessionId`.

use std::path::Path;

use gpui::{SharedString, VisualTestAppContext};
use kagi::ui::FooterStatus;

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
        assert_eq!(flight.waiters, vec![owner], "same owner joins the flight");
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
        assert_eq!(
            app.fetch_in_flight.as_ref().unwrap().waiters,
            vec![closed],
            "launch Pull is represented in the flight"
        );
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
        assert!(app.pending_pull_confirm.is_empty(), "closed waiter was not parked");
        assert!(!app.app_sessions.has_leases(), "completion released the lease");
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_detach_retains_flight_and_isolates_reopen");
}
