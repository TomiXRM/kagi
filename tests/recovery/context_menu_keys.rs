//! #985 — the context-menu key and the keyboard inside a context menu.
//!
//! Shift+F10 on the window opens the selected commit's menu at its row's
//! bottom-left, with the focus on the first visible item; ↑/↓/Home/End move
//! among visible items, including disabled ones (the last item, Reset, is
//! always disabled and inert); Enter presses an enabled focused item; Escape closes the menu, and
//! either way the focus goes back to the window it came from. On a focused
//! sidebar row the key opens that row's menu below it, and Escape gives the
//! focus back to the row.
use std::cell::RefCell;
use std::rc::Rc;

use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, open_offscreen, unmount};

/// The open menu's focused item and enabled items (slots), as of a fresh
/// frame.
fn menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Option<usize>, Vec<usize>) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx).menu_keys_for_e2e(window)
    })
    .unwrap()
}

/// Reach a rendered command/header through the actual active menu keyboard.
pub(crate) fn focus_menu_control(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    control: &str,
) {
    let (_, visible) = menu(cx, app, window);
    for _ in 0..visible.len() {
        let focused = menu(cx, app, window)
            .0
            .and_then(kagi::ui::menu_overlay::recorded_slot_control);
        if focused.as_deref() == Some(control) {
            return;
        }
        keys(cx, window, "down");
    }
    panic!("{control}: not keyboard reachable among {visible:?}");
}

fn root_focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx)
            .root_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
    })
    .unwrap()
}

/// Shift+F10 from the window: the selected commit's menu opens below its
/// row, its first enabled item focused.
fn open_from_key(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let anchor = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).context_anchor_for_e2e()
        })
        .unwrap()
        .expect("the selected row records where it is drawn");
    keys(cx, window, "shift-f10");
    let state = cx.read(|cx| app.read(cx).commit_menu.clone());
    let state = state.expect("Shift+F10 opens the selected commit's menu");
    assert_eq!(state.row_index, 0);
    assert_eq!(
        state.position, anchor,
        "the menu opens at the row's bottom-left"
    );
    let (focused, enabled) = menu(cx, app, window);
    assert_eq!(
        focused,
        enabled.first().copied(),
        "the first enabled item has the focus"
    );
}

pub fn scenario_context_menu_keys(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.select(0);
            cx.notify();
        });
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();

    open_from_key(cx, &app, window);
    let window_id = window.window_id();
    // Disabled Reset remains keyboard-discoverable and Enter is inert.
    assert!(e2e::control_bounds(window_id, "commit-menu-item-5-0").is_some());
    let (_, enabled) = menu(cx, &app, window);
    let last = *enabled.last().expect("visible items");
    keys(cx, window, "end");
    assert_eq!(menu(cx, &app, window).0, Some(last));
    assert_eq!(
        kagi::ui::menu_overlay::recorded_slot_control(last).as_deref(),
        Some("commit-menu-item-5-0"),
        "End focuses disabled Reset"
    );
    keys(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
    assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));
    // ↓ wraps from disabled Reset to the first visible item.
    keys(cx, window, "down");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[0]));
    keys(cx, window, "up");
    assert_eq!(menu(cx, &app, window).0, Some(last));
    keys(cx, window, "home");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[0]));

    // Enter presses the focused item: ↓ to "Copy SHA", a read-only item.
    keys(cx, window, "down");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[1]));
    keys(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.path())
        .output()
        .expect("git rev-parse");
    let head = String::from_utf8(head.stdout).unwrap();
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(head.trim().to_string()),
        "Enter on Copy SHA copied the commit's SHA"
    );
    assert!(
        root_focused(cx, &app, window),
        "the focus is back on the window"
    );

    // Escape closes the menu; the focus goes back where it came from.
    open_from_key(cx, &app, window);
    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
    assert!(
        root_focused(cx, &app, window),
        "Escape gives the focus back"
    );
    unmount(cx, app, window);
}

/// The sidebar's LOCAL pane.
const LOCAL: usize = 0;

