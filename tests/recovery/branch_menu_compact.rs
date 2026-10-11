//! #1238: branch menus stay compact at the reference window size.
use std::cell::RefCell;
use std::rc::Rc;

use gpui::{point, px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, git, open_offscreen, unmount};
use crate::recovery_layout::{draw, GlobalSettings};

fn head(repo: &std::path::Path) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("read fixture HEAD");
    assert!(output.status.success());
    output.stdout
}
pub fn scenario_branch_menu_compact(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    for has_upstream in [false, true] {
        if has_upstream {
            git(
                fixture.path(),
                &["branch", "--set-upstream-to=feature", "main"],
            );
            git(
                fixture.path(),
                &["branch", "--set-upstream-to=main", "feature"],
            );
        }
        for locale in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", locale);
            i18n::init_lang();
            theme::set_zoom(1.);
            let state = e2e::app_state(fixture.path()).expect("fixture app state");
            let captured: Rc<RefCell<Option<gpui::Entity<KagiApp>>>> = Rc::default();
            let output = captured.clone();
            let window: AnyWindowHandle =
                open_offscreen(cx, size(px(1392.), px(883.)), move |window, cx| {
                    e2e::mount_root(state, window, cx, &output)
                })
                .into();
            let app = captured.borrow().clone().expect("captured KagiApp");
            cx.run_until_parked();
            for branch in ["main", "feature"] {
                let controls: Vec<_> = (0..7)
                    .flat_map(|group| {
                        (0..12).map(move |item| format!("branch-menu-item-{group}-{item}"))
                    })
                    .collect();
                for control in &controls {
                    e2e::clear_control_bounds(window.window_id(), control);
                }
                app.update(cx, |app, cx| {
                    app.open_local_branch_menu(branch.into(), point(px(400.), px(40.)));
                    cx.notify();
                });
                draw(cx, window, (1392., 883.));
                assert_geometry(window, &controls, locale, branch);
                assert_submenu_keys(cx, &app, window, locale, branch);
                assert_submenu_hover(cx, &app, window);
                if branch == "main" {
                    assert_disabled_checkout(cx, &app, window, fixture.path());
                }
                crate::keyboard_nav::keys(cx, window, "escape");
                assert!(cx.read(|cx| app.read(cx).branch_menu.is_none()));
            }
            unmount(cx, app, window);
        }
    }
}

fn assert_geometry(window: AnyWindowHandle, controls: &[String], locale: &str, branch: &str) {
    let bounds = e2e::control_bounds(window.window_id(), "branch-context-menu")
        .expect("actual menu bounds recorded");
    assert!(
        bounds.size.height > px(0.),
        "menu measurement must be nonempty"
    );
    let rows = controls
        .iter()
        .filter(|id| {
            e2e::control_bounds(window.window_id(), id).is_some_and(|row| {
                row.size.height > px(0.)
                    && row.top() >= bounds.top()
                    && row.bottom() <= bounds.bottom()
            })
        })
        .count();
    assert!(bounds.size.height <= px(883. * 0.45),
        "{locale}/{branch}: measured top-level menu height {:?} exceeds 45% of 883px (397.35px); visible rows={rows}", bounds.size.height);
    assert!(
        rows <= 10,
        "{locale}/{branch}: {rows} visible top-level rows exceeds 10"
    );
    assert!(rows > 0, "the menu must render visible rows");
    eprintln!(
        "[gui-e2e] branch_menu_compact: {locale}/{branch} height={:?} rows={rows}",
        bounds.size.height
    );
}

fn assert_submenu_keys(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    locale: &str,
    branch: &str,
) {
    assert_eq!(cx.read(|cx| app.read(cx).submenu_depth_for_e2e()), 0);
    for (open, close) in [("right", "left"), ("enter", "escape")] {
        crate::context_menu_keys::focus_menu_control(cx, app, window, "branch-menu-item-3-0");
        crate::keyboard_nav::keys(cx, window, open);
        assert_eq!(
            cx.read(|cx| app.read(cx).submenu_depth_for_e2e()),
            1,
            "{locale}/{branch}: {open} opens Integrate"
        );
        assert!(
            e2e::control_bounds(window.window_id(), "branch-menu-item-3-0-submenu-0-0").is_some()
        );
        crate::keyboard_nav::keys(cx, window, close);
        assert_eq!(
            cx.read(|cx| app.read(cx).submenu_depth_for_e2e()),
            0,
            "{locale}/{branch}: {close} closes one level"
        );
        assert!(
            cx.read(|cx| app.read(cx).branch_menu.is_some()),
            "closing a submenu retains its parent menu"
        );
        let focused = cx
            .update_window(window, |_, window, cx| {
                app.read(cx).menu_keys_for_e2e(window).0
            })
            .unwrap()
            .and_then(kagi::ui::menu_overlay::recorded_slot_control);
        assert_eq!(
            focused.as_deref(),
            Some("branch-menu-item-3-0"),
            "closing restores the parent header focus"
        );
    }
}

