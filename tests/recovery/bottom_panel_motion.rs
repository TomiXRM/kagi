//! Issue #950 — the bottom panel slides open and closed by its height.
//!
//! A stand-in clock stops the motion in mid-flight. While it moves, only the
//! clipping box's height changes: the panel inside keeps its full height (so
//! the Terminal's grid and PTY size stay put), and the Graph above keeps the
//! same top row. Cmd+J in mid-flight turns around from the current height
//! and lands at the right end; `reduce_motion` jumps to the end at once.

use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, theme, KagiApp};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

/// Commits on top of the fixture, so the Graph scrolls.
const EXTRA_COMMITS: usize = 120;
/// The Graph row scrolled to the top before the panel moves.
const TOP_ROW: usize = 40;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for name in ["bottom-panel", "bottom-panel-clip"] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the window");
}

fn height(window: AnyWindowHandle, name: &str) -> Option<f32> {
    e2e::control_bounds(window.window_id(), name).map(|b: Bounds<Pixels>| f32::from(b.size.height))
}

/// The Graph's scroll position: how far its rows are scrolled up. The same
/// value means the same row at the top, at the same pixel.
fn top_row(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> f32 {
    cx.read(|cx| {
        -f32::from(
            app.read(cx)
                .ui()
                .commit_scroll_handle
                .0
                .borrow()
                .base_handle
                .offset()
                .y,
        )
    })
}

/// Draw the frame at `at` on the stand-in clock.
fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    e2e::set_panel_motion_clock(Some(at));
    draw(cx, window);
}

pub fn scenario_bottom_panel_motion(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduce_before = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    for n in 0..EXTRA_COMMITS {
        git(
            fixture.path(),
            &["commit", "-q", "--allow-empty", "-m", &format!("extra {n}")],
        );
    }
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);

    let t0 = Instant::now();
    frame(cx, window, t0);
    assert!(
        cx.read(|cx| app.read(cx).bottom_panel_open),
        "the panel starts open"
    );
    let full = height(window, "bottom-panel-clip").expect("the open panel is drawn");
    assert_eq!(height(window, "bottom-panel"), Some(full));
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_scroll_handle
            .scroll_to_item_strict(TOP_ROW, gpui::ScrollStrategy::Top)
    });
    frame(cx, window, t0);
    frame(cx, window, t0);
    let top = top_row(cx, &app);
    assert!(top > 0., "the Graph is scrolled away from its first row");

    // Close: ease-in over 150ms. In mid-flight only the clip shrinks.
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t0);
    assert_eq!(
        height(window, "bottom-panel-clip"),
        Some(full),
        "no jump at the start"
    );
    let t_mid = t0 + Duration::from_millis(100);
    frame(cx, window, t_mid);
    let mid = height(window, "bottom-panel-clip").expect("still drawn in mid-flight");
    assert!(
        0. < mid && mid < full,
        "closing in mid-flight: {mid} of {full}"
    );
    assert_eq!(
        height(window, "bottom-panel"),
        Some(full),
        "the panel keeps its full height inside the clip (no PTY resize)"
    );
    assert_eq!(
        top_row(cx, &app),
        top,
        "the Graph keeps its top row while the panel moves"
    );

    // Cmd+J in mid-flight turns around from the current height…
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t_mid);
    let turned = height(window, "bottom-panel-clip").expect("drawn at the turn");
    assert!(
        (turned - mid).abs() <= 1.,
        "no jump at the turn: {turned} vs {mid}"
    );
    // …and opens again within the opening time.
    frame(cx, window, t_mid + Duration::from_millis(180));
    assert_eq!(height(window, "bottom-panel-clip"), Some(full));
    assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
    assert_eq!(top_row(cx, &app), top);

    // Two more presses in quick succession end closed: the last one wins.
    let t1 = t_mid + Duration::from_millis(400);
    frame(cx, window, t1);
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t1 + Duration::from_millis(20));
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t1 + Duration::from_millis(40));
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t1 + Duration::from_millis(400));
    assert!(!cx.read(|cx| app.read(cx).bottom_panel_open));
    assert_eq!(
        height(window, "bottom-panel-clip"),
        None,
        "a closed panel is not drawn"
    );
    assert_eq!(top_row(cx, &app), top, "closing keeps the Graph's top row");

    // reduce_motion: the very next frame is the end, on the same clock time.
    theme::set_reduce_motion(true);
    let t2 = t1 + Duration::from_millis(800);
    frame(cx, window, t2);
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t2);
    assert_eq!(
        height(window, "bottom-panel-clip"),
        Some(full),
        "with reduce_motion the panel opens at once"
    );
    cx.simulate_keystrokes(window, "cmd-j");
    frame(cx, window, t2);
    assert_eq!(
        height(window, "bottom-panel-clip"),
        None,
        "and closes at once"
    );
    assert_eq!(top_row(cx, &app), top);

    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(reduce_before);
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "moving a panel is not a write"
    );
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS bottom_panel_motion: the clip eases 150ms/180ms, turns around mid-flight, keeps the panel at full height and the Graph's top row, and reduce_motion is instant"
    );
}
