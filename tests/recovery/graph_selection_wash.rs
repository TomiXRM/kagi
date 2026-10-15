//! #1485: Graph selection preserves stripes/lane tints and normal ink.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Hsla, Modifiers, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::theme;

pub fn scenario_graph_selection_wash(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "accent_source",
        "accent",
        "graph_lane_compact",
    ]);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("README.md"),
        "Graph wash dirty fixture\n",
    )
    .unwrap();
    let (app, window) = mount(cx, fixture.path());
    theme::set_graph_lane_compact(true);
    for slug in ["apple-light", "apple-dark"] {
        let accent = if slug == "apple-light" {
            theme::SystemAccent::Blue
        } else {
            theme::SystemAccent::Yellow
        };
        let _accent = theme::AccentInjection::new(accent, Some(true));
        for zoom in [1000, 1670] {
            theme::set_zoom(zoom as f32 / 1000.);
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                if let Some(selected) = app.ui().selected {
                    app.select_headless(selected);
                }
                app.set_window_active_for_e2e(true, cx);
            });
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(1.), gpui::px(1.)),
                None,
                Modifiers::none(),
            );
            for ix in [0, 1] {
                draw(cx, window);
                let row_id = format!("graph-row-{ix}");
                let lane_id = format!("graph-lane-band-{ix}");
                let baseline = pixel(cx, window, &row_id);
                let lane_baseline = pixel(cx, window, &lane_id);
                let palette = theme::theme();
                let stripe = if ix == 0 {
                    palette.bg_base
                } else {
                    palette.bg_row_alt
                };
                assert_rgb(baseline, stripe, "ordinary stripe");
                app.update(cx, |app, cx| {
                    app.select_headless(ix);
                    cx.notify();
                });
                draw(cx, window);
                // Selecting opens the existing Inspector; compare activation
                // geometry only after that legitimate workspace reflow.
                let row = e2e::control_bounds(window.window_id(), &row_id).unwrap();
                let actual = pixel(cx, window, &row_id);
                assert_rgb(
                    actual,
                    composite(palette.ui_accent(), 0.30, baseline),
                    "Graph accent wash over stripe",
                );
                assert_ne!(
                    actual,
                    palette.ui_accent(),
                    "Graph selection is not opaque accent"
                );
                assert_rgb(
                    pixel(cx, window, &lane_id),
                    composite(palette.ui_accent(), 0.30, lane_baseline),
                    "accent wash over preserved lane tint",
                );
                assert!(
                    contrast(palette.text_main, actual) >= 4.5,
                    "Graph primary contrast {slug}/{zoom}/{ix}"
                );
                if ix == 0 {
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-commit-label")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(palette.text_main))
                    );
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-commit-secondary")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(palette.text_sub))
                    );
                    let lane = cx.read(|cx| app.read(cx).view().rows[0].node_color);
                    let (_, _, ink) = theme::badge_style(theme::lane_color_u32(lane));
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-selected-badge-label")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(ink))
                    );
                }
                app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
                draw(cx, window);
                assert_rgb(
                    pixel(cx, window, &row_id),
                    palette.inactive_selected,
                    "inactive Graph grey",
                );
                assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(ix));
                assert_eq!(
                    e2e::control_bounds(window.window_id(), &row_id).unwrap(),
                    row
                );
                app.update(cx, |app, cx| {
                    app.set_window_active_for_e2e(true, cx);
                    app.select_headless(ix);
                    cx.notify();
                });
            }
            draw(cx, window);
            let wip_base = pixel(cx, window, "graph-wip-row");
            let chip_ink = e2e::control_paint(window.window_id(), "graph-wip-chip")
                .unwrap()
                .text_color;
            let note_ink = e2e::control_paint(window.window_id(), "graph-wip-note")
                .unwrap()
                .text_color;
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.open_commit_panel(window, cx))
            })
            .unwrap();
            cx.run_until_parked();
            draw(cx, window);
            let palette = theme::theme();
            let selected_wip = pixel(cx, window, "graph-wip-row");
            assert_rgb(
                selected_wip,
                composite(palette.ui_accent(), 0.30, wip_base),
                "WIP wash over lane tint",
            );
            assert!(
                contrast(palette.text_main, selected_wip) >= 4.5,
                "WIP normal primary contrast {slug}/{zoom}"
            );
            assert_eq!(
                e2e::control_paint(window.window_id(), "graph-wip-chip")
                    .unwrap()
                    .text_color,
                chip_ink
            );
            assert_eq!(
                e2e::control_paint(window.window_id(), "graph-wip-note")
                    .unwrap()
                    .text_color,
                note_ink
            );
            app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
            draw(cx, window);
            assert_rgb(
                pixel(cx, window, "graph-wip-row"),
                palette.inactive_selected,
                "inactive WIP grey",
            );
            app.update(cx, |app, cx| {
                app.select_headless(0);
                app.set_window_active_for_e2e(true, cx);
                cx.notify();
            });
        }
    }
    unmount(cx, app, window);
}

