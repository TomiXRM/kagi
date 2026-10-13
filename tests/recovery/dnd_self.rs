//! #1421: real drag feedback agrees with the merge destination identity.
use std::{cell::RefCell, rc::Rc};

use gpui::{
    point, px, size, AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};

use crate::macos::{build_fixture, git, open_offscreen, repo_fingerprint, unmount};
use crate::recovery_operations::{paint, wait_idle};

fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    width: f32,
    height: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = open_offscreen(cx, size(px(width), px(height)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    });
    cx.run_until_parked();
    let app = captured.borrow().clone().unwrap();
    (app, window.into())
}

fn fill(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> [u8; 4] {
    cx.update_window(window, |_, window, _| {
        let bounds = e2e::control_bounds(window.window_handle().window_id(), id).unwrap();
        let image = window.render_to_image().expect("native drag-target fill");
        let scale = window.scale_factor();
        image
            .get_pixel(
                (f32::from(bounds.center().x) * scale) as u32,
                ((f32::from(bounds.top()) + 2.) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

fn drag_over(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    source: &str,
    target: &str,
) -> gpui::Point<gpui::Pixels> {
    let source = e2e::control_bounds(window.window_id(), source)
        .unwrap()
        .center();
    let target = e2e::control_bounds(window.window_id(), target)
        .unwrap()
        .center();
    cx.simulate_mouse_move(window, source, None, Modifiers::none());
    cx.simulate_mouse_down(window, source, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        window,
        source + point(px(12.), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_move(window, target, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    paint(cx, window);
    target
}

fn check_feedback(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    target_id: &str,
    valid: bool,
    viewport: (f32, f32),
) {
    paint(cx, window);
    let neutral = fill(cx, window, target_id);
    let target = drag_over(cx, window, "sidebar-local-source", target_id);
    let hovered = fill(cx, window, target_id);
    if valid {
        assert_ne!(
            hovered, neutral,
            "distinct branch must have valid highlight"
        );
    } else {
        assert_eq!(
            hovered, neutral,
            "self/alias target must not show valid highlight: {target_id}"
        );
    }
    let cursor = if valid {
        gpui::CursorStyle::OpenHand
    } else {
        gpui::CursorStyle::OperationNotAllowed
    };
    cx.read(|cx| {
        assert_eq!(
            cx.active_drag_cursor_style(),
            Some(cursor),
            "drag cursor agrees with acceptance"
        )
    });
    if !valid {
        cx.simulate_mouse_move(
            window,
            point(px(viewport.0 - 5.), px(viewport.1 - 5.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.read(|cx| {
            assert_eq!(
                cx.active_drag_cursor_style(),
                Some(gpui::CursorStyle::OpenHand),
                "leaving invalid target restores cursor"
            )
        });
        cx.simulate_mouse_move(window, target, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        paint(cx, window);
    }
    cx.simulate_mouse_up(window, target, MouseButton::Left, Modifiers::none());
}

fn check_outcome(cx: &VisualTestAppContext, app: &Entity<KagiApp>, operand: &str, valid: bool) {
    cx.read(|cx| {
        let state = app.read(cx);
        if valid {
            let modal = state.merge_modal().expect("valid drop opens plan");
            assert_eq!(modal.target, "source");
            assert_eq!(modal.into_branch, operand);
        } else {
            assert!(
                state.merge_modal().is_none(),
                "invalid drop must not open plan"
            );
            assert!(
                state
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .is_empty(),
                "invalid drop is a no-op, not a late refusal toast"
            );
        }
    });
}

pub fn scenario_branch_drag_self_target(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let previous_lang = i18n::lang();
    let previous_zoom = theme::zoom();
    let previous_theme = theme::theme().slug.clone();
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "source"]);
    git(fixture.path(), &["branch", "target", "HEAD~1"]);
    let repo = git2::Repository::open(fixture.path()).unwrap();
    let tip = repo.revparse_single("HEAD~1").unwrap().id();
    repo.reference("refs/remotes/origin/source", tip, false, "fixture")
        .unwrap();
    drop(repo);
    let before = repo_fingerprint(fixture.path());
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for language in [Lang::En, Lang::Ja] {
            i18n::set_lang(language);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for (width, height) in [(940., 660.), (1392., 883.)] {
                    let (app, window) = mount_at(cx, fixture.path(), width, height);
                    for (target_id, operand, valid) in [
                        ("sidebar-local-source", "source", false),
                        ("graph-remote-origin/source", "origin/source", false),
                        ("sidebar-local-target", "target", true),
                    ] {
                        check_feedback(cx, window, target_id, valid, (width, height));
                        wait_idle(cx, &app);
                        check_outcome(cx, &app, operand, valid);
                        assert_eq!(repo_fingerprint(fixture.path()), before);
                    }
                    unmount(cx, app, window);
                }
            }
        }
    }
    theme::set_active(&previous_theme);
    theme::set_zoom(previous_zoom);
    i18n::set_lang(previous_lang);
    eprintln!("[gui-e2e] PASS branch_drag_self_target");
}
