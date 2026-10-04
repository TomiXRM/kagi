//! Issue #955 — the left sidebar and the right pane slide open and closed by
//! their width, with the bottom panel's timing (#950).
//!
//! A stand-in clock stops each motion in mid-flight. While a side pane
//! moves, only its clip's width changes: the Graph's commit list follows the
//! moving edge exactly (no gap, no overlap), keeps its scroll offset and its
//! selected row, and does not move at all while the right pane slides.
//! Switching the right slot between the Inspector and the Commit Panel is
//! no motion at all. A toggle in mid-flight turns around, rapid presses end
//! in the last state, `reduce_motion` jumps.

use std::time::{Duration, Instant};

use gpui::{
    point, px, AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton, Pixels,
    VisualTestAppContext,
};
use kagi::ui::commands::{ToggleCommitDetails, ToggleSidebar};
use kagi::ui::{e2e, theme, KagiApp};

/// Press on the sidebar's divider (the last pixels of its clip) and drag it
/// 40px to the left, drawing each step at clock time `at`.
fn drag_sidebar_divider(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    let clip = bounds(window, "sidebar-clip").expect("the sidebar is drawn");
    let start = point(clip.origin.x + clip.size.width - px(2.), clip.center().y);
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    let mut pointer = start;
    for step in [5., 20., 40.] {
        pointer = point(start.x - px(step), start.y);
        cx.simulate_mouse_move(window, pointer, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        frame(cx, window, at);
    }
    cx.simulate_mouse_up(window, pointer, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

use crate::macos::{build_fixture, git, mount, unmount};

const EXTRA_COMMITS: usize = 120;
const TOP_ROW: usize = 40;
const SELECTED: usize = 45;
const PROBES: [&str; 3] = ["sidebar-clip", "right-pane-clip", "commit-list-viewport"];

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    e2e::set_panel_motion_clock(Some(at));
    for name in PROBES {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the window");
}

fn bounds(window: AnyWindowHandle, name: &str) -> Option<Bounds<Pixels>> {
    e2e::control_bounds(window.window_id(), name)
}

fn width(window: AnyWindowHandle, name: &str) -> Option<f32> {
    bounds(window, name).map(|b| f32::from(b.size.width))
}

fn list_left(window: AnyWindowHandle) -> f32 {
    f32::from(
        bounds(window, "commit-list-viewport")
            .expect("the Graph is drawn")
            .origin
            .x,
    )
}

/// The Graph's scroll offset and selected row: the same pair means the same
/// rows at the same place, with the same one selected.
fn graph_state(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (f32, Option<usize>) {
    cx.read(|cx| {
        let ui = app.read(cx).ui();
        (
            -f32::from(ui.commit_scroll_handle.0.borrow().base_handle.offset().y),
            ui.selected,
        )
    })
}

/// The commit list starts exactly where the sidebar's clip ends.
fn assert_list_follows_sidebar(window: AnyWindowHandle, what: &str) {
    let list = list_left(window);
    match bounds(window, "sidebar-clip") {
        Some(clip) => {
            let edge = f32::from(clip.origin.x + clip.size.width);
            assert!(
                (list - edge).abs() <= 1.,
                "{what}: Graph at {list}, sidebar edge at {edge}"
            );
        }
        None => assert!(
            list <= 1.,
            "{what}: with no sidebar the Graph starts at the left ({list})"
        ),
    }
}

pub fn scenario_side_panel_motion(cx: &mut VisualTestAppContext) {
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
    let (app, window) = mount(cx, &repo);

    let t0 = Instant::now();
    app.update(cx, |app, cx| {
        app.select(SELECTED);
        cx.notify();
    });
    frame(cx, window, t0);
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_scroll_handle
            .scroll_to_item_strict(TOP_ROW, gpui::ScrollStrategy::Top)
    });
    frame(cx, window, t0);
    frame(cx, window, t0);
    let graph = graph_state(cx, &app);
    assert!(
        graph.0 > 0.,
        "the Graph is scrolled away from its first row"
    );
    assert_eq!(graph.1, Some(SELECTED));
    let sidebar_full = width(window, "sidebar-clip").expect("the sidebar starts shown");
    let right_full = width(window, "right-pane-clip").expect("the Inspector starts shown");
    assert_list_follows_sidebar(window, "at rest");

    // ── Left: hide (150ms ease-in), mid-flight, turn around, open ──
    cx.dispatch_action(window, ToggleSidebar);
    let mid = t0 + Duration::from_millis(100);
    frame(cx, window, mid);
    let narrow = width(window, "sidebar-clip").expect("still drawn while closing");
    assert!(
        0. < narrow && narrow < sidebar_full,
        "closing: {narrow} of {sidebar_full}"
    );
    assert_list_follows_sidebar(window, "sidebar closing");
    assert_eq!(
        graph_state(cx, &app),
        graph,
        "the Graph keeps its rows and selection"
    );
    cx.dispatch_action(window, ToggleSidebar);
    frame(cx, window, mid);
    let turned = width(window, "sidebar-clip").expect("drawn at the turn");
    assert!(
        (turned - narrow).abs() <= 1.,
        "no jump at the turn: {turned} vs {narrow}"
    );
    frame(cx, window, mid + Duration::from_millis(60));
    assert_list_follows_sidebar(window, "sidebar reopening");
    frame(cx, window, mid + Duration::from_millis(180));
    assert_eq!(width(window, "sidebar-clip"), Some(sidebar_full));
    assert_list_follows_sidebar(window, "sidebar open again");
    assert_eq!(graph_state(cx, &app), graph);

    // Rapid presses end in the last state (three: hidden).
    let t1 = mid + Duration::from_millis(400);
    frame(cx, window, t1);
    for step in 0..3u64 {
        cx.dispatch_action(window, ToggleSidebar);
        frame(cx, window, t1 + Duration::from_millis(15 * (step + 1)));
    }
    frame(cx, window, t1 + Duration::from_millis(400));
    assert!(!cx.read(|cx| app.read(cx).sidebar.visible));
    assert_eq!(
        width(window, "sidebar-clip"),
        None,
        "a hidden sidebar is not drawn"
    );
    assert_list_follows_sidebar(window, "sidebar hidden");
    assert_eq!(graph_state(cx, &app), graph);

    // ── Right: hide, mid-flight, the Graph does not move at all ──
    let t2 = t1 + Duration::from_millis(800);
    frame(cx, window, t2);
    let left_before = list_left(window);
    cx.dispatch_action(window, ToggleCommitDetails);
    frame(cx, window, t2 + Duration::from_millis(100));
    let right_mid = width(window, "right-pane-clip").expect("still drawn while closing");
    assert!(
        0. < right_mid && right_mid < right_full,
        "closing: {right_mid} of {right_full}"
    );
    assert_eq!(
        list_left(window),
        left_before,
        "the Graph's left edge stays put"
    );
    assert_eq!(graph_state(cx, &app), graph);
    frame(cx, window, t2 + Duration::from_millis(400));
    assert_eq!(width(window, "right-pane-clip"), None);
    // …and back open by its toggle.
    let t3 = t2 + Duration::from_millis(800);
    frame(cx, window, t3);
    cx.dispatch_action(window, ToggleCommitDetails);
    frame(cx, window, t3 + Duration::from_millis(60));
    let opening = width(window, "right-pane-clip").expect("drawn while opening");
    assert!(
        0. < opening && opening < right_full,
        "opening: {opening} of {right_full}"
    );
    frame(cx, window, t3 + Duration::from_millis(400));
    assert_eq!(width(window, "right-pane-clip"), Some(right_full));
    assert_eq!(graph_state(cx, &app), graph);

    // ── reduce_motion: jumps on the same clock time ──
    let tr = t3 + Duration::from_millis(600);
    theme::set_reduce_motion(true);
    frame(cx, window, tr);
    cx.dispatch_action(window, ToggleSidebar);
    frame(cx, window, tr);
    assert_eq!(
        width(window, "sidebar-clip"),
        Some(sidebar_full),
        "shown at once"
    );
    cx.dispatch_action(window, ToggleCommitDetails);
    frame(cx, window, tr);
    assert_eq!(width(window, "right-pane-clip"), None, "hidden at once");
    cx.dispatch_action(window, ToggleCommitDetails);
    frame(cx, window, tr);
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(right_full),
        "shown at once"
    );
    theme::set_reduce_motion(false);

    // ── #957 review: a divider drag in mid-slide writes no width ──
    // The divider sits at the clip's moving edge, not at the saved width;
    // the drag is ignored until the pane settles.
    let t6 = tr + Duration::from_millis(1000);
    frame(cx, window, t6);
    let saved_width = cx.read(|cx| app.read(cx).sidebar.width);
    cx.dispatch_action(window, ToggleSidebar);
    let t_drag = t6 + Duration::from_millis(60);
    frame(cx, window, t_drag);
    drag_sidebar_divider(cx, window, t_drag);
    assert_eq!(
        cx.read(|cx| app.read(cx).sidebar.width),
        saved_width,
        "a drag while the sidebar slides does not rewrite its width"
    );
    // At rest the same drag resizes it, so the check above is not vacuous.
    cx.dispatch_action(window, ToggleSidebar);
    let t7 = t_drag + Duration::from_millis(1000);
    frame(cx, window, t7);
    assert_eq!(width(window, "sidebar-clip"), Some(sidebar_full));
    drag_sidebar_divider(cx, window, t7);
    assert_ne!(
        cx.read(|cx| app.read(cx).sidebar.width),
        saved_width,
        "at rest the divider drag resizes the sidebar"
    );
    app.update(cx, |app, cx| {
        app.sidebar.width = saved_width;
        cx.notify();
    });

    // ── Inspector → Commit Panel in the same slot: no motion ──
    std::fs::write(repo.join("README.md"), "# changed by the scenario\n").unwrap();
    app.update(cx, |app, cx| app.reload(cx));
    crate::recovery_operations::wait_idle(cx, &app);
    let t4 = t7 + Duration::from_millis(1000);
    frame(cx, window, t4);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    frame(cx, window, t4);
    assert!(
        cx.read(|cx| app.read(cx).ui().commit_panel_open),
        "the Commit Panel took the slot"
    );
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(right_full),
        "switching the right slot's pane is no motion: full width on the same frame"
    );
    frame(cx, window, t4 + Duration::from_millis(60));
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(right_full),
        "…and on the frames after it: one pane does not close while the other opens"
    );
    app.update(cx, |app, cx| {
        app.select(SELECTED);
        cx.notify();
    });
    let t5 = t4 + Duration::from_millis(500);
    frame(cx, window, t5);
    assert!(
        !cx.read(|cx| app.read(cx).ui().commit_panel_open),
        "the Inspector is back"
    );
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(right_full),
        "and back, no motion"
    );
    frame(cx, window, t5 + Duration::from_millis(60));
    assert_eq!(width(window, "right-pane-clip"), Some(right_full));

    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(reduce_before);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS side_panel_motion: sidebar and right pane slide by width with the shared timing, the Graph follows the edge with the same rows and selection, an Inspector/Commit Panel swap does not animate, reduce_motion is instant"
    );
}