pub fn scenario_graph_wip_hover_wash(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "accent_source", "accent"]);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "WIP hover changed\n").unwrap();
    let (app, window) = mount(cx, fixture.path());
    for slug in ["apple-light", "apple-dark"] {
        for accent in [theme::SystemAccent::Blue, theme::SystemAccent::Yellow] {
            let _accent = theme::AccentInjection::new(accent, Some(true));
            for zoom in [1000, 1670] {
                theme::set_zoom(zoom as f32 / 1000.);
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    app.select_headless(0);
                    app.set_window_active_for_e2e(true, cx);
                    cx.notify();
                });
                cx.simulate_mouse_move(
                    window,
                    gpui::point(gpui::px(1.), gpui::px(1.)),
                    None,
                    Modifiers::none(),
                );
                draw(cx, window);
                let base = pixel(cx, window, "graph-wip-row");
                let row = e2e::control_bounds(window.window_id(), "graph-wip-row").unwrap();
                let ink = e2e::control_paint(window.window_id(), "graph-wip-note")
                    .unwrap()
                    .text_color;
                cx.simulate_mouse_move(window, row.center(), None, Modifiers::none());
                draw(cx, window);
                let palette = theme::theme();
                assert_rgb(
                    pixel(cx, window, "graph-wip-row"),
                    composite(palette.ui_accent(), 0.15, base),
                    "WIP hover wash over lane tint",
                );
                assert_eq!(
                    cx.read(|cx| app.read(cx).ui().selected),
                    Some(0),
                    "hover must not select WIP"
                );
                assert_eq!(
                    e2e::control_paint(window.window_id(), "graph-wip-note")
                        .unwrap()
                        .text_color,
                    ink
                );
                cx.simulate_click(window, row.center(), Modifiers::none());
                cx.run_until_parked();
                draw(cx, window);
                assert_rgb(
                    pixel(cx, window, "graph-wip-row"),
                    composite(palette.ui_accent(), 0.30, base),
                    "clicked WIP retains selection wash under pointer",
                );
                assert_eq!(
                    e2e::control_paint(window.window_id(), "graph-wip-note")
                        .unwrap()
                        .text_color,
                    ink
                );
            }
        }
    }
    unmount(cx, app, window);
}

pub fn scenario_graph_selection_crossfade(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::motion;
    use std::time::{Duration, Instant};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion", "theme"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    crate::macos::git(
        fixture.path(),
        &["commit", "-q", "--allow-empty", "-m", "third selection"],
    );
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_theme("apple-light", cx);
        app.set_window_active_for_e2e(true, cx);
        app.select_headless(0);
        cx.notify();
    });
    draw(cx, window);
    let palette = theme::theme();
    let selected_a = pixel(cx, window, "graph-row-0");
    let baseline_b = pixel(cx, window, "graph-row-1");
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    crate::compact_select_changes::focus_graph(cx, &app, window);
    crate::compact_select_changes::key(cx, window, "down");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(1),
        "keyboard selection is immediate"
    );
    draw(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    draw(cx, window);
    let mid_a = pixel(cx, window, "graph-row-0");
    let mid_b = pixel(cx, window, "graph-row-1");
    assert_ne!(
        mid_a, palette.bg_base,
        "outgoing Graph selection must remain partially washed"
    );
    assert_ne!(mid_a, selected_a, "outgoing Graph selection must fade");
    assert_ne!(
        mid_b, baseline_b,
        "incoming Graph selection must be partially washed"
    );
    assert_ne!(mid_b, composite(palette.ui_accent(), 0.30, baseline_b));
    // Retarget A → B → C without dropping A's still-visible fade.
    app.update(cx, |app, cx| {
        app.select_headless(2);
        cx.notify();
    });
    draw(cx, window);
    assert_rgb(
        pixel(cx, window, "graph-row-0"),
        mid_a,
        "retarget preserves A",
    );
    assert_rgb(
        pixel(cx, window, "graph-row-1"),
        mid_b,
        "retarget preserves B",
    );
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(400)));
    draw(cx, window);
    assert_rgb(
        pixel(cx, window, "graph-row-0"),
        palette.bg_base,
        "settled outgoing stripe",
    );
    assert_rgb(
        pixel(cx, window, "graph-row-1"),
        baseline_b,
        "settled second stripe",
    );
    assert_rgb(
        pixel(cx, window, "graph-row-2"),
        composite(palette.ui_accent(), 0.30, palette.bg_base),
        "settled existing wash",
    );
    theme::set_reduce_motion(true);
    app.update(cx, |app, cx| {
        app.select_headless(0);
        cx.notify();
    });
    draw(cx, window);
    assert_rgb(
        pixel(cx, window, "graph-row-0"),
        selected_a,
        "Reduce Motion immediate",
    );
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}

