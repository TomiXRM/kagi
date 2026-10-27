//! Full diff identity and stable toast Status metadata (#1336/#1337).
use crate::diff_highlight::commit_image_index;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use gpui::{point, px, size, AnyWindowHandle, Bounds, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, toast_stack, ToastKind};
use kagi_ui_core::{i18n, theme};
use std::time::Duration;

fn paint(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, win, dimensions);
    }
}

pub fn scenario_diff_header_identity(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "diff_split"]);
    let old_theme = theme::theme().slug.to_string();
    let old_split = theme::diff_split();
    theme::set_diff_split(false);
    let fixture = build_fixture();
    let paths = [
        format!("src/{}same.rs", "long-directory/".repeat(14)),
        format!("src/{}same.rs", "長いディレクトリ/".repeat(22)),
        "src/control\u{1b}/same.rs".to_owned(),
    ];
    for path in &paths {
        let file = fixture.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "fn example() {}\n".repeat(30)).unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(
        fixture.path(),
        &["commit", "-qm", "deep identical basenames"],
    );
    let (app, win) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        // User-resizable side panes use their supported compact widths. The
        // header-fit contract starts with a nonzero center slot, not two wide
        // side panes consuming the whole 940px/high-zoom viewport.
        app.sidebar.width = 160.;
        app.panel_width = 240.;
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    let mut compact_seen = false;
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, win, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                i18n::set_lang(lang);
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    for path in &paths {
                        check_header(cx, &app, win, dimensions, path, &mut compact_seen);
                    }
                }
            }
        }
    }
    assert!(compact_seen, "matrix exercises icon-only fitting");
    theme::set_active(&old_theme);
    theme::set_diff_split(old_split);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS diff_header_identity");
}

fn check_header(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
    path: &str,
    compact_seen: &mut bool,
) {
    e2e::diff_presentation::clear();
    let index = commit_image_index(cx, app, path);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    paint(cx, win, dimensions);
    let pane = cx.read(|cx| app.read(cx).ui().main_diff.clone()).unwrap();
    let fit = cx.read(|cx| pane.read(cx).fit.clone());
    *compact_seen |= fit.compact();
    let expected = kagi_domain::text_safety::sanitize_control_bytes(path);
    let identity = e2e::diff_presentation::header().expect("header rendered");
    assert_eq!(
        identity.displayed, expected,
        "full path is not replaced by basename"
    );
    assert_eq!(
        identity.role,
        Some(gpui::Role::Label),
        "diff title has AX identity"
    );
    assert_eq!(
        identity.description.as_deref(),
        Some(expected.as_str()),
        "full sanitized AX description"
    );
    check_header_geometry(&fit, win, dimensions);
    check_header_hover(cx, &pane, &fit, win, dimensions, &expected);
    drop(pane);
}

fn check_header_geometry(
    fit: &kagi_ui_core::header_fit::HeaderFit,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    let row = fit.row_bounds().unwrap();
    contained(
        Bounds::new(
            point(px(0.), px(0.)),
            size(px(dimensions.0), px(dimensions.1)),
        ),
        row,
        "diff header",
    );
    for id in [
        "main-diff-back",
        "main-diff-ext-editor",
        "main-diff-history",
        "diff-mode-unified",
        "diff-mode-split",
        "main-diff-stats",
    ] {
        contained(row, fit.control_bounds(id).unwrap(), id);
    }
    contained(
        row,
        e2e::control_bounds(win.window_id(), "main-diff-title").unwrap(),
        "title",
    );
}

fn check_header_hover(
    cx: &mut VisualTestAppContext,
    pane: &gpui::Entity<kagi::ui::main_diff_pane::MainDiffPane>,
    fit: &kagi_ui_core::header_fit::HeaderFit,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
    expected: &str,
) {
    let title = e2e::control_bounds(win.window_id(), "main-diff-title").unwrap();
    let before = fit.control_bounds("diff-mode-split").unwrap();
    let viewport_before = cx.read(|cx| pane.read(cx).diff_row_bounds_for_e2e(0));
    cx.simulate_mouse_move(win, title.center(), None, Modifiers::none());
    cx.advance_clock(Duration::from_secs(1));
    paint(cx, win, dimensions);
    assert_eq!(
        e2e::diff_presentation::take_header_tooltip().as_deref(),
        Some(expected),
        "invoked tooltip retains exact sanitized identity"
    );
    assert_eq!(
        before,
        fit.control_bounds("diff-mode-split").unwrap(),
        "tooltip does not resize header actions"
    );
    assert_eq!(
        viewport_before,
        cx.read(|cx| pane.read(cx).diff_row_bounds_for_e2e(0)),
        "hunk viewport/bounds unchanged by tooltip"
    );
    cx.simulate_mouse_move(win, point(px(1.), px(1.)), None, Modifiers::none());
}

