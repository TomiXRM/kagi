//! Empty-list boundary for the Graph's retained WIP presentation.
use crate::macos::{git, mount, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{motion, theme};
use std::time::{Duration, Instant};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

pub fn scenario_graph_unborn_wip_motion(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = tempfile::tempdir().unwrap();
    git(fixture.path(), &["init", "-q", "-b", "main"]);
    let file = fixture.path().join("untracked");
    std::fs::write(&file, "unborn WIP\n").unwrap();
    let (app, window) = mount(cx, fixture.path());
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    draw(cx, window);
    let original = e2e::control_bounds(window.window_id(), "graph-wip-row").unwrap();
    assert!(cx.read(|cx| app.read(cx).view().rows.is_empty()));
    std::fs::remove_file(&file).unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    let ghost = e2e::control_bounds(window.window_id(), "graph-wip-ghost")
        .expect("last WIP remains mounted over the unborn empty state");
    assert!((f32::from(ghost.top() - original.top())).abs() <= 1.);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    e2e::clear_control_bounds(window.window_id(), "graph-wip-ghost");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_some());
    // Reappearance before close completes returns the same logical WIP to a
    // real, interactive list row instead of leaving two presentations mounted.
    std::fs::write(&file, "reappeared\n").unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "graph-wip-ghost");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none());
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-row").is_some());
    std::fs::remove_file(&file).unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(400)));
    e2e::clear_control_bounds(window.window_id(), "graph-wip-ghost");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none());
    assert!(cx.read(|cx| !app.read(cx).view().is_dirty));
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}
