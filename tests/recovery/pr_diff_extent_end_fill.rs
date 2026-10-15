//! #1479-S5C: painted tail geometry, not estimated extent, owns end fill.
use super::*;

pub(super) fn shrink_at_end(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
    for split in [true, false] {
        theme::set_diff_split(split);
        theme::set_zoom(1.667);
        paint(cx, window);
        if split {
            // #1479-S5C: split diff owns the product End binding.
            let viewport = reading(cx, app).viewport;
            let position = gpui::point(
                viewport.left() + viewport.size.width * 0.75,
                viewport.center().y,
            );
            cx.simulate_click(window, position, gpui::Modifiers::none());
            paint(cx, window);
            cx.simulate_keystrokes(window, "end");
            paint(cx, window);
        } else {
            // Unified has no End binding; use its native saturating wheel path.
            wheel(cx, app, window, -20_000_000.);
        }
        assert_tail(cx, app, false, "167% End");
        // One layout invalidation, rather than intermediate menu zoom steps.
        theme::set_zoom(1.0);
        paint(cx, window);
        let tail = assert_tail(cx, app, false, "End then shrink to 100%");
        assert_smooth_up(cx, app, window, false, tail);
    }
}

fn geometry(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    conflict: bool,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        let tab = &mode.tabs[mode.active.unwrap()];
        let scroll = if conflict {
            &tab.conflict_scroll
        } else {
            &tab.diff_scroll
        };
        let tail = scroll
            .bounds_for_item(scroll.item_count() - 1)
            .expect("#1479-S5C actual final row must be laid out");
        (scroll.viewport_bounds(), tail)
    })
}

fn assert_tail(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    conflict: bool,
    reason: &str,
) -> Bounds<Pixels> {
    let (viewport, tail) = geometry(cx, app, conflict);
    assert!(
        (tail.bottom() - viewport.bottom()).abs() <= gpui::px(1.),
        "#1479-S5C {reason}: actual final row must bottom-align without blank space: tail={tail:?}, viewport={viewport:?}"
    );
    tail
}

fn assert_smooth_up(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    conflict: bool,
    mut before: Bounds<Pixels>,
) {
    for _ in 0..2 {
        let (viewport, _) = geometry(cx, app, conflict);
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(4.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        paint(cx, window);
        let (_, after) = geometry(cx, app, conflict);
        let movement = f32::from(after.top() - before.top());
        assert!(
            (movement - 4.).abs() <= 1.,
            "#1479-S5C first and subsequent upward wheels must move by 4px, not snap: movement={movement}, before={before:?}, after={after:?}"
        );
        before = after;
    }
}

pub fn scenario_pr_diff_extent_conflict_end_fill(cx: &mut VisualTestAppContext) {
    conflict_jump(cx, false);
    conflict_jump(cx, true);
}

fn conflict_jump(cx: &mut VisualTestAppContext, wrapped_successors: bool) {
    let _restore = Restore::capture();
    theme::set_diff_split(false);
    theme::set_zoom(1.0);
    let _gh = OfflineGh::with_script(GH);
    let fixture = fixture();
    let repo = &fixture.repo;
    let remote = fixture._remote.path().join("repo.git");
    let base: String = (1..=LINES)
        .map(|line| {
            let suffix = if wrapped_successors && line > 598 {
                " trailing wrapped context".repeat(120)
            } else {
                String::new()
            };
            format!("context_{line:04}{suffix}\n")
        })
        .collect();
    std::fs::write(repo.join("conflicts.txt"), &base).unwrap();
    git(repo, &["add", "conflicts.txt"]);
    git(repo, &["commit", "-q", "-m", "conflict preview ancestor"]);
    let ancestor = rev(repo, "HEAD");
    let side = |name: &str| {
        base.replace("context_0010", &format!("{name}_0010"))
            .replace("context_0598", &format!("{name}_0598"))
    };
    let head = push_pr_head(
        repo,
        &remote,
        &ancestor,
        &[("conflicts.txt", &side("incoming"))],
    );
    std::fs::write(repo.join("conflicts.txt"), side("current")).unwrap();
    git(repo, &["add", "conflicts.txt"]);
    git(repo, &["commit", "-q", "-m", "conflict preview base tip"]);
    git(
        repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let (app, window) = mount(cx, repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.pr_mode_open(&pr_at(&head), cx));
        wait_loaded(cx, &app, &head);
        app.update(cx, |app, cx| {
            app.pr_mode_show(kagi::ui::pr_mode::PrView::Conflicts, cx)
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            paint(cx, window);
            let loaded = cx.read(|cx| {
                let mode = app.read(cx).pr_mode().unwrap();
                let tab = &mode.tabs[mode.active.unwrap()];
                tab.conflicts.as_ref().is_some_and(|result| {
                    result.as_ref().is_ok_and(|files| {
                        files.len() == 1 && files[0].path == std::path::Path::new("conflicts.txt")
                    })
                }) && tab.conflict_scroll.item_count() > LINES
            });
            if loaded {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "#1479-S5C real Git conflict preview did not load"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        crate::native_ax::activate(cx);
        paint(cx, window);
        // Native Press executes render_jump_nav's actual pr_mode_jump_conflict callback.
        assert!(
            crate::native_ax::press(cx, window, "›"),
            "#1479-S5C native conflict Next must activate"
        );
        paint(cx, window);
        cx.read(|cx| {
            let mode = app.read(cx).pr_mode().unwrap();
            let tab = &mode.tabs[mode.active.unwrap()];
            assert_eq!(
                tab.conflict_at, 1,
                "native Next must select the near-last conflict"
            );
            assert!(tab.conflict_scroll.logical_scroll_top().item_ix > LINES / 2);
        });
        if wrapped_successors {
            cx.read(|cx| {
                let mode = app.read(cx).pr_mode().unwrap();
                let scroll = &mode.tabs[mode.active.unwrap()].conflict_scroll;
                // Each one-line conflict adds a header and a second side.
                // The preceding conflict therefore shifts source line 598 by 2.
                let target = 597 + 2;
                let header = scroll.bounds_for_item(target).expect(
                    "#1479-S5C wrapped successors must not turn a conflict jump into End",
                );
                let viewport = scroll.viewport_bounds();
                assert!(
                    (header.top() - viewport.top()).abs() <= gpui::px(1.)
                        && header.bottom() <= viewport.bottom(),
                    "requested conflict header must remain at the viewport top: {header:?}, {viewport:?}"
                );
                assert_eq!(scroll.logical_scroll_top().item_ix, target);
                for ix in target + 3..scroll.item_count() {
                    assert!(
                        scroll.measured_height(ix).expect("wrapped successor measured")
                            > scroll.estimated_height() * 2.,
                        "native successor must exceed its estimate, not just add ordinary rows"
                    );
                }
            });
        } else {
            let tail = assert_tail(cx, &app, true, "near-last native conflict jump");
            assert_smooth_up(cx, &app, window, true, tail);
        }
    }));
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire conflict end-fill root");
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
