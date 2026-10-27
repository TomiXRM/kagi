//! #1240: real failed-fetch notification reading and keyboard dismissal.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, ToastKind};
use std::time::{Duration, Instant};

pub fn scenario_toast_tooltip_lifetime(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let old_lang = kagi::ui::i18n::lang();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        // No seam here: the real hidden native window must suppress help.
        cx.update_window(window, |_, window, _| assert!(!window.is_window_active()))
            .unwrap();
        stack.update(cx, |stack, cx| {
            stack.push_notify(ToastKind::Error, "Inactive tooltip", cx);
        });
        paint(cx, window);
        let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
        let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}"))
            .expect("inactive dismiss control painted");
        cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert_tooltip_hidden(cx, window, "real inactive native window");
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(10.), gpui::px(300.)),
            None,
            Modifiers::none(),
        );
        stack.update(cx, |stack, cx| {
            stack.remove(id);
            cx.notify();
        });
        paint(cx, window);
        // Only substitute the active-state input for positive/lifecycle cases.
        let _active = kagi_ui_core::tooltip::e2e::active_window(window.window_id());
        for change in [
            "leave", "rerender", "move", "remove", "replace", "exit", "pending",
        ] {
            stack.update(cx, |stack, cx| {
                stack.push_notify(ToastKind::Error, "Tooltip lifetime", cx);
            });
            paint(cx, window);
            let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
            let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}"))
                .expect("dismiss control painted");
            cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
            if change != "pending" {
                cx.advance_clock(Duration::from_secs(1));
                cx.run_until_parked();
                paint(cx, window);
                assert!(
                    e2e::control_bounds(window.window_id(), "toast-dismiss-tooltip").is_some(),
                    "{lang:?}/{change}: hovered tooltip is visible"
                );
            }
            if change == "move" {
                crate::pr_detail_adaptive::resize(
                    cx,
                    window,
                    (
                        f32::from(viewport.width) - 200.,
                        f32::from(viewport.height) - 200.,
                    ),
                );
                let moved = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}"))
                    .unwrap();
                assert_ne!(
                    moved.origin, close.origin,
                    "same live trigger actually moved"
                );
            }
            stack.update(cx, |stack, cx| {
                match change {
                    "remove" | "pending" => stack.remove(id),
                    "replace" => stack.push_notify(ToastKind::Error, "Replacement preview", cx),
                    "exit" => stack.begin_exit(id, cx),
                    _ => {}
                }
                cx.notify();
            });
            settle_removal(cx, window);
            if matches!(change, "move" | "remove" | "replace" | "exit" | "pending") {
                assert_tooltip_hidden(cx, window, &format!("{lang:?}/{change}: retired hover"));
            }
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(10.), gpui::px(300.)),
                None,
                Modifiers::none(),
            );
            cx.advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
            assert_tooltip_hidden(cx, window, &format!("{lang:?}/{change}: pointer left"));
            stack.update(cx, |stack, cx| {
                let ids: Vec<_> = stack.toasts().iter().map(|toast| toast.id).collect();
                for id in ids {
                    stack.remove(id);
                }
                cx.notify();
            });
            if change == "move" {
                crate::pr_detail_adaptive::resize(
                    cx,
                    window,
                    (f32::from(viewport.width), f32::from(viewport.height)),
                );
            }
        }
    }
    kagi::ui::i18n::set_lang(old_lang);
    drop(stack);
    unmount(cx, app, window);
}

fn assert_tooltip_hidden(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
    context: &str,
) {
    e2e::clear_control_bounds(window.window_id(), "toast-dismiss-tooltip");
    paint(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "toast-dismiss-tooltip").is_none(),
        "{context}: tooltip must be hidden"
    );
}