/// Shift+F10 on a focused sidebar branch row opens that branch's menu at
/// the row's bottom-left, its first enabled item focused; Escape closes it
/// and the row has the focus again.
pub fn scenario_context_menu_keys_sidebar(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let (app, window) = mount(cx, fixture.path());
    let row_focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).sidebar_row_focused_for_e2e(window)
        })
        .unwrap()
    };
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(LOCAL, "branch:feature", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    assert_eq!(row_focused(cx), Some((LOCAL, "branch:feature".into())));
    let anchor = cx
        .read(|cx| app.read(cx).sidebar_row_anchor_for_e2e())
        .expect("the focused row records where it is drawn");
    let row = e2e::control_bounds(window.window_id(), "sidebar-local-feature")
        .expect("feature row drawn");
    assert!(
        (f32::from(anchor.y) - f32::from(row.bottom())).abs() <= 2.0,
        "the anchor is the row's bottom ({anchor:?} vs {row:?})"
    );

    keys(cx, window, "shift-f10");
    let state = cx.read(|cx| app.read(cx).branch_menu.clone());
    let state = state.expect("Shift+F10 opens the row's branch menu");
    assert_eq!(state.name, "feature");
    assert_eq!(state.position, anchor, "the menu opens below the row");
    let (focused, enabled) = menu(cx, &app, window);
    assert_eq!(
        focused,
        enabled.first().copied(),
        "the first enabled item has the focus"
    );

    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_none()));
    assert_eq!(
        row_focused(cx),
        Some((LOCAL, "branch:feature".into())),
        "Escape gives the focus back to the row"
    );
    unmount(cx, app, window);
}

/// Focus the window and select the first commit, as of a fresh frame.
fn select_head(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.select(0);
            cx.notify();
        });
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
        window.draw(cx).clear();
    })
    .unwrap();
}

/// A commit menu left open while Home comes to the front is closed with the
/// tab it belonged to (#991 review): back on the tab no menu is up and ↓
/// moves the Graph's selection, not a hidden menu's focus.
pub fn scenario_context_menu_keys_home(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(
        fixture.path(),
        &["commit", "-q", "--allow-empty", "-m", "two"],
    );
    let (app, window) = mount(cx, fixture.path());
    select_head(cx, &app, window);
    keys(cx, window, "shift-f10");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus"
    );

    // ⌘T then ⌘W (their app-menu entries' methods: Tier A sends no
    // platform-menu keystroke).
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    // Checked before anything is parked: the tab's re-read on return also
    // drops the menus, but only once it lands, and until then a menu left up
    // would take Enter on the root (#991 review).
    assert!(
        cx.read(|cx| app.read(cx).commit_menu.is_none()),
        "leaving the tab closed its menu"
    );
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).home_in_front()));
    app.update(cx, |app, cx| app.close_home_tab(cx));
    assert!(
        cx.read(|cx| app.read(cx).commit_menu.is_none()),
        "no menu is up on the tab Home gave back"
    );
    cx.run_until_parked();
    assert_down_reaches_graph(cx, &app, window);
    unmount(cx, app, window);
}

/// A commit menu taller than a short window scrolls: End focuses the last
/// enabled item and scrolls it into the window (#991 review), so Enter can
/// never press an item the user cannot see.
pub fn scenario_context_menu_keys_short(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    kagi_ui_core::theme::set_zoom(1.67);
    // #1098 clamping must reveal the final disabled row too, not merely
    // stop at the final enabled action.
    const LAST: &str = "commit-menu-item-5-0";
    let fixture = build_fixture();
    crate::gui_evidence::fixture(fixture.path());
    let mount_at_height = |cx: &mut VisualTestAppContext, height: f32| {
        let state = e2e::app_state(fixture.path()).expect("fixture app state");
        let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
        let output = captured.clone();
        let window = open_offscreen(cx, size(px(1440.), px(height)), move |window, cx| {
            e2e::mount_root(state, window, cx, &output)
        });
        let app = captured.borrow().clone().expect("mounted KagiApp");
        cx.run_until_parked();
        (app, AnyWindowHandle::from(window))
    };
    // Derive the overflow fixture from actual rendered content, not a row-height
    // default or whatever zoom a preceding scenario happened to leave behind.
    let (probe, probe_window) = mount_at_height(cx, 1400.);
    select_head(cx, &probe, probe_window);
    keys(cx, probe_window, "shift-f10");
    for row in ["commit-menu-item-0-0", LAST] {
        e2e::clear_control_bounds(probe_window.window_id(), row);
    }
    menu(cx, &probe, probe_window);
    let first = e2e::control_bounds(probe_window.window_id(), "commit-menu-item-0-0")
        .expect("first item laid out");
    let last =
        e2e::control_bounds(probe_window.window_id(), LAST).expect("last enabled item laid out");
    let height = f32::from(last.bottom() - first.top()) * 0.65;
    assert!(
        height > 120.,
        "measured menu permits a usable short viewport"
    );
    unmount(cx, probe, probe_window);
    let (app, window) = mount_at_height(cx, height);
    select_head(cx, &app, window);
    keys(cx, window, "shift-f10");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
    let window_id = window.window_id();
    let bottom = |cx: &mut VisualTestAppContext| {
        e2e::clear_control_bounds(window_id, LAST);
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        let item = e2e::control_bounds(window_id, LAST).expect("last item laid out");
        f32::from(item.bottom())
    };
    assert!(
        bottom(cx) > height,
        "precondition: the menu is taller than the window"
    );
    keys(cx, window, "end");
    let (focused, enabled) = menu(cx, &app, window);
    assert_eq!(focused, enabled.last().copied());
    let after = bottom(cx);
    assert!(
        after <= height,
        "End scrolled the focused item into the window (bottom {after} > {height})"
    );
    unmount(cx, app, window);
}