fn assert_disabled_checkout(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    let before = head(repo);
    crate::context_menu_keys::focus_menu_control(cx, app, window, "branch-menu-item-0-0");
    let reason = kagi::ui::menu_overlay::recorded_item_description("branch-menu-item-0-0")
        .flatten()
        .expect("current Checkout has a disabled reason");
    assert!(!reason.is_empty());
    for key in ["enter", "space"] {
        crate::keyboard_nav::keys(cx, window, key);
        assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
        assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    }
    assert_eq!(head(repo), before, "disabled activation cannot move HEAD");
    let row = e2e::control_bounds(window.window_id(), "branch-menu-item-0-0").unwrap();
    cx.simulate_mouse_move(window, row.center(), None, gpui::Modifiers::none());
    draw(cx, window, (1392., 883.));
    cx.advance_clock(std::time::Duration::from_millis(750));
    cx.run_until_parked();
    draw(cx, window, (1392., 883.));
    assert_eq!(
        kagi::ui::menu_overlay::recorded_item_tooltip("branch-menu-item-0-0"),
        Some(reason),
        "the actual tooltip text equals the disabled AX reason"
    );
}

fn hover(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    elapsed_ms: u64,
) {
    cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
    draw(cx, window, (1392., 883.));
    cx.advance_clock(std::time::Duration::from_millis(elapsed_ms));
    cx.run_until_parked();
    draw(cx, window, (1392., 883.));
}

fn active_slots(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Option<usize>, Vec<usize>) {
    cx.update_window(window, |_, window, cx| {
        app.read(cx).menu_keys_for_e2e(window)
    })
    .unwrap()
}

fn assert_submenu_hover(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let launcher = e2e::control_bounds(window.window_id(), "branch-menu-item-3-0").unwrap();
    for crossed in [
        "branch-menu-item-3-1",
        "branch-menu-item-3-2",
        "branch-menu-item-2-2",
    ] {
        crate::context_menu_keys::focus_menu_control(cx, app, window, "branch-menu-item-3-0");
        crate::keyboard_nav::keys(cx, window, "right");
        let child =
            e2e::control_bounds(window.window_id(), "branch-menu-item-3-0-submenu-0-1").unwrap();
        hover(cx, window, launcher.center(), 0);
        let row = e2e::control_bounds(window.window_id(), crossed).unwrap();
        hover(cx, window, point(row.right() - px(2.), row.center().y), 100);
        assert_eq!(
            cx.read(|cx| app.read(cx).submenu_depth_for_e2e()),
            1,
            "crossing a parent leaf must not immediately close a child"
        );
        assert_eq!(
            active_slots(cx, app, window).1.len(),
            3,
            "crossing an adjacent launcher must not immediately replace Integrate"
        );
        hover(cx, window, child.center(), 300);
        assert_eq!(
            active_slots(cx, app, window).1.len(),
            3,
            "entering the child panel cancels the pending parent hover"
        );
        crate::keyboard_nav::keys(cx, window, "left");
    }
    assert_hover_focus_return(cx, app, window);
}

fn assert_hover_focus_return(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    crate::context_menu_keys::focus_menu_control(cx, app, window, "branch-menu-item-3-0");
    crate::keyboard_nav::keys(cx, window, "right");
    assert!(
        active_slots(cx, app, window).0.is_some(),
        "child has keyboard focus"
    );
    let leaf = e2e::control_bounds(window.window_id(), "branch-menu-item-2-0").unwrap();
    hover(cx, window, leaf.center(), 300);
    assert_eq!(cx.read(|cx| app.read(cx).submenu_depth_for_e2e()), 0);
    let parent = active_slots(cx, app, window)
        .0
        .expect("focus returns to parent launcher");
    assert_eq!(
        kagi::ui::menu_overlay::recorded_slot_control(parent).as_deref(),
        Some("branch-menu-item-3-0")
    );
    crate::keyboard_nav::keys(cx, window, "down");
    assert_eq!(
        active_slots(cx, app, window).0,
        Some(parent + 1),
        "Down remains scoped to the menu after a hovered child closes"
    );
    crate::keyboard_nav::keys(cx, window, "up");
    assert_eq!(active_slots(cx, app, window).0, Some(parent));
    crate::keyboard_nav::keys(cx, window, "right");
    let adjacent = e2e::control_bounds(window.window_id(), "branch-menu-item-3-1").unwrap();
    hover(cx, window, adjacent.center(), 300);
    assert_eq!(
        active_slots(cx, app, window).1.len(),
        2,
        "hover can replace Integrate with Worktree"
    );
    crate::keyboard_nav::keys(cx, window, "down");
    assert_eq!(cx.read(|cx| app.read(cx).submenu_depth_for_e2e()), 0);
    assert_eq!(
        active_slots(cx, app, window).0,
        Some(parent + 1),
        "replacement also returns keyboard focus to the original launcher"
    );
    let launcher = e2e::control_bounds(window.window_id(), "branch-menu-item-3-0").unwrap();
    hover(cx, window, launcher.center(), 100);
    assert_eq!(
        cx.read(|cx| app.read(cx).submenu_depth_for_e2e()),
        0,
        "pointer-only submenu opening also waits for the hover delay"
    );
    hover(cx, window, launcher.center(), 150);
    assert_eq!(cx.read(|cx| app.read(cx).submenu_depth_for_e2e()), 1);
    crate::keyboard_nav::keys(cx, window, "right");
    crate::keyboard_nav::keys(cx, window, "left");
}