pub fn scenario_toast_tooltip_scroll(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let old_lang = kagi::ui::i18n::lang();
    let old_zoom = theme::zoom();
    let fixture = build_fixture();
    for lang in [Lang::En, Lang::Ja] {
        let (app, window) = mount(cx, fixture.path());
        let _active = kagi_ui_core::tooltip::e2e::active_window(window.window_id());
        crate::surface_scrollbars::resize(cx, window);
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        theme::set_zoom(1.);
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        stack.update(cx, |stack, cx| {
            for _ in 0..4 {
                stack.push_notify(ToastKind::Error, "First line\nSecond line\nThird line", cx);
            }
        });
        crate::native_ax::activate(cx);
        paint(cx, window);
        assert!(crate::native_ax::press(
            cx,
            window,
            &kagi::ui::i18n::Msg::ToastMore.t().replace("{count}", "3"),
        ));
        cx.run_until_parked();
        paint(cx, window);
        let newest = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
        let close =
            e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{newest}")).unwrap();
        cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        paint(cx, window);
        assert!(e2e::control_bounds(window.window_id(), "toast-dismiss-tooltip").is_some());
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: close.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-1000.))),
                touch_phase: gpui::TouchPhase::Moved,
                modifiers: Modifiers::none(),
            },
        );
        cx.run_until_parked();
        assert_tooltip_hidden(cx, window, "scrolled trigger");
        paint(cx, window);
        let oldest = cx.read(|cx| stack.read(cx).toasts()[0].id);
        let close =
            e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{oldest}")).unwrap();
        let viewport = e2e::control_bounds(window.window_id(), "toast-stack").unwrap();
        assert!(
            viewport.contains(&close.center()),
            "oldest close scrolled into view"
        );
        cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        paint(cx, window);
        assert!(e2e::control_bounds(window.window_id(), "toast-dismiss-tooltip").is_some());
        assert!(crate::native_ax::press(
            cx,
            window,
            kagi::ui::i18n::Msg::ToastCollapse.t()
        ));
        cx.run_until_parked();
        assert_tooltip_hidden(cx, window, "collapsed older trigger");
        drop(stack);
        unmount(cx, app, window);
    }
    theme::set_zoom(old_zoom);
    kagi::ui::i18n::set_lang(old_lang);
}

pub fn scenario_toast_dismiss(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let old_zoom = theme::zoom();
    let old_lang = kagi::ui::i18n::lang();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(
        repo,
        &[
            "remote",
            "add",
            "origin",
            repo.join("missing").to_str().unwrap(),
        ],
    );
    let (app, window) = mount(cx, repo);
    crate::surface_scrollbars::resize(cx, window);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            theme::set_zoom(1.67);
            app.fetch_async(false, cx);
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        while cx.read(|cx| app.read(cx).fetch_in_flight.is_some()) {
            cx.run_until_parked();
            assert!(Instant::now() < deadline, "failed fetch settles");
            std::thread::sleep(Duration::from_millis(2));
        }
        let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("app.settings", window, cx)
            });
            window.draw(cx).clear();
        })
        .unwrap();
        cx.read(|cx| assert!(app.read(cx).menu_overlay.is_some(), "Settings owns Escape"));
        cx.simulate_keystrokes(window, "escape");
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(
                app.read(cx).menu_overlay.is_none(),
                "Escape closes Settings first"
            );
            assert!(
                stack.read(cx).toasts().last().unwrap().dismissing.is_none(),
                "menu cancellation preserves notification"
            );
        });
        copy_escape_contract(cx, &app, &stack, window);
        hovered_error(cx, &stack, window);
        let root = cx.read(|cx| app.read(cx).root_focus.clone().unwrap());
        three_line_error(cx, &stack, window, lang, &root);
        routine_hover(cx, &stack, window, &root);
        hidden_routine_hover(cx, &stack, window);
        focused_eviction(cx, &stack, window, &root);
        settings_reload_preserves_repository_focus(cx, &app, &stack, window, &root);
        let _ = kagi::ui::toast_stack::take_close_focus_for_e2e();
        drop(stack);
    }
    actionable_info_preserves_transient_focus(cx, &app, window);
    theme::set_zoom(old_zoom);
    kagi::ui::i18n::set_lang(old_lang);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS toast_dismiss: EN/JA hovered error survives eleven seconds and Escape dismisses");
}

fn hovered_error(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
) {
    let id = cx.read(|cx| {
        let toast = stack.read(cx).toasts().last().unwrap();
        assert_eq!(toast.kind, ToastKind::Error);
        toast.id
    });
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}"))
        .expect("close Button painted");
    assert!(close.origin.x >= gpui::px(0.) && close.origin.y >= gpui::px(0.));
    assert!(
        close.right() <= gpui::px(940.) && close.bottom() <= gpui::px(660.),
        "close Button inside window"
    );
    assert!(close.size.width >= gpui::px(20.) && close.size.height >= gpui::px(20.));
    cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(11))
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .any(|t| t.id == id && t.dismissing.is_none()),
            "hovered error remains after old eight-second deadline"
        )
    });
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .all(|t| t.id != id || t.dismissing.is_some()),
            "Escape closes newest toast"
        )
    });
}

