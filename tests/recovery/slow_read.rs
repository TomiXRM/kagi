//! #355: a read that runs two seconds explains itself in the busy snackbar,
//! and Skip shows its result as unknown until the next read counts again.
//!
//! The production reload runs; only its snapshot is held in the ahead/behind
//! phase (`KagiApp::hold_next_snapshot_for_e2e`) and released by the test. The
//! clock is the test dispatcher's, advanced in the tracker's own 250 ms ticks.
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
