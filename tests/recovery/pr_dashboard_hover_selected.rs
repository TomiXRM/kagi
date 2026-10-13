//! #1451: pointer hover must not erase a keyboard-selected dashboard PR.
use crate::macos::{build_fixture, git, unmount};
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, Pixels, Role, VisualTestAppContext,
};
use kagi::ui::{e2e, i18n, list_a11y, theme, KagiApp};
use std::{cell::RefCell, rc::Rc};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn press(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
}

fn tab_to(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, id: &str) {
    for _ in 0..120 {
        let focused = cx
            .update_window(win, |_, window, cx| {
                app.read(cx).dashboard_keyboard_focused_for_e2e(id, window)
            })
            .unwrap();
        if focused {
            return;
        }
        cx.simulate_keystrokes(win, "tab");
        draw(cx, win);
    }
    panic!("Tab never reaches production dashboard control {id}");
}

fn assert_identity(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        assert_eq!(
            app.read(cx)
                .dashboard_row_focused_for_e2e(window)
                .as_deref(),
            Some("2")
        );
    })
    .unwrap();
    let ax = list_a11y::recorded_list("pr-list").unwrap();
    assert_eq!(ax.role, Some(Role::ListBox));
    assert_eq!(ax.size, 3);
    let selected: Vec<_> = ax.rows.values().filter(|(_, selected)| *selected).collect();
    assert_eq!(selected.len(), 1, "exactly one selected AX option");
    assert!(
        selected[0].0.contains("Hover identity 2"),
        "AX selected PR remains N"
    );
}

fn row_bounds(win: AnyWindowHandle, number: u64) -> Bounds<Pixels> {
    e2e::control_bounds(win.window_id(), &format!("pr-home-row-{number}"))
        .expect("production dashboard row painted")
}

fn move_to(cx: &mut VisualTestAppContext, win: AnyWindowHandle, number: Option<u64>) {
    let target = number.map_or(point(px(1.), px(1.)), |number| {
        let rect = row_bounds(win, number);
        // The left gutter is visible even when the table scrolls horizontally;
        // its centre is clear of the avatar and keyboard-only focus border.
        point(rect.left() + theme::scaled_px(12.), rect.center().y)
    });
    cx.simulate_mouse_move(win, target, None, Modifiers::none());
}

type RowPaint = ([u8; 4], Bounds<Pixels>);

fn paint(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> [RowPaint; 2] {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let viewport = e2e::control_bounds(win.window_id(), "pr-home-table-viewport").unwrap();
        let image = window
            .render_to_image()
            .expect("native selected PR Metal fill");
        let scale = window.scale_factor();
        // One native capture samples both real rows, including the selected
        // row while the pointer rests over the unselected row.
        [1, 2].map(|number| {
            let rect = row_bounds(win, number);
            let x = rect.left() + theme::scaled_px(12.);
            let y = rect.top() + theme::scaled_px(12.);
            assert!(x >= viewport.left() && x < viewport.right());
            assert!(y >= viewport.top() && y < viewport.bottom());
            (
                image
                    .get_pixel((f32::from(x) * scale) as u32, (f32::from(y) * scale) as u32)
                    .0,
                rect,
            )
        })
    })
    .unwrap()
}

fn assert_fill(pixel: [u8; 4], expected: gpui::Hsla, state: &str) {
    let rgba = expected.to_rgb();
    for (actual, expected) in pixel[..3].iter().zip([rgba.r, rgba.g, rgba.b]) {
        assert!(
            (f32::from(*actual) - expected * 255.).abs() <= 1.,
            "{state}: painted {pixel:?} differs from resolved token {rgba:?}"
        );
    }
    assert_eq!(pixel[3], 255, "opaque native row fill");
}