fn three_line_error(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
    lang: Lang,
    root: &gpui::FocusHandle,
) {
    let lines = match lang {
        Lang::En => "First error line\nSecond error line\nThird error line",
        Lang::Ja => "エラーの一行目\nエラーの二行目\nエラーの三行目",
    };
    for message in [
        lines.to_string(),
        format!("{lines}\n四行目\n五行目\n六行目"),
        match lang {
            Lang::En => "Long error ".repeat(500),
            Lang::Ja => "長いエラー ".repeat(500),
        },
    ] {
        stack.update(cx, |stack, cx| {
            stack.push_notify(ToastKind::Error, message, cx)
        });
        let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
        paint(cx, window);
        let message = e2e::control_bounds(window.window_id(), &format!("toast-message-{id}"))
            .expect("error painted");
        let line_height = cx
            .update_window(window, |_, window, _| (window.rem_size() * 1.4).round())
            .unwrap();
        assert!(
            message.size.height >= line_height * 3. - gpui::px(1.),
            "three error lines have painted height"
        );
        assert!(
            message.size.height <= line_height * 3. + gpui::px(1.),
            "error preview bounded to three lines"
        );
        focus_close(cx, window);
        cx.simulate_keystrokes(window, "enter");
        cx.simulate_event(
            window,
            gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse("enter").unwrap(),
            },
        );
        settle_removal(cx, window);
        cx.read(|cx| {
            assert!(
                stack.read(cx).toasts().iter().all(|t| t.id != id),
                "Enter removes only focused card"
            )
        });
        cx.update_window(window, |_, window, _| {
            assert!(root.is_focused(window), "removed Button returns root focus")
        })
        .unwrap();
    }
}

fn paint(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn focus_close(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    let focus = kagi::ui::toast_stack::take_close_focus_for_e2e().expect("real close focus");
    for _ in 0..150 {
        cx.simulate_keystrokes(window, "tab");
        cx.run_until_parked();
        if cx
            .update_window(window, |_, window, _| focus.is_focused(window))
            .unwrap()
        {
            paint(cx, window);
            return;
        }
    }
    panic!("Tab reaches the newest close Button");
}

fn settle_removal(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    paint(cx, window);
    cx.run_until_parked();
    paint(cx, window);
}

fn hidden_routine_hover(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
) {
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Info, "Hover then hide", cx)
    });
    let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
    paint(cx, window);
    let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}")).unwrap();
    cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
    cx.read(|cx| assert!(stack.read(cx).toasts().last().unwrap().paused_at.is_some()));
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Error, "Replacement preview", cx)
    });
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(5))
    });
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    settle_removal(cx, window);
    cx.read(|cx| {
        assert!(
            stack.read(cx).toasts().iter().all(|toast| toast.id != id),
            "hidden hovered Info expires and decreases the notification count"
        )
    });
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Info, "Hover then collapse", cx)
    });
    let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
    crate::native_ax::activate(cx);
    paint(cx, window);
    let count = cx.read(|cx| stack.read(cx).toasts().len() - 1);
    let caption = if count == 1 {
        kagi::ui::i18n::Msg::ToastMoreOne.t().to_string()
    } else {
        kagi::ui::i18n::Msg::ToastMore
            .t()
            .replace("{count}", &count.to_string())
    };
    assert!(crate::native_ax::press(cx, window, &caption));
    cx.run_until_parked();
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Error, "Keep newest visible", cx);
        // Retained hover state models an older card clipped by the bounded scroller.
        stack.set_hovered(id, true);
    });
    paint(cx, window);
    assert!(crate::native_ax::press(
        cx,
        window,
        kagi::ui::i18n::Msg::ToastCollapse.t()
    ));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(stack
            .read(cx)
            .toasts()
            .iter()
            .find(|toast| toast.id == id)
            .unwrap()
            .paused_at
            .is_none())
    });
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(5))
    });
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    settle_removal(cx, window);
    cx.read(|cx| {
        assert!(
            stack.read(cx).toasts().iter().all(|toast| toast.id != id),
            "collapsed hovered Info expires"
        )
    });
}

fn routine_hover(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
    root: &gpui::FocusHandle,
) {
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Info, "Routine notification", cx)
    });
    let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
    paint(cx, window);
    let close = e2e::control_bounds(window.window_id(), &format!("toast-dismiss-{id}")).unwrap();
    cx.simulate_mouse_move(window, close.center(), None, Modifiers::none());
    cx.read(|cx| {
        assert!(
            stack.read(cx).toasts().last().unwrap().paused_at.is_some(),
            "real pointer pauses Info"
        )
    });
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(10))
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .any(|t| t.id == id && t.dismissing.is_none()),
            "hovered Info survives ten seconds"
        )
    });
    cx.simulate_mouse_move(
        window,
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        Modifiers::none(),
    );
    cx.read(|cx| {
        assert!(
            stack.read(cx).expiring_ids().is_empty(),
            "hover time excluded on leave"
        )
    });
    paint(cx, window);
    focus_close(cx, window);
    stack.update(cx, |stack, _| {
        stack.advance_age_for_e2e(Duration::from_secs(4))
    });
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    settle_removal(cx, window);
    cx.read(|cx| {
        assert!(
            stack.read(cx).toasts().iter().all(|t| t.id != id),
            "Info expires after remaining timer"
        )
    });
    cx.update_window(window, |_, window, _| {
        assert!(
            root.is_focused(window),
            "expired focused Info returns root focus"
        )
    })
    .unwrap();
}

