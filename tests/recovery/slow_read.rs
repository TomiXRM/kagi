//! #355: a read that runs two seconds explains itself in the busy snackbar,
//! and Skip shows its result as unknown until the next read counts again.
//!
//! The production reload runs; only its snapshot is held in the ahead/behind
//! phase (`KagiApp::hold_next_snapshot_for_e2e`) and released by the test. The
//! clock is the test dispatcher's, advanced in the tracker's own 250 ms ticks.
use std::path::Path;
use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
    KagiApp,
};
use kagi_git::AheadBehind;

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};

const TICK: Duration = Duration::from_millis(250);

/// `main` one commit ahead of a live bare `origin`.
fn ahead_fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let fixture = build_fixture();
    let remote = tempfile::tempdir().expect("remote tempdir");
    let bare = remote.path().join("origin.git");
    let bare = bare.to_str().unwrap();
    git(fixture.path(), &["init", "--bare", "-q", bare]);
    git(fixture.path(), &["remote", "add", "origin", bare]);
    git(fixture.path(), &["push", "-q", "-u", "origin", "main"]);
    git(
        fixture.path(),
        &["commit", "-q", "--allow-empty", "-m", "ahead"],
    );
    (fixture, remote)
}

fn advance(cx: &mut VisualTestAppContext, total: Duration) {
    let mut left = total;
    while !left.is_zero() {
        cx.advance_clock(TICK);
        cx.run_until_parked();
        left = left.saturating_sub(TICK);
    }
}

/// Draw one frame; report which snackbar controls it drew.
fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> (bool, bool) {
    let id = window.window_id();
    e2e::clear_control_bounds(id, "busy-snackbar-advice");
    e2e::clear_control_bounds(id, "busy-snackbar-skip");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    (
        e2e::control_bounds(id, "busy-snackbar-advice").is_some(),
        e2e::control_bounds(id, "busy-snackbar-skip").is_some(),
    )
}

fn drawn_write_advice(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Option<String> {
    e2e::take_busy_advice();
    let (advice, skip) = drawn(cx, window);
    let text = e2e::take_busy_advice();
    assert_eq!(
        advice,
        text.is_some(),
        "the rendered advice owns the control"
    );
    assert!(!skip, "running writes cannot be skipped");
    text
}

fn counts(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<Option<AheadBehind>> {
    cx.read(|cx| {
        app.read(cx)
            .view()
            .branch_upstream_info
            .get("main")
            .map(|upstream| upstream.counts)
    })
}

fn wait_for_counts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    want: Option<Option<AheadBehind>>,
    what: &str,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while counts(cx, app) != want {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: got {:?}",
            counts(cx, app)
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn scenario_slow_read_explained(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, _remote) = ahead_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let counted = Some(Some(AheadBehind {
        ahead: 1,
        behind: 0,
    }));
    let (app, window) = mount(cx, &repo);
    wait_for_counts(cx, &app, counted, "the first read counts");
    let original_language = i18n::lang();

    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_snapshot_for_e2e(hold);
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();

        // Under two seconds in the ahead/behind phase: nothing to explain.
        // The first tick sees the phase; 1.75 s after it is still not slow.
        advance(cx, TICK * 8);
        assert_eq!(
            cx.read(|cx| app.read(cx).slow_read_shown_for_e2e()),
            None,
            "{language:?}: a read under two seconds must not be explained"
        );
        assert_eq!(drawn(cx, window), (false, false));

        // Past two seconds: the snackbar explains it and offers Skip.
        advance(cx, TICK * 2);
        assert_eq!(
            cx.read(|cx| app.read(cx).slow_read_shown_for_e2e()),
            Some("ahead-behind"),
            "{language:?}: a two-second ahead/behind must be explained"
        );
        assert_eq!(drawn(cx, window), (true, true));

        // Skip: the explanation goes, and the released read counts nothing.
        let skip = e2e::control_bounds(window.window_id(), "busy-snackbar-skip").unwrap();
        cx.simulate_click(window, skip.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| app.read(cx).slow_read_shown_for_e2e()), None);
        release.send(());
        wait_for_counts(cx, &app, Some(None), "a skipped count is unknown");
        cx.read(|cx| {
            let summary = &app.read(cx).view().status_summary;
            assert_eq!((summary.ahead, summary.behind), (None, None));
            assert!(!summary.no_upstream, "unknown is not 'no upstream'");
        });
        assert_eq!(drawn(cx, window), (false, false));

        // Skip is not a setting: the next read counts again.
        app.update(cx, |app, cx| app.reload_external(cx));
        wait_for_counts(cx, &app, counted, "the next read counts again");
        eprintln!("[gui-e2e] ok slow_read_explained/{language:?}");
    }

    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS slow_read_explained: EN/JA 2s threshold, Skip → unknown → recount");
}