pub fn scenario_graph_commit_insert_motion(cx: &mut VisualTestAppContext) {
    use crate::macos::git;
    use kagi_ui_core::motion;
    use std::time::{Duration, Instant};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    for n in 0..40 {
        git(
            fixture.path(),
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("scroll fixture {n}"),
            ],
        );
    }
    let (app, window) = mount(cx, fixture.path());
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    draw(cx, window);
    let original = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    let old_id = cx.read(|cx| app.read(cx).view().rows[0].id.clone());
    let viewport = e2e::control_bounds(window.window_id(), "commit-list-viewport").unwrap();
    let (tail_index, tail_bounds) = (0..42)
        .filter_map(|index| {
            let bounds = e2e::control_bounds(window.window_id(), &format!("graph-row-{index}"))?;
            (bounds.top() < viewport.bottom()).then_some((index, bounds))
        })
        .last()
        .unwrap();
    git(
        fixture.path(),
        &["commit", "-q", "--allow-empty", "-m", "motion arrival"],
    );
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), &format!("graph-row-{}", tail_index + 1));
    draw(cx, window);
    assert_eq!(cx.read(|cx| app.read(cx).view().rows[1].id.clone()), old_id);
    let start = e2e::control_bounds(window.window_id(), "graph-row-1").unwrap();
    assert!(
        (f32::from(start.top() - original.top())).abs() <= 1.,
        "arrival starts at old painted position"
    );
    let tail_after =
        e2e::control_bounds(window.window_id(), &format!("graph-row-{}", tail_index + 1))
            .expect("displaced viewport-tail row remains realized");
    assert!(
        (f32::from(tail_after.top() - tail_bounds.top())).abs() <= 1.,
        "tail {tail_index}: before={tail_bounds:?}, after={tail_after:?}, viewport={viewport:?}"
    );
    let tail_id = cx.read(|cx| app.read(cx).view().rows[tail_index + 1].id.clone());
    let tail_point = gpui::point(
        tail_after.center().x,
        tail_after.top() + gpui::px(3.).min((viewport.bottom() - tail_after.top()) / 2.),
    );
    cx.simulate_mouse_move(window, tail_point, None, Modifiers::none());
    cx.simulate_click(window, tail_point, Modifiers::none());
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.ui()
                .selected
                .map(|index| app.view().rows[index].id.clone())
        }),
        Some(tail_id),
        "displaced viewport-tail pixels must hit their real commit identity"
    );
    app.update(cx, |app, cx| {
        app.select_headless(tail_index + 1);
        cx.notify();
    });
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    draw(cx, window);
    let mid = e2e::control_bounds(window.window_id(), "graph-row-1").unwrap();
    assert!(
        mid.top() > original.top() && mid.top() < original.top() + original.size.height,
        "commit below arrival is between old and final geometry: {mid:?} / {original:?}"
    );
    let mid_node = graph_node_pixel(cx, window, "graph-row-0");
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(300)));
    draw(cx, window);
    let end = e2e::control_bounds(window.window_id(), "graph-row-1").unwrap();
    assert!((f32::from(end.top() - original.top() - original.size.height)).abs() <= 1.);
    assert_ne!(
        graph_node_pixel(cx, window, "graph-row-0"),
        mid_node,
        "inserted commit node fades in with its row"
    );
    // A later batch while scrolled preserves the actual logical reading anchor.
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_scroll_handle
            .scroll_to_item_strict(2, gpui::ScrollStrategy::Top)
    });
    draw(cx, window);
    let anchor = e2e::control_bounds(window.window_id(), "graph-row-2").unwrap();
    git(
        fixture.path(),
        &["commit", "-q", "--allow-empty", "-m", "anchored arrival"],
    );
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    draw(cx, window);
    let anchored = e2e::control_bounds(window.window_id(), "graph-row-3").unwrap();
    assert!(
        (f32::from(anchored.top() - anchor.top())).abs() <= 1.,
        "insertion above viewport preserves anchor"
    );
    theme::set_reduce_motion(true);
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}