pub fn scenario_toast_live_status(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "reduce_motion"]);
    let old_theme = theme::theme().slug.to_string();
    let old_motion = theme::reduce_motion();
    let fixture = build_fixture();
    let (app, win) = mount(cx, fixture.path());
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone()).unwrap();
    let root = cx.read(|cx| app.read(cx).root_focus.clone()).unwrap();
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, win, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [i18n::Lang::En, i18n::Lang::Ja] {
                i18n::set_lang(lang);
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    check_arrivals(cx, &stack, win, dimensions, &root, lang);
                }
            }
        }
    }
    theme::set_active(&old_theme);
    theme::set_reduce_motion(old_motion);
    let _ = toast_stack::take_close_focus_for_e2e();
    drop(stack);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS toast_live_status: stable newest-arrival Status metadata; GPUI has no live/priority setter");
}

fn check_arrivals(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<toast_stack::ToastStack>,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
    root: &gpui::FocusHandle,
    lang: i18n::Lang,
) {
    stack.update(cx, |stack, cx| {
        let ids = stack.toasts().iter().map(|t| t.id).collect::<Vec<_>>();
        for id in ids {
            stack.remove(id);
        }
        cx.notify();
    });
    cx.update_window(win, |_, window, cx| root.focus(window, cx))
        .unwrap();
    let mut previous_id = None;
    for kind in [
        ToastKind::Info,
        ToastKind::Success,
        ToastKind::Error,
        ToastKind::Sync,
    ] {
        for sequence in 0..6 {
            let text = match lang {
                i18n::Lang::En => format!("Result {kind:?} {sequence}: complete"),
                i18n::Lang::Ja => format!("結果 {kind:?} {sequence}：完了"),
            };
            stack.update(cx, |stack, cx| stack.push_notify(kind, text.clone(), cx));
            paint(cx, win, dimensions);
            let status = toast_stack::status_for_e2e(
                win.window_id(),
                toast_stack::ToastAudience::Repository,
            )
            .unwrap();
            assert_eq!(
                status.role,
                Some(gpui::Role::Status),
                "toast arrivals expose Status role"
            );
            assert_eq!(status.label.as_deref(), Some(text.as_str()));
            assert_ne!(
                status.newest_id, previous_id,
                "each arrival has an independent identity"
            );
            previous_id = status.newest_id;
            check_stable_arrival(cx, stack, win, dimensions, root, status);
        }
    }
    cx.simulate_keystrokes(win, "escape");
    paint(cx, win, dimensions);
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .last()
                .is_none_or(|t| Some(t.id) != previous_id || t.dismissing.is_some()),
            "Escape dismisses newest"
        )
    });
}

fn check_stable_arrival(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<toast_stack::ToastStack>,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
    root: &gpui::FocusHandle,
    status: toast_stack::ToastStatusPresentation,
) {
    let id = status.newest_id.unwrap();
    let close = e2e::control_bounds(win.window_id(), &format!("toast-dismiss-{id}")).unwrap();
    contained(
        Bounds::new(
            point(px(0.), px(0.)),
            size(px(dimensions.0), px(dimensions.1)),
        ),
        close,
        "toast close",
    );
    cx.simulate_mouse_move(win, close.center(), None, Modifiers::none());
    cx.advance_clock(Duration::from_millis(300));
    paint(cx, win, dimensions);
    assert_eq!(
        toast_stack::status_for_e2e(win.window_id(), toast_stack::ToastAudience::Repository)
            .unwrap(),
        status,
        "hover/tick/animation do not change arrival metadata"
    );
    cx.update_window(win, |_, window, _| {
        assert!(root.is_focused(window), "arrival never steals focus")
    })
    .unwrap();
    cx.read(|cx| {
        let toasts = stack.read(cx).toasts();
        assert!(toasts.len() <= 4);
        assert!(toasts.windows(2).all(|w| w[0].id < w[1].id));
        assert_eq!(toasts.last().unwrap().id, id);
    });
}