/// The real fetch is admitted through the local writer lease. Only its backend
/// dispatch is held, so the virtual clock can inspect the running snackbar.
pub fn scenario_slow_write_explained(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let (fixture, _remote) = ahead_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    wait_for_counts(
        cx,
        &app,
        Some(Some(AheadBehind {
            ahead: 1,
            behind: 0,
        })),
        "the initial snapshot",
    );
    let original_language = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_fetch_for_e2e(hold);
        let before = kagi_ui_core::klog::tail()
            .into_iter()
            .filter(|line| line == "[kagi] busy: slow write fetch after 2s")
            .count();
        app.update(cx, |app, cx| app.fetch_async(false, cx));
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.app_sessions.has_leases(), "fetch has been admitted");
            assert_eq!(app.write_busy_op, Some("fetch"));
        });
        assert_eq!(drawn_write_advice(cx, window), None);
        advance(cx, TICK * 7);
        assert_eq!(drawn_write_advice(cx, window), None, "not slow at 1.75 s");
        advance(cx, TICK);
        let text = drawn_write_advice(cx, window).expect("2s fetch draws an explanation");
        let expected_reason = match language {
            Lang::En => "network: waiting for the remote",
            Lang::Ja => "network: remote の応答を待っています",
        };
        assert_eq!(
            text,
            format!("{expected_reason} · 2 s"),
            "reason and seconds only"
        );
        assert!(!text.contains("cannot be interrupted"), "{text}");
        advance(cx, TICK * 8);
        let text = drawn_write_advice(cx, window).expect("running fetch updates seconds");
        assert_eq!(
            text,
            format!("{expected_reason} · 4 s"),
            "reason and seconds only"
        );
        let after = kagi_ui_core::klog::tail()
            .into_iter()
            .filter(|line| line == "[kagi] busy: slow write fetch after 2s")
            .count();
        assert_eq!(after, before + 1, "one slow log per admitted fetch");

        if language == Lang::En {
            let (snapshot_hold, snapshot_release) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(snapshot_hold);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, TICK * 10);
            e2e::take_busy_advice();
            assert_eq!(
                drawn(cx, window),
                (true, true),
                "write advice wins while the slow read still offers Skip"
            );
            let advice = e2e::take_busy_advice().expect("drawn write advice");
            assert!(
                advice.contains("network: waiting for the remote"),
                "{advice}"
            );
            let skip = e2e::control_bounds(window.window_id(), "busy-snackbar-skip").unwrap();
            cx.simulate_click(window, skip.center(), Modifiers::none());
            cx.run_until_parked();
            snapshot_release.send(());
            cx.run_until_parked();
        }

        release.send(());
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
            cx.run_until_parked();
            assert!(std::time::Instant::now() < deadline, "fetch did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(drawn_write_advice(cx, window), None, "end clears advice");
        cx.read(|cx| assert!(!app.read(cx).app_sessions.has_leases()));
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS slow_write_explained: EN/JA fetch admission, 2s, 4s, release");
}

/// #995 review: a slow write whose guard is dropped by a panic stops being a
/// running write. The ticker's tick that finds it ended must notify on its
/// own, so the advice drawn on the last frame is withdrawn rather than left
/// until an unrelated frame.
pub fn scenario_slow_write_cleared_after_panic(cx: &mut VisualTestAppContext) {
    let (fixture, _remote) = ahead_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    cx.run_until_parked();

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_fetch_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.panic_next_fetch_for_e2e();
        app.fetch_async(false, cx);
    });
    cx.run_until_parked();
    advance(cx, TICK * 8);
    assert!(
        drawn_write_advice(cx, window).is_some(),
        "precondition: the held fetch is explained after 2 s"
    );

    // The task panics once released; its completion and abandonment land.
    release.send(());
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.running_lease().is_none()),
        "precondition: the panicked guard is no longer a running write"
    );

    // From here, count only what the ticker itself sends on its next tick.
    let notified = std::rc::Rc::new(std::cell::Cell::new(0usize));
    {
        let notified = notified.clone();
        cx.update(|cx| {
            cx.observe(&app, move |_, _| notified.set(notified.get() + 1))
                .detach()
        });
    }
    cx.advance_clock(TICK);
    cx.run_until_parked();
    assert!(
        notified.get() >= 1,
        "slow-write-cleared-after-panic: the tick that found the write ended did not notify"
    );
    assert_eq!(
        drawn_write_advice(cx, window),
        None,
        "slow-write-cleared-after-panic: the advice outlived the panicked write"
    );
    // The ticker stopped: no further self-notifications.
    let after = notified.get();
    cx.advance_clock(TICK * 4);
    cx.run_until_parked();
    assert_eq!(notified.get(), after, "the ended ticker kept running");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS slow_write_cleared_after_panic");
}

