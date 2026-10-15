//! Graph motion lifetime and native pointer ownership boundaries.
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

pub fn scenario_graph_insert_motion_hit_targets(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = crate::macos::build_fixture();
    for index in 0..40 {
        git(
            fixture.path(),
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("tail fixture {index}"),
            ],
        );
    }
    for zoom in [1., 1.67] {
        theme::set_zoom(zoom);
        let (app, window) = mount(cx, fixture.path());
        let t = Instant::now();
        motion::set_clock_for_e2e(Some(t));
        draw(cx, window);
        let height = e2e::control_bounds(window.window_id(), "graph-row-0")
            .unwrap()
            .size
            .height;
        let viewport = e2e::control_bounds(window.window_id(), "commit-list-viewport").unwrap();
        for index in 0..4 {
            git(
                fixture.path(),
                &[
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    &format!("batch arrival {index}"),
                ],
            );
        }
        app.update(cx, |app, cx| app.reload(cx));
        cx.run_until_parked();
        draw(cx, window);
        motion::set_clock_for_e2e(Some(t + Duration::from_millis(37)));
        // A fractional scroll after the splice starts makes the final-index
        // viewport tail partial while displaced extra rows remain visible.
        let pitch = f32::from(height);
        let scroll =
            (pitch * 0.75 - f32::from(viewport.size.height).rem_euclid(pitch)).rem_euclid(pitch);
        cx.read(|cx| {
            app.read(cx)
                .ui()
                .commit_scroll_handle
                .0
                .borrow()
                .base_handle
                .set_offset(gpui::point(gpui::px(0.), gpui::px(-scroll)))
        });
        let count = cx.read(|cx| app.read(cx).view().rows.len());
        for index in 0..count {
            e2e::clear_control_bounds(window.window_id(), &format!("graph-row-{index}"));
        }
        draw(cx, window);
        let (tail, bounds) = (0..count)
            .filter_map(|index| {
                let bounds =
                    e2e::control_bounds(window.window_id(), &format!("graph-row-{index}"))?;
                (bounds.top() < viewport.bottom()).then_some((index, bounds))
            })
            .last()
            .unwrap();
        let requested_end = ((f32::from(viewport.size.height) + scroll) / pitch).ceil() as usize;
        assert!(
            tail >= requested_end,
            "must click an actually realized fringe row at zoom {zoom}"
        );
        let click =
            |cx: &mut VisualTestAppContext, index: usize, bounds: gpui::Bounds<gpui::Pixels>| {
                let expected = cx.read(|cx| app.read(cx).view().rows[index].id.clone());
                let point = gpui::point(
                    bounds.center().x,
                    bounds.top() + gpui::px(3.).min((viewport.bottom() - bounds.top()) / 2.),
                );
                cx.simulate_mouse_move(window, point, None, gpui::Modifiers::none());
                cx.simulate_click(window, point, gpui::Modifiers::none());
                draw(cx, window);
                assert_eq!(
                    cx.read(|cx| {
                        let app = app.read(cx);
                        app.ui()
                            .selected
                            .map(|selected| app.view().rows[selected].id.clone())
                    }),
                    Some(expected),
                    "painted commit {index} owns its visible hit strip at zoom {zoom}"
                );
                app.update(cx, |app, cx| {
                    app.select_headless(index);
                    cx.notify();
                });
            };
        click(cx, tail, bounds);
        cx.read(|cx| {
            app.read(cx)
                .ui()
                .commit_scroll_handle
                .0
                .borrow()
                .base_handle
                .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)))
        });
        draw(cx, window);
        let first = e2e::control_bounds(window.window_id(), "graph-row-1").unwrap();
        let next = e2e::control_bounds(window.window_id(), "graph-row-2").unwrap();
        let gap = next.top() - first.top();
        assert!(
            gap > gpui::px(3.) && gap < height,
            "batch rows overlap during insertion"
        );
        click(cx, 2, next);
        motion::set_clock_for_e2e(None);
        unmount(cx, app, window);
    }
    theme::set_reduce_motion(reduced);
}

fn graph_targets_fixture() -> tempfile::TempDir {
    let fixture = crate::macos::build_fixture();
    std::fs::write(fixture.path().join("README.md"), "stash target\n").unwrap();
    git(
        fixture.path(),
        &["stash", "push", "-q", "-m", "row interaction"],
    );
    std::fs::write(fixture.path().join("current-untracked"), "current WIP\n").unwrap();
    fixture
}

pub fn scenario_graph_over_row_folder_drop(cx: &mut VisualTestAppContext) {
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let initial = graph_targets_fixture();
    for row in ["graph-row-0", "graph-wip-row", "graph-stash-row-0"] {
        let target = crate::macos::build_fixture();
        let (app, window) = mount(cx, initial.path());
        draw(cx, window);
        let position = e2e::control_bounds(window.window_id(), row)
            .unwrap()
            .center();
        cx.simulate_event(
            window,
            gpui::FileDropEvent::Entered {
                position,
                paths: gpui::ExternalPaths(vec![target.path().to_path_buf()].into_iter().collect()),
            },
        );
        draw(cx, window);
        cx.simulate_event(window, gpui::FileDropEvent::Submit { position });
        cx.run_until_parked();
        draw(cx, window);
        cx.read(|cx| {
            let app = app.read(cx);
            assert_eq!(
                app.tabs.len(),
                2,
                "external folder drop over {row} reaches the ancestor"
            );
            assert_eq!(
                app.repo_path.as_deref(),
                Some(target.path().canonicalize().unwrap().as_path())
            );
        });
        unmount(cx, app, window);
    }
}

