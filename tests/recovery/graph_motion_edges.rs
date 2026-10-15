//! Empty-list boundary for the Graph's retained WIP presentation.
use crate::macos::{git, mount, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::{motion, theme};
use std::time::{Duration, Instant};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "graph-wip-ghost");
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

pub fn scenario_graph_activation_motion_reset(cx: &mut VisualTestAppContext) {
    use crate::evidence_support::deferred;
    use crate::graph_selection_wash::pixel;
    use crate::macos::build_fixture;
    use kagi::ui::KagiApp;
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let first = build_fixture();
    let other = build_fixture();
    let (app, window) = mount(cx, first.path());
    let wait_read = |cx: &mut VisualTestAppContext, generation| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).ui().view_publish_gen == generation) {
            cx.run_until_parked();
            assert!(Instant::now() < deadline, "held full read must publish");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    draw(cx, window);
    let original = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().canonicalize().unwrap(), cx));
    });
    cx.run_until_parked();
    draw(cx, window);
    let file = first.path().join("while-away");
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_snapshot_for_e2e(hold);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    assert!(
        !cx.read(|cx| app.read(cx).view().is_dirty),
        "cached clean read before revalidation"
    );
    // Render the cached model before the async read completes: this consumes
    // the switch-time reset, so acceptance must reset presentation again.
    draw(cx, window);
    std::fs::write(&file, "arrived while inactive\n").unwrap();
    let generation = cx.read(|cx| app.read(cx).ui().view_publish_gen);
    release.send(());
    wait_read(cx, generation);
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).view().is_dirty));
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none());
    let accepted = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    assert!((f32::from(accepted.top() - original.top() - original.size.height)).abs() <= 1.,
        "activation arrivals are already at final geometry, not live WIP insertion: {accepted:?}, {original:?}");
    let accepted_fill = pixel(cx, window, "graph-wip-row");
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    draw(cx, window);
    assert_eq!(pixel(cx, window, "graph-wip-row"), accepted_fill);
    // The exclusion is read-specific: a subsequent cheap, live WIP change
    // must still animate rather than inheriting a persistent instant flag.
    std::fs::remove_file(&file).unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_some());
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(400)));
    draw(cx, window);

    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), &["init", "-q", "-b", "main"]);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_snapshot_for_e2e(hold);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(unborn.path().canonicalize().unwrap(), cx));
    });
    cx.run_until_parked();
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).view().rows.is_empty()));
    std::fs::write(unborn.path().join("initial-dirty"), "initial WIP\n").unwrap();
    let generation = cx.read(|cx| app.read(cx).ui().view_publish_gen);
    release.send(());
    wait_read(cx, generation);
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).view().is_dirty));
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none());
    let initial_fill = pixel(cx, window, "graph-wip-row");
    let initial_bounds = e2e::control_bounds(window.window_id(), "graph-wip-row").unwrap();
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(490)));
    draw(cx, window);
    assert_eq!(
        pixel(cx, window, "graph-wip-row"),
        initial_fill,
        "initial unborn read must not fade in"
    );
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(800)));
    draw(cx, window);
    assert_eq!(pixel(cx, window, "graph-wip-row"), initial_fill);
    assert_eq!(
        e2e::control_bounds(window.window_id(), "graph-wip-row").unwrap(),
        initial_bounds
    );
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}