/// #1001: selecting a commit changes the right slot even when the View toggle
/// stays on. Escape and a second click both reverse from the painted width.
pub fn scenario_right_selection_motion(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduce_before = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let t0 = Instant::now();
    frame(cx, window, t0);
    assert_eq!(width(window, "right-pane-clip"), None);
    let list = list_left(window);

    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, window, t0);
    frame(cx, window, t0 + Duration::from_millis(60));
    let opening = width(window, "right-pane-clip").expect("selection opens Inspector");
    let full = f32::from(theme::scaled_px(
        cx.read(|cx| app.read(cx).panel_width + 4.),
    ));
    assert!(
        0. < opening && opening < full,
        "opening: {opening} of {full}"
    );
    assert_eq!(list_left(window), list, "Graph stays at the same edge");

    // Esc while opening: the first closing frame keeps the same painted width,
    // then retracts. The selection is gone while exit-only content still draws.
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert_eq!(graph_state(cx, &app).1, None, "Escape clears the selection");
    frame(cx, window, t0 + Duration::from_millis(60));
    let turned = width(window, "right-pane-clip").expect("exit clip remains");
    assert!(
        (turned - opening).abs() <= 1.,
        "turn: {turned} vs {opening}"
    );
    frame(cx, window, t0 + Duration::from_millis(110));
    let closing = width(window, "right-pane-clip").expect("closing Inspector still drawn");
    assert!(
        0. < closing && closing < turned,
        "closing: {closing} of {turned}"
    );
    frame(cx, window, t0 + Duration::from_millis(310));
    assert_eq!(width(window, "right-pane-clip"), None);

    // A click on the selected row is another deselection route. Reverse that
    // closing clip by clicking the row again before it reaches zero.
    let t1 = t0 + Duration::from_millis(400);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, window, t1);
    frame(cx, window, t1 + Duration::from_millis(300));
    assert_eq!(width(window, "right-pane-clip"), Some(full));
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, window, t1 + Duration::from_millis(360));
    let mid = width(window, "right-pane-clip").expect("click closes Inspector");
    assert!(0. < mid && mid < full);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, window, t1 + Duration::from_millis(360));
    let reversed = width(window, "right-pane-clip").expect("click reopens Inspector");
    assert!((reversed - mid).abs() <= 1., "turn: {reversed} vs {mid}");
    frame(cx, window, t1 + Duration::from_millis(600));
    assert_eq!(width(window, "right-pane-clip"), Some(full));

    // Reduced motion uses the same selection and Escape paths, without a
    // single intermediate width.
    let tr = t1 + Duration::from_millis(700);
    theme::set_reduce_motion(true);
    frame(cx, window, tr);
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    frame(cx, window, tr);
    assert_eq!(width(window, "right-pane-clip"), None, "instant close");
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    frame(cx, window, tr);
    assert_eq!(width(window, "right-pane-clip"), Some(full), "instant open");
    theme::set_reduce_motion(false);

    // Tab entry is a whole-workspace change. The destination starts with no
    // Inspector; returning to the selected source shows its full width at once.
    let other_fixture = build_fixture();
    let other = other_fixture.path().canonicalize().unwrap();
    let ts = tr + Duration::from_millis(500);
    frame(cx, window, ts);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other, cx));
    });
    frame(cx, window, ts);
    assert_eq!(
        width(window, "right-pane-clip"),
        None,
        "no old tab exit clip"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    frame(cx, window, ts);
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(full),
        "tab return jumps"
    );

    // A mode change resets both sides of the boundary: the right pane is
    // absent in PRs, then the retained Graph selection is full-width on return.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    frame(cx, window, ts);
    assert_eq!(
        width(window, "right-pane-clip"),
        None,
        "PRs have no Inspector"
    );
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    frame(cx, window, ts);
    assert_eq!(
        width(window, "right-pane-clip"),
        Some(full),
        "Graph mode return is instant"
    );

    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(reduce_before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS right_selection_motion");
}