pub fn scenario_graph_stash_pointer_focus(cx: &mut VisualTestAppContext) {
    use gpui::Focusable;
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let fixture = graph_targets_fixture();
    let (app, window) = mount(cx, fixture.path());
    draw(cx, window);
    let input = cx.read(|cx| {
        app.read(cx)
            .sidebar
            .filter
            .clone()
            .expect("mounted sidebar filter")
    });
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.focus(window, cx));
    })
    .unwrap();
    assert!(
        cx.update_window(window, |_, window, cx| {
            input.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap(),
        "start from an actual text input, not Graph"
    );
    let row = e2e::control_bounds(window.window_id(), "graph-stash-row-0").unwrap();
    cx.simulate_mouse_move(window, row.center(), None, gpui::Modifiers::none());
    cx.simulate_click(window, row.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx)
                .graph_focus
                .as_ref()
                .unwrap()
                .is_focused(window)
        })
        .unwrap(),
        "stash pointer selection retains ancestor Graph focus-on-mouse-down"
    );
    cx.simulate_keystrokes(window, "down");
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).ui().selected.is_some()),
        "arrows navigate Graph, not the prior input"
    );
    drop(input);
    unmount(cx, app, window);
}

pub fn scenario_graph_mode_motion_reset(cx: &mut VisualTestAppContext) {
    use crate::graph_selection_wash::pixel;
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    for mode in ["pr", "issues", "editor"] {
        let fixture = crate::macos::build_fixture();
        let file = fixture.path().join("live-wip");
        std::fs::write(&file, "visible WIP\n").unwrap();
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            app.select_headless(0);
            cx.notify();
        });
        draw(cx, window);
        let original = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
        let selected_fill = pixel(cx, window, "graph-row-0");
        let neutral_fill = pixel(cx, window, "graph-row-1");
        let t = Instant::now();
        motion::set_clock_for_e2e(Some(t));
        app.update(cx, |app, cx| {
            app.select_headless(1);
            cx.notify();
        });
        std::fs::remove_file(&file).unwrap();
        app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
        cx.run_until_parked();
        draw(cx, window);
        motion::set_clock_for_e2e(Some(t + Duration::from_millis(37)));
        draw(cx, window);
        let mid = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
        assert!(
            mid.top() < original.top() && mid.top() > original.top() - original.size.height,
            "start with real partial displacement before {mode}"
        );
        assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_some());
        let mid_fill = pixel(cx, window, "graph-row-0");
        assert_ne!(mid_fill, selected_fill);
        assert_ne!(
            mid_fill, neutral_fill,
            "start with a genuine partial selection wash"
        );
        app.update(cx, |app, cx| match mode {
            "pr" => app.show_pr_mode(cx),
            "issues" => app.show_issues_mode(cx),
            "editor" => app.show_editor_mode(cx),
            _ => unreachable!(),
        });
        cx.run_until_parked();
        e2e::clear_control_bounds(window.window_id(), "graph-row-0");
        draw(cx, window);
        assert!(
            e2e::control_bounds(window.window_id(), "graph-row-0").is_none(),
            "{mode} actually hides the retained Graph"
        );
        // A full read accepted while hidden must not queue an insertion for the
        // first visible frame, even though this is not a tab activation read.
        let count = cx.read(|cx| app.read(cx).view().rows.len());
        git(
            fixture.path(),
            &["commit", "--allow-empty", "-q", "-m", "hidden arrival"],
        );
        app.update(cx, |app, cx| app.reload(cx));
        cx.run_until_parked();
        draw(cx, window);
        assert_eq!(cx.read(|cx| app.read(cx).view().rows.len()), count + 1);
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        cx.run_until_parked();
        draw(cx, window);
        assert!(
            e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none(),
            "rapid {mode} return must retire the WIP ghost before its first visible frame"
        );
        let returned = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
        assert!(
            (f32::from(returned.top() - original.top() + original.size.height)).abs() <= 1.,
            "{mode} return and hidden commit arrival use settled geometry"
        );
        let selected = cx
            .read(|cx| app.read(cx).ui().selected)
            .expect("logical selection retained");
        let selected_probe = format!("graph-row-{selected}");
        let returned_fill = pixel(cx, window, &selected_probe);
        let returned_old_fill = pixel(cx, window, "graph-row-0");
        motion::set_clock_for_e2e(Some(t + Duration::from_millis(400)));
        draw(cx, window);
        assert_eq!(
            pixel(cx, window, &selected_probe),
            returned_fill,
            "{mode} first visible wash already equals the settled endpoint"
        );
        assert_eq!(pixel(cx, window, "graph-row-0"), returned_old_fill);
        assert_eq!(
            e2e::control_bounds(window.window_id(), "graph-row-0").unwrap(),
            returned
        );
        // Returning must not permanently disable subsequent live WIP motion.
        std::fs::write(&file, "live after return\n").unwrap();
        app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
        cx.run_until_parked();
        draw(cx, window);
        motion::set_clock_for_e2e(Some(t + Duration::from_millis(437)));
        draw(cx, window);
        let live = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
        assert!(
            live.top() > returned.top() && live.top() < returned.top() + returned.size.height,
            "subsequent visible WIP insertion still animates after {mode}"
        );
        motion::set_clock_for_e2e(None);
        unmount(cx, app, window);
    }
    theme::set_reduce_motion(reduced);
}