fn copy_escape_contract(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
) {
    app.update(cx, |app, cx| {
        if app.ui().selected != Some(0) {
            app.select(0);
        }
        cx.notify();
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    cx.run_until_parked();
    paint(cx, window);
    crate::diff_split_unicode_badge::copy_body(cx, app, window);
    cx.run_until_parked();
    let id = cx.read(|cx| stack.read(cx).toasts().last().unwrap().id);
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(
        !e2e::diff_selection_present(),
        "Cmd+C then Escape clears diff selection first"
    );
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .any(|t| t.id == id && t.dismissing.is_none()),
            "R1 Escape leaves copy feedback"
        )
    });
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    settle_removal(cx, window);
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
    })
    .unwrap();
}

fn focused_eviction(
    cx: &mut VisualTestAppContext,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
    root: &gpui::FocusHandle,
) {
    stack.update(cx, |stack, cx| {
        for _ in 0..4 {
            stack.push_notify(ToastKind::Error, "Error preview", cx);
        }
    });
    // Older controls are intentionally absent until the counted stack opens.
    crate::native_ax::activate(cx);
    paint(cx, window);
    let count = cx.read(|cx| stack.read(cx).toasts().len() - 1);
    let caption = kagi::ui::i18n::Msg::ToastMore
        .t()
        .replace("{count}", &count.to_string());
    assert!(crate::native_ax::press(cx, window, &caption));
    cx.run_until_parked();
    paint(cx, window);
    let focus = cx.read(|cx| {
        stack.read(cx).toasts()[0]
            .close_focus
            .borrow()
            .iter()
            .find(|(host, _)| *host == window.window_id())
            .map(|(_, focus)| focus.clone())
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    paint(cx, window);
    stack.update(cx, |stack, cx| {
        stack.push_notify(ToastKind::Info, "Newest info", cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            stack.read(cx).toasts().last().unwrap().kind,
            ToastKind::Info,
            "Info admitted through four errors"
        )
    });
    cx.update_window(window, |_, window, _| {
        assert!(
            root.is_focused(window),
            "evicted focused card returns root focus"
        )
    })
    .unwrap();
    stack.update(cx, |stack, cx| {
        let ids: Vec<_> = stack.toasts().iter().map(|t| t.id).collect();
        for id in ids {
            stack.begin_exit(id, cx);
        }
    });
    settle_removal(cx, window);
}

fn settings_reload_preserves_repository_focus(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    stack: &gpui::Entity<kagi::ui::toast_stack::ToastStack>,
    window: gpui::AnyWindowHandle,
    root: &gpui::FocusHandle,
) {
    use kagi::ui::toast_stack::{ToastSpec, ToastSubject};
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    kagi::ui::KagiApp::hold_next_theme_reload_for_e2e(hold);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx);
            app.handle_menu_command("view.reloadThemes", window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).menu_overlay.is_some()));
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).menu_overlay.is_none()));
    let id = stack.update(cx, |stack, cx| {
        assert!(
            stack.toasts().is_empty(),
            "previous dismissal cases settled"
        );
        stack.push_keyed_notify(
            ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Info,
                "Older repository notice",
            ),
            cx,
        );
        stack.push_keyed_notify(
            ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Info,
                "Focused repository notice",
            ),
            cx,
        )
    });
    paint(cx, window);
    let focus = cx.read(|cx| {
        let toasts = stack.read(cx).toasts();
        assert!(
            toasts[0].close_focus.borrow().is_empty(),
            "older Repository notice is collapsed"
        );
        toasts[1]
            .close_focus
            .borrow()
            .iter()
            .find(|(host, _)| *host == window.window_id())
            .expect("newest Repository dismiss button painted")
            .1
            .clone()
    });
    cx.update_window(window, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    release.send(());
    cx.run_until_parked();
    // Check before repaint: recreating an emptied control store must not mask
    // the erroneous ToastRemoved consumed by KagiApp's production subscriber.
    cx.read(|cx| {
        let toasts = stack.read(cx).toasts();
        assert!(
            toasts
                .iter()
                .any(|toast| toast.subject == ToastSubject::Settings
                    && toast.kind == ToastKind::Success),
            "the held real Settings Reload completed with its captured subject"
        );
        let toast = toasts.iter().find(|toast| toast.id == id).unwrap();
        assert!(
            toast
                .close_focus
                .borrow()
                .iter()
                .any(|(host, stored)| *host == window.window_id() && *stored == focus),
            "arrival preserves the original Repository control handle"
        );
    });
    cx.update_window(window, |_, window, _| {
        assert!(
            focus.is_focused(window),
            "Settings completion preserves Repository keyboard focus"
        );
        assert!(
            !root.is_focused(window),
            "production removal subscriber must not steal focus"
        );
    })
    .unwrap();
    paint(cx, window);
    cx.update_window(window, |_, window, _| assert!(focus.is_focused(window)))
        .unwrap();
    stack.update(cx, |stack, cx| {
        let ids: Vec<_> = stack.toasts().iter().map(|toast| toast.id).collect();
        for id in ids {
            stack.begin_exit(id, cx);
        }
    });
    settle_removal(cx, window);
}