/// A re-read resets the selection once it lands: select the first commit
/// until the selection holds, then ↓ must move the Graph's selection.
fn assert_down_reaches_graph(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        select_head(cx, app, window);
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).ui().selected) == Some(0) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the tab settles");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    keys(cx, window, "down");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(1),
        "↓ reaches the Graph"
    );
}

/// A branch menu left open across an external checkout: the reload that
/// lands closes it (its items were planned against the old HEAD; #991
/// review), and the focus goes back to the row it was opened from — no
/// item that is gone keeps it — so ↓ moves on from that row.
pub fn scenario_context_menu_keys_reload(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "feature"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let (app, window) = mount(cx, repo);
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(LOCAL, "branch:feature", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    keys(cx, window, "shift-f10");
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus"
    );

    git(repo, &["checkout", "-q", "feature"]);
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).branch_menu.is_none()),
        "the reload closed the menu"
    );
    // Nothing refocuses here: the close itself gave the focus back.
    let row_focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).sidebar_row_focused_for_e2e(window)
        })
        .unwrap()
    };
    assert_eq!(
        row_focused(cx),
        Some((LOCAL, "branch:feature".into())),
        "the focus went back to the row the menu was opened from"
    );
    keys(cx, window, "down");
    let moved = row_focused(cx);
    assert!(
        moved
            .as_ref()
            .is_some_and(|(pane, key)| *pane == LOCAL && key != "branch:feature"),
        "↓ moves on from that row ({moved:?})"
    );
    unmount(cx, app, window);
}

/// Tab and Shift+Tab inside a menu close it (#991 review) rather than walk
/// out to the controls behind it; the focus goes back to the window.
pub fn scenario_context_menu_keys_tab(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    // Selected once: selecting the selected row again would clear it.
    select_head(cx, &app, window);
    for key in ["tab", "shift-tab"] {
        keys(cx, window, "shift-f10");
        assert!(
            cx.read(|cx| app.read(cx).commit_menu.is_some()),
            "{key}: the menu opened"
        );
        keys(cx, window, key);
        assert!(
            cx.read(|cx| app.read(cx).commit_menu.is_none()),
            "{key} closed the menu"
        );
        assert!(
            root_focused(cx, &app, window),
            "{key}: the focus went back to the window"
        );
    }
    unmount(cx, app, window);
}