fn select(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, first: bool) {
    tab_to(cx, app, win, "pr-home-list");
    if !first {
        cx.simulate_keystrokes(win, "home");
    }
    cx.simulate_keystrokes(win, "down");
    draw(cx, win);
    // High zoom can reveal N at the viewport top, virtualizing N-1. Scroll
    // without changing focus so the hover oracle samples both live rows.
    let selected = row_bounds(win, 2);
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: point(selected.left() + theme::scaled_px(12.), selected.center().y),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(1000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    draw(cx, win);
    assert!(
        row_bounds(win, 1).bottom() <= row_bounds(win, 2).top() + px(1.),
        "the unselected neighbor and selected PR must have distinct live row bounds"
    );
    assert_identity(cx, app, win);
}

fn selection(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    first: bool,
) {
    select(cx, app, win, first);
    let mut active_fill = None;
    for active in [true, false] {
        app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
        move_to(cx, win, None);
        let rest = paint(cx, win)[1];
        assert_fill(
            rest.0,
            theme::theme().selected_for_window(active),
            "selected rest",
        );
        assert_identity(cx, app, win);
        move_to(cx, win, Some(2));
        let hovered = paint(cx, win)[1];
        assert_fill(
            hovered.0,
            theme::theme().selected_for_window(active),
            "selected hover",
        );
        assert_eq!(
            hovered, rest,
            "same-row hover preserves native fill and bounds"
        );
        assert_identity(cx, app, win);
        if active {
            active_fill = Some(hovered);
        } else if theme::theme().selected_for_window(false)
            != theme::theme().selected_for_window(true)
        {
            assert_ne!(
                Some(hovered),
                active_fill,
                "distinct selection tokens produce distinct fills"
            );
        } else {
            assert_eq!(
                Some(hovered),
                active_fill,
                "equal neutral selection tokens retain the same fill"
            );
        }
        move_to(cx, win, Some(1));
        let other_hover = paint(cx, win);
        assert_fill(
            other_hover[0].0,
            gpui::rgb(theme::theme().surface).into(),
            "unselected hover",
        );
        assert_eq!(
            other_hover[1], rest,
            "other-row hover does not move selection"
        );
        assert_identity(cx, app, win);
    }
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    move_to(cx, win, Some(2));
    let reactivated = paint(cx, win)[1];
    assert_fill(
        reactivated.0,
        theme::theme().selected_for_window(true),
        "reactivated hover",
    );
    assert_eq!(
        Some(reactivated),
        active_fill,
        "reactivation restores selected fill and bounds"
    );
    assert_identity(cx, app, win);
}

fn activate(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, key: &str) {
    let before = e2e::pr_detail::activation_count("pr-home-row-2");
    press(cx, win, key);
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(
        e2e::pr_detail::activation_count("pr-home-row-2"),
        before + 1,
        "hovered {key} opens N exactly once, including key-up"
    );
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        assert_eq!(mode.tabs.len(), 1, "no other PR or duplicate tab opens");
        assert_eq!(mode.tabs[mode.active.unwrap()].pr.number, 2);
    });
    tab_to(cx, app, win, "pr-mode-home");
    press(cx, win, "enter");
    draw(cx, win);
    cx.read(|cx| assert!(app.read(cx).pr_mode().unwrap().active.is_none()));
}

fn queue_rows(head: &str) {
    let rows = (1..=3)
        .map(|number| {
            let mut pr = crate::evidence_support::pull_request(
                number,
                &format!("Hover identity {number}"),
                "feature/hover",
            );
            pr.head_sha = head.to_owned();
            pr
        })
        .collect();
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
        rows,
        "example/repo",
        None,
    ))));
}

fn matrix_window(cx: &mut VisualTestAppContext, dimensions: (f32, f32)) {
    let fixture = build_fixture();
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let remote_dir = tempfile::tempdir().unwrap();
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    let remote = remote_dir.path().join("repo.git");
    git(
        fixture.path(),
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "HEAD:refs/heads/main",
            "HEAD:refs/pull/2/head",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(fixture.path(), &["remote", "add", "origin", url]);
    git(
        fixture.path(),
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    let head = crate::pr_viewed::rev(fixture.path(), "HEAD");
    let state = e2e::app_state(fixture.path()).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win: AnyWindowHandle = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow_mut().take().unwrap();
    queue_rows(&head);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    let mut first = true;
    for slug in ["apple-light", "apple-dark"] {
        for lang in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", lang);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                app.update(cx, |app, cx| app.set_theme(slug, cx));
                draw(cx, win);
                // Paint the full selection matrix once per presentation;
                // Enter and Space exercise the same style, not different fills.
                selection(cx, &app, win, first);
                first = false;
                activate(cx, &app, win, "enter");
                select(cx, &app, win, false);
                move_to(cx, win, Some(2));
                draw(cx, win);
                assert_identity(cx, &app, win);
                activate(cx, &app, win, "space");
            }
        }
    }
    unmount(cx, app, win);
}

pub fn scenario_pr_dashboard_hover_selected(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    for dimensions in [(940., 660.), (1392., 883.)] {
        matrix_window(cx, dimensions);
    }
    eprintln!("[gui-e2e] PASS pr_dashboard_hover_selected");
}