/// #996: the snapshot task retains its writer lease while its backend dispatch
/// is held, so the shared #995 ticker explains its real elapsed write.
pub fn scenario_slow_write_snapshot(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let original_language = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let before_refs = crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
            .lines()
            .count();
        let (hold, release) = deferred::<()>(cx);
        KagiApp::hold_next_snapshot_write_for_e2e(hold);
        cx.dispatch_action(window, kagi::ui::commands::CreateSnapshot);
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(state.write_busy_op, Some("snapshot"));
            assert!(state.app_sessions.running_lease().is_some());
            assert_eq!(
                e2e::busy_snackbar_label(state),
                Some(i18n::busy_label("snapshot"))
            );
        });
        assert_eq!(drawn_write_advice(cx, window), None);
        advance(cx, TICK * 7);
        assert_eq!(drawn_write_advice(cx, window), None, "not slow at 1.75 s");
        advance(cx, TICK);
        let reason = match language {
            Lang::En => "stash: updating saved changes",
            Lang::Ja => "stash: 保存した変更を更新中",
        };
        assert_eq!(
            drawn_write_advice(cx, window).as_deref(),
            Some(format!("{reason} · 2 s").as_str())
        );
        advance(cx, TICK * 8);
        assert_eq!(
            drawn_write_advice(cx, window).as_deref(),
            Some(format!("{reason} · 4 s").as_str())
        );
        assert_eq!(
            crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
                .lines()
                .count(),
            before_refs,
            "held snapshot must not run before release"
        );
        release.send(());
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).app_sessions.running_lease().is_some()) {
            cx.run_until_parked();
            assert!(
                std::time::Instant::now() < deadline,
                "snapshot did not settle"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(drawn_write_advice(cx, window), None, "end clears advice");
        assert!(
            crate::macos::for_each_ref(&repo, "refs/kagi/snapshots/")
                .lines()
                .count()
                > before_refs,
            "released snapshot must create a ref"
        );
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS slow_write_snapshot: EN/JA 2s, 4s, release");
}

/// #996: merge Continue's background staging must use the same lease clock as
/// fetch and snapshot, then remove its explanation on completion.
pub fn scenario_slow_write_conflict_continue(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    i18n::set_lang(Lang::En);
    let fixture = crate::app_conflict::content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
            view.mode
                .as_mut()
                .unwrap()
                .buffer
                .apply_choice(Path::new("file.txt"), kagi_git::ResolutionChoice::Incoming)
                .unwrap();
        });
    });
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_continue_merge_for_e2e(hold);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.write_busy_op, Some("conflict-continue"));
        assert!(state.app_sessions.running_lease().is_some());
        assert_eq!(
            e2e::busy_snackbar_label(state),
            Some(i18n::busy_label("conflict-continue"))
        );
        assert!(!state.ui().commit_panel_open);
    });
    assert_eq!(drawn_write_advice(cx, window), None);
    advance(cx, TICK * 8);
    assert_eq!(
        drawn_write_advice(cx, window).as_deref(),
        Some("conflict resolution: updating local files · 2 s")
    );
    advance(cx, TICK * 8);
    assert_eq!(
        drawn_write_advice(cx, window).as_deref(),
        Some("conflict resolution: updating local files · 4 s")
    );
    release.send(());
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while cx.read(|cx| app.read(cx).app_sessions.running_lease().is_some()) {
        cx.run_until_parked();
        assert!(
            std::time::Instant::now() < deadline,
            "merge Continue did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(cx.read(|cx| app.read(cx).ui().commit_panel_open));
    assert_eq!(drawn_write_advice(cx, window), None, "end clears advice");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS slow_write_conflict_continue: 2s, 4s, release");
}