/// A notice drawn over an open menu (a modal that leaves the focus where it
/// was) closes the menu (#991 review): no hidden item keeps the focus for
/// Enter, which goes to the window — the notice's own key path.
pub fn scenario_context_menu_keys_covered(cx: &mut VisualTestAppContext) {
    use kagi::ui::modals::ActiveModal;
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    select_head(cx, &app, window);
    keys(cx, window, "shift-f10");
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus"
    );
    app.update(cx, |app, cx| {
        app.active_modal = Some(ActiveModal::AppNotice(
            "a notice over the menu".to_string().into(),
        ));
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).commit_menu.is_none()),
        "the notice closed the menu"
    );
    assert!(
        root_focused(cx, &app, window),
        "the focus left the hidden item for the window"
    );

    // The Keyboard Shortcuts Info panel takes no focus of its own:
    // it covers the menu as the notice did (#991 review). Settings and the
    // palette, which do take the focus, are the only overlays that do not.
    app.update(cx, |app, cx| {
        app.active_modal = None;
        cx.notify();
    });
    keys(cx, window, "shift-f10");
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus again"
    );
    app.update(cx, |app, cx| {
        app.menu_overlay = Some(kagi::ui::commands::MenuOverlay::Info {
            title: "Keyboard Shortcuts".into(),
            lines: vec![].into(),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        });
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).commit_menu.is_none()),
        "the Info panel closed the menu"
    );
    assert!(
        root_focused(cx, &app, window),
        "Info: the focus left the hidden item for the window"
    );
    unmount(cx, app, window);
}

/// A disabled item names its reason to assistive technology
/// (`aria_description`, #991 review); an enabled one has none.
pub fn scenario_context_menu_keys_a11y(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    select_head(cx, &app, window);
    keys(cx, window, "shift-f10");
    menu(cx, &app, window);
    let reset = kagi::ui::menu_overlay::recorded_item_description("commit-menu-item-5-0");
    assert!(
        reset
            .as_ref()
            .is_some_and(|d| d.as_ref().is_some_and(|d| !d.is_empty())),
        "Reset (disabled) is described by its reason ({reset:?})"
    );
    assert_eq!(
        kagi::ui::menu_overlay::recorded_item_description("commit-menu-item-0-0"),
        Some(None),
        "an enabled item has no description"
    );
    unmount(cx, app, window);
}

/// Open a focused sidebar branch row's menu with Shift+F10.
fn open_row_menu(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(LOCAL, "branch:feature", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    keys(cx, window, "shift-f10");
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus"
    );
}

/// An Info panel over a menu opened from a sidebar row closes it, and the
/// focus goes to the window — not back to the row behind the panel, which
/// would then take Enter / Space unseen (#991 review).
pub fn scenario_context_menu_keys_covered_row(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let (app, window) = mount(cx, fixture.path());
    open_row_menu(cx, &app, window);
    app.update(cx, |app, cx| {
        app.menu_overlay = Some(kagi::ui::commands::MenuOverlay::Info {
            title: "About".into(),
            lines: vec![].into(),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        });
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_none()));
    assert!(
        root_focused(cx, &app, window),
        "the focus went to the window, not back to the row"
    );
    unmount(cx, app, window);
}

/// A worktree command turns disabled while its submenu stays open. It keeps
/// focus, records a reason, remains inert on Enter, and ↓ reaches its sibling.
pub fn scenario_context_menu_keys_disabled_live(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let (app, window) = mount(cx, fixture.path());
    open_row_menu(cx, &app, window);
    focus_menu_control(cx, &app, window, "branch-menu-item-3-1");
    keys(cx, window, "right");
    const OPEN_WORKTREE: &str = "branch-menu-item-3-1-submenu-1-0";
    focus_menu_control(cx, &app, window, OPEN_WORKTREE);
    assert_eq!(
        kagi::ui::menu_overlay::recorded_item_description(OPEN_WORKTREE),
        Some(None)
    );
    let before = menu(cx, &app, window).0.expect("worktree item focused");
    // A live operation disables this same focused command in the open submenu.
    app.update(cx, |app, cx| {
        app.planning = Some("e2e");
        cx.notify();
    });
    let (focused, visible) = menu(cx, &app, window);
    assert_eq!(focused, Some(before), "the focus is still on that item");
    assert!(visible.contains(&before), "disabled rows remain focusable");
    let reason = kagi::ui::menu_overlay::recorded_item_description(OPEN_WORKTREE)
        .flatten()
        .expect("live-disabled worktree command records its reason");
    assert!(!reason.is_empty());
    keys(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
    assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));
    assert_eq!(cx.read(|cx| app.read(cx).submenu_depth_for_e2e()), 1);
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    keys(cx, window, "down");
    let (after, visible) = menu(cx, &app, window);
    assert!(
        after != Some(before) && after.is_some_and(|slot| visible.contains(&slot)),
        "↓ moves to the next visible submenu row"
    );
    app.update(cx, |app, cx| {
        app.planning = None;
        cx.notify();
    });
    unmount(cx, app, window);
}