fn actionable_info_preserves_transient_focus(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
) {
    use kagi::ui::toast_stack::{ToastSpec, ToastSubject};
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let (stack, root) = cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.home_in_front(), "Home owns the split notification host");
        (
            app.toast_stack.clone().unwrap(),
            app.root_focus.clone().unwrap(),
        )
    });
    let id = stack.update(cx, |stack, cx| {
        assert!(stack.toasts().is_empty(), "prior focus cases settled");
        stack.push_keyed_notify(
            ToastSpec::new(ToastSubject::Repository, ToastKind::Info, "Older transient"),
            cx,
        );
        stack.push_keyed_notify(
            ToastSpec::new(
                ToastSubject::Repository,
                ToastKind::Info,
                "Focused transient",
            ),
            cx,
        )
    });
    paint(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "home-page-controls").is_some());
    assert!(e2e::control_bounds(window.window_id(), "toast-container").is_some());
    let focus = cx.read(|cx| {
        let stack = stack.read(cx);
        assert!(stack.toasts().iter().all(|toast| !toast.is_persistent()));
        assert!(
            stack.toasts()[0].close_focus.borrow().is_empty(),
            "older transient is collapsed, not an expanded-stack fixture"
        );
        stack.toasts()[1]
            .close_focus
            .borrow()
            .iter()
            .find(|(host, _)| *host == window.window_id())
            .expect("actual transient dismiss button painted")
            .1
            .clone()
    });
    cx.update_window(window, |_, window, cx| {
        focus.focus(window, cx);
        assert!(
            focus.is_focused(window),
            "actual Button owns focus before arrival"
        );
    })
    .unwrap();
    // This enters KagiApp's recorded-result presentation and its real removal
    // subscriber; attaching an action to an already-pushed test card cannot
    // establish the insertion-order contract.
    let arrived_id = app.update(cx, |app, cx| e2e::present_recorded_info_result(app, cx));
    cx.run_until_parked();
    for painted in [false, true] {
        if painted {
            paint(cx, window);
        }
        cx.read(|cx| {
            let stack = stack.read(cx);
            let arrived = stack.toasts().last().unwrap();
            assert_eq!(arrived.id, arrived_id, "recorded operation result arrived");
            assert_ne!(arrived.id, id);
            assert_eq!(arrived.kind, ToastKind::Info);
            assert!(
                matches!(
                    arrived.action,
                    Some(kagi::ui::ToastAction::OpenOperationLog(_))
                ),
                "arrival already owns its recorded-result action"
            );
            assert!(
                arrived.is_persistent(),
                "actionable Info is persistent on arrival"
            );
            let original = stack.toasts().iter().find(|toast| toast.id == id).unwrap();
            assert!(!original.is_persistent(), "original card remains transient");
            assert!(
                original.close_focus.borrow().is_empty(),
                "painted={painted}: collapsed preview no longer owns a control"
            );
        });
        cx.update_window(window, |_, window, _| {
            assert!(
                !focus.is_focused(window),
                "painted={painted}: hidden Button relinquishes focus"
            );
            assert!(
                root.is_focused(window),
                "painted={painted}: production subscriber repairs hidden focus"
            );
        })
        .unwrap();
    }
    assert!(e2e::control_bounds(window.window_id(), "toast-stack").is_some());
    stack.update(cx, |stack, cx| {
        let ids: Vec<_> = stack.toasts().iter().map(|toast| toast.id).collect();
        for id in ids {
            stack.begin_exit(id, cx);
        }
    });
    settle_removal(cx, window);
}