pub fn scenario_graph_wip_insert_motion(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::motion;
    use std::time::{Duration, Instant};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    for n in 0..40 {
        crate::macos::git(
            fixture.path(),
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("WIP scroll {n}"),
            ],
        );
    }
    let (app, window) = mount(cx, fixture.path());
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    draw(cx, window);
    let original = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    let file = fixture.path().join("motion-untracked");
    std::fs::write(&file, "WIP motion\n").unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    draw(cx, window);
    let mid = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    assert!(
        mid.top() > original.top() && mid.top() < original.top() + original.size.height,
        "WIP insertion pushes commits partway"
    );
    let mid_wip = pixel(cx, window, "graph-wip-row");
    // Reverse a partially inserted WIP; its retained row must not jump.
    std::fs::remove_file(&file).unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    let reversed = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    assert!(
        (f32::from(reversed.top() - mid.top())).abs() <= 1.,
        "WIP reversal continuity"
    );
    assert!(
        e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_some(),
        "removed WIP retains presentation"
    );
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(130)));
    draw(cx, window);
    let closing = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    assert!(closing.top() < mid.top() && closing.top() > original.top());
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_scroll_handle
            .scroll_to_item_strict(1, gpui::ScrollStrategy::Top)
    });
    e2e::clear_control_bounds(window.window_id(), "graph-row-0");
    draw(cx, window);
    let top_edge = e2e::control_bounds(window.window_id(), "graph-row-0")
        .expect("removal-displaced row remains realized above the final-index viewport");
    let viewport = e2e::control_bounds(window.window_id(), "commit-list-viewport").unwrap();
    assert!(top_edge.top() < viewport.top() && top_edge.bottom() > viewport.top());
    let visible_center = gpui::point(
        top_edge.center().x,
        (top_edge.top().max(viewport.top()) + top_edge.bottom().min(viewport.bottom())) / 2.,
    );
    cx.simulate_mouse_move(window, visible_center, None, Modifiers::none());
    cx.simulate_click(window, visible_center, Modifiers::none());
    crate::compact_select_changes::settle(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "overflow hit tests the real commit index"
    );
    app.update(cx, |app, cx| {
        app.select_headless(0);
        cx.notify();
    });
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_scroll_handle
            .scroll_to_item_strict(0, gpui::ScrollStrategy::Top)
    });
    crate::compact_select_changes::settle(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(400)));
    e2e::clear_control_bounds(window.window_id(), "graph-wip-ghost");
    e2e::clear_control_bounds(window.window_id(), "graph-row-0");
    draw(cx, window);
    assert_eq!(
        e2e::control_bounds(window.window_id(), "graph-row-0").unwrap(),
        original
    );
    assert!(e2e::control_bounds(window.window_id(), "graph-wip-ghost").is_none());
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .commit_scroll_handle
            .0
            .borrow()
            .base_handle
            .offset()
            .y),
        gpui::px(0.),
        "the Reduce Motion leg starts at the top, not at a retained reading anchor"
    );
    // Reduce Motion also bypasses the insertion path, not just selection.
    theme::set_reduce_motion(true);
    std::fs::write(&file, "instant WIP\n").unwrap();
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    cx.run_until_parked();
    draw(cx, window);
    let instant = e2e::control_bounds(window.window_id(), "graph-row-0").unwrap();
    assert!(
        (f32::from(instant.top() - original.top() - original.size.height)).abs() <= 1.,
        "instant insertion: {instant:?}, original={original:?}, scroll={:?}",
        cx.read(|cx| app
            .read(cx)
            .ui()
            .commit_scroll_handle
            .0
            .borrow()
            .base_handle
            .offset())
    );
    assert_ne!(
        pixel(cx, window, "graph-wip-row"),
        mid_wip,
        "inserted WIP has real intermediate native opacity"
    );
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}

