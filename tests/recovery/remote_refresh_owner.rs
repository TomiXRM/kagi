//! #643 S2a: real refresh dispatch with controllable transport completion order.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

type Reply = Result<kagi_git::RepoSnapshot, String>;

// A pending future yields to TestDispatcher rather than blocking a child thread.
// Only the SSH transport is substituted; the production launch/accept path runs.
fn defer_refresh(cx: &mut VisualTestAppContext) -> impl FnOnce(Reply) {
    let state = Arc::new(Mutex::new((None::<Reply>, None::<Waker>)));
    let pending = state.clone();
    e2e::queue_remote_refresh(
        cx.background_executor
            .spawn(std::future::poll_fn(move |cx| {
                let mut state = pending.lock().expect("remote reply lock");
                if let Some(reply) = state.0.take() {
                    Poll::Ready(reply)
                } else {
                    state.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            })),
    );
    move |reply| {
        let mut state = state.lock().expect("remote reply lock");
        state.0 = Some(reply);
        if let Some(waker) = state.1.take() {
            waker.wake();
        }
    }
}

fn snapshot(repo: &std::path::Path) -> kagi_git::RepoSnapshot {
    kagi_git::Backend::open(repo)
        .unwrap()
        .snapshot(100)
        .unwrap()
}

pub fn scenario_remote_refresh_departed_owner(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let host = kagi_domain::remote::RemoteHost::parse("example.test").unwrap();
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/repo".into(), snapshot(&repo), cx);
    });
    cx.run_until_parked();
    let finish = defer_refresh(cx);
    let local = app.update(cx, |app, cx| {
        app.refresh_remote_view(cx);
        app.switch_repo(0, cx);
        app.active_session().unwrap()
    });
    finish(Ok(snapshot(&repo)));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(local),
            "departed refresh must not steal the active tab"
        );
        assert!(
            app.remote_view.is_none(),
            "local repository must remain displayed"
        );
    });
    // Returning to the same session is a new visit, not permission for old UI work.
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    let finish = defer_refresh(cx);
    let remote = app.update(cx, |app, cx| {
        let remote = app.active_session().unwrap();
        app.refresh_remote_view(cx);
        app.switch_repo(0, cx);
        app.switch_repo(1, cx);
        remote
    });
    finish(Ok(snapshot(&repo)));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(remote),
            "old visit must not replace the revisited remote session"
        );
    });
    unmount(cx, app, window);
}

pub fn scenario_remote_refresh_newest_request(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let host = kagi_domain::remote::RemoteHost::parse("example.test").unwrap();
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/repo".into(), snapshot(&repo), cx);
    });
    cx.run_until_parked();
    let older = defer_refresh(cx);
    let owner = app.update(cx, |app, cx| {
        app.refresh_remote_view(cx);
        app.active_session().unwrap()
    });
    let newer = defer_refresh(cx);
    app.update(cx, |app, cx| app.refresh_remote_view(cx));
    older(Ok(snapshot(&repo)));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.active_session(),
            Some(owner),
            "superseded refresh must not replace the remote session"
        );
        assert!(
            app.reads.is_loading(owner),
            "older completion must leave the newer request pending"
        );
    });
    git(&repo, &["branch", "-m", "newest"]);
    newer(Ok(snapshot(&repo)));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_ne!(
            app.active_session(),
            Some(owner),
            "accepted refresh replaces the old incarnation"
        );
        assert_eq!(
            app.view().status_summary.branch,
            "newest",
            "the newest snapshot must reach the remote view"
        );
        assert!(!app.app_sessions.is_attached(owner));
    });
    unmount(cx, app, window);
}