/// A sibling appears while Advanced is open (Push becomes a disabled row
/// when busy). Focus must remain on Copy SHA and Enter must still copy the
/// exact SHA, not dispatch a different command after the live redraw.
pub fn scenario_context_menu_keys_item_appears(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let head = std::process::Command::new("git")
        .args(["rev-parse", "feature"])
        .current_dir(fixture.path())
        .output()
        .expect("git rev-parse");
    let head = String::from_utf8(head.stdout).unwrap().trim().to_string();
    let (app, window) = mount(cx, fixture.path());
    open_row_menu(cx, &app, window);
    focus_menu_control(cx, &app, window, "branch-menu-item-3-2");
    const PUSH: &str = "branch-menu-item-3-2-submenu-2-1";
    e2e::clear_control_bounds(window.window_id(), PUSH);
    keys(cx, window, "right");
    const COPY_SHA: &str = "branch-menu-item-3-2-submenu-0-3";
    let control = |cx: &mut VisualTestAppContext| {
        menu(cx, &app, window)
            .0
            .and_then(kagi::ui::menu_overlay::recorded_slot_control)
    };
    focus_menu_control(cx, &app, window, COPY_SHA);
    assert!(
        e2e::control_bounds(window.window_id(), PUSH).is_none(),
        "precondition: Push is hidden without an upstream"
    );
    assert_eq!(
        control(cx).as_deref(),
        Some(COPY_SHA),
        "focused Copy head SHA"
    );
    app.update(cx, |app, cx| {
        app.planning = Some("e2e");
        cx.notify();
    });
    assert_eq!(
        control(cx).as_deref(),
        Some(COPY_SHA),
        "the focus stayed on Copy head SHA when a disabled Push sibling appeared"
    );
    assert!(
        e2e::control_bounds(window.window_id(), PUSH).is_some(),
        "the live busy redraw really introduced Push"
    );
    assert!(
        kagi::ui::menu_overlay::recorded_item_description(PUSH)
            .flatten()
            .is_some_and(|reason| !reason.is_empty()),
        "the new Push sibling is disabled with a reason"
    );
    keys(cx, window, "enter");
    app.update(cx, |app, cx| {
        app.planning = None;
        cx.notify();
    });
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(head),
        "Enter ran the action that had the focus (Copy head SHA)"
    );
    unmount(cx, app, window);
}

/// Shift+F10 on a sidebar row while an Info panel is in front (it leaves the
/// row focused) does nothing (#1000): no branch menu, and the Graph does not
/// jump to the branch's commit behind the panel.
pub fn scenario_context_menu_keys_row_behind_info(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    // On the first commit, so opening its menu would move the selection.
    git(fixture.path(), &["branch", "feature", "HEAD~1"]);
    let (app, window) = mount(cx, fixture.path());
    select_head(cx, &app, window);
    app.update(cx, |app, cx| {
        app.menu_overlay = Some(kagi::ui::commands::MenuOverlay::Info {
            title: "About".into(),
            lines: vec![].into(),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        });
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(LOCAL, "branch:feature", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    keys(cx, window, "shift-f10");
    assert!(
        cx.read(|cx| app.read(cx).branch_menu.is_none()),
        "no branch menu behind the Info panel"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "the Graph did not jump to feature behind the Info panel"
    );
    unmount(cx, app, window);
}

/// Closing the last tab (⌘W) with a menu item focused leaves no menu and no
/// focus on an item Home never draws (#1000): the window holds the focus.
pub fn scenario_context_menu_keys_last_tab(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    select_head(cx, &app, window);
    keys(cx, window, "shift-f10");
    assert!(
        menu(cx, &app, window).0.is_some(),
        "the menu holds the focus"
    );
    cx.dispatch_action(window, kagi::ui::commands::CloseTab);
    assert!(
        cx.read(|cx| app.read(cx).tabs.is_empty()),
        "⌘W closed the last tab"
    );
    assert!(
        cx.read(|cx| app.read(cx).commit_menu.is_none()),
        "the menu went with the tab"
    );
    assert!(
        root_focused(cx, &app, window),
        "the window holds the focus on Welcome"
    );
    unmount(cx, app, window);
}