pub fn scenario_graph_target_crossfade(cx: &mut VisualTestAppContext) {
    use crate::macos::git;
    use kagi_ui_core::motion;
    use std::time::{Duration, Instant};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion", "theme"]);
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "stash target\n").unwrap();
    git(
        fixture.path(),
        &["stash", "push", "-q", "-m", "motion stash"],
    );
    std::fs::write(fixture.path().join("current-untracked"), "current\n").unwrap();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "motion-linked",
            linked.to_str().unwrap(),
        ],
    );
    std::fs::write(linked.join("linked-untracked"), "linked\n").unwrap();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_theme("apple-light", cx);
        app.set_window_active_for_e2e(true, cx);
    });
    draw(cx, window);
    let current_base = pixel(cx, window, "graph-wip-row-0");
    let linked_base = pixel(cx, window, "graph-wip-row-1");
    let stash_base = pixel(cx, window, "graph-stash-row-0");
    let t = Instant::now();
    motion::set_clock_for_e2e(Some(t));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(90)));
    draw(cx, window);
    let mid_current = pixel(cx, window, "graph-wip-row-0");
    assert_ne!(mid_current, current_base, "current WIP fades in");
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(300)));
    draw(cx, window);
    let current_selected = pixel(cx, window, "graph-wip-row-0");
    assert_ne!(mid_current, current_selected);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_commit_panel_for_worktree(
                linked.clone(),
                "motion-linked".into(),
                1,
                window,
                cx,
            )
        });
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window);
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(390)));
    draw(cx, window);
    let outgoing = pixel(cx, window, "graph-wip-row-0");
    let incoming = pixel(cx, window, "graph-wip-row-1");
    assert_ne!(outgoing, current_base);
    assert_ne!(outgoing, current_selected);
    assert_ne!(incoming, linked_base, "linked WIP target owns its wash");
    app.update(cx, |app, cx| app.open_stash_peek(0, cx));
    cx.run_until_parked();
    draw(cx, window);
    assert_rgb(
        pixel(cx, window, "graph-wip-row-1"),
        incoming,
        "WIP to stash retarget continuity",
    );
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(480)));
    draw(cx, window);
    let stash_mid = pixel(cx, window, "graph-stash-row-0");
    assert_ne!(stash_mid, stash_base, "stash OID selection fades in");
    motion::set_clock_for_e2e(Some(t + Duration::from_millis(700)));
    draw(cx, window);
    assert_rgb(
        pixel(cx, window, "graph-wip-row-0"),
        current_base,
        "current WIP settled",
    );
    assert_rgb(
        pixel(cx, window, "graph-wip-row-1"),
        linked_base,
        "linked WIP settled",
    );
    assert_ne!(pixel(cx, window, "graph-stash-row-0"), stash_mid);
    motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    unmount(cx, app, window);
}

fn graph_node_pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> u32 {
    kagi::ui::graph_view::take_paint_trace(window.window_id());
    draw(cx, window);
    let row = e2e::control_bounds(window.window_id(), id).unwrap();
    let (nodes, _) = kagi::ui::graph_view::take_paint_trace(window.window_id());
    let node = nodes
        .iter()
        .find(|node| (f32::from(node.center.y - row.center().y)).abs() <= 1.)
        .expect("graph canvas moves with its row");
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().unwrap();
        let scale = window.scale_factor();
        let pixel = image
            .get_pixel(
                (f32::from(node.center.x) * scale) as u32,
                (f32::from(node.center.y) * scale) as u32,
            )
            .0;
        ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32
    })
    .unwrap()
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

pub(crate) fn pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> u32 {
    let row = e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"));
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native Graph wash capture");
        let scale = window.scale_factor();
        let (x, y) = if id.starts_with("graph-lane-band-") {
            // Avoid graph nodes/connector ink, including compressed columns.
            (
                f32::from(row.left()) + 5.,
                f32::from(row.top()) + f32::from(row.size.height) * 0.25,
            )
        } else {
            (f32::from(row.right()) - 4., f32::from(row.center().y))
        };
        let p = image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
    })
    .unwrap()
}

fn assert_rgb(actual: u32, expected: u32, context: &str) {
    assert!(
        [0, 8, 16]
            .into_iter()
            .all(|shift| ((actual >> shift) & 255).abs_diff((expected >> shift) & 255) <= 1),
        "{context}: #{actual:06x} != #{expected:06x}"
    );
}

fn composite(fg: u32, alpha: f32, bg: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |rgb, shift| {
        let f = ((fg >> shift) & 255) as f32;
        let b = ((bg >> shift) & 255) as f32;
        rgb | (((f * alpha + b * (1. - alpha)).round() as u32) << shift)
    })
}

fn contrast(a: u32, b: u32) -> f64 {
    let luminance = |c: u32| {
        let channel = |shift| {
            let v = ((c >> shift) & 255u32) as f64 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
