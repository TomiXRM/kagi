//! #1198: production macOS Settings host, EN/JA × 100/167%.
use crate::macos::{build_fixture, drain_native_events, mount, mount_state, unmount};
use crate::recovery_layout::GlobalSettings;
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, i18n, settings_view::SettingsHostForE2e, theme};

fn draw(cx: &mut VisualTestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

type Leg = fn(&mut VisualTestAppContext, AnyWindowHandle, AnyWindowHandle);
fn settle(cx: &mut VisualTestAppContext, handle: AnyWindowHandle) {
    // Cocoa content-size updates and their bounds notifications are queued on
    // the native main loop, not the deterministic GPUI dispatcher.
    for _ in 0..4 {
        draw(cx, handle);
        drain_native_events();
        cx.run_until_parked();
    }
}

fn assert_screen_fit(cx: &mut VisualTestAppContext, settings: AnyWindowHandle, step: &str) {
    settle(cx, settings);
    cx.update_window(settings, |_, window, cx| {
        let frame = window.window_bounds().get_bounds();
        let visible = window.display(cx).expect("Settings display").visible_bounds();
        assert!(
            frame.left() >= visible.left() - gpui::px(1.)
                && frame.top() >= visible.top() - gpui::px(1.)
                && frame.right() <= visible.right() + gpui::px(1.)
                && frame.bottom() <= visible.bottom() + gpui::px(1.),
            "Settings entire frame must fit visible screen after {step} ({:?}/{}/{}) — frame {frame:?}, visible {visible:?}",
            i18n::lang(), theme::zoom(), theme::theme().slug
        );
    }).unwrap();
}

fn assert_manual_move_preserved(cx: &mut VisualTestAppContext, settings: AnyWindowHandle) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSRect;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let expected = cx
        .update_window(settings, |_, window, cx| {
            let mut bounds = window.window_bounds().get_bounds();
            let visible = window.display(cx).unwrap().visible_bounds();
            let origin = gpui::point(
                visible.right() - bounds.size.width + gpui::px(110.),
                visible.bottom() - bounds.size.height + gpui::px(90.),
            );
            let delta = origin - bounds.origin;
            bounds.origin = origin;
            let handle = HasWindowHandle::window_handle(window).unwrap();
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                panic!("native Settings requires AppKit");
            };
            // SAFETY: exact live GPUI NSView, accessed on the main thread. Retain
            // its NSWindow and move it outside GPUI's window borrow, like a drag.
            let (native, frame) = unsafe {
                let view = handle.ns_view.as_ptr().cast::<AnyObject>();
                let native: *mut AnyObject = msg_send![view, window];
                let mut frame: NSRect = msg_send![native, frame];
                frame.origin.x += f64::from(f32::from(delta.x));
                frame.origin.y -= f64::from(f32::from(delta.y));
                (Retained::retain(native).unwrap(), frame)
            };
            cx.foreground_executor()
                .spawn(async move {
                    // SAFETY: retained native window on the foreground executor.
                    unsafe {
                        let _: () = msg_send![&*native, setFrame: frame, display: true];
                    }
                })
                .detach();
            bounds
        })
        .unwrap();
    settle(cx, settings);
    cx.update_window(settings, |_, window, _| {
        assert_eq!(
            window.window_bounds().get_bounds(), expected,
            "ordinary bounds notifications and redraws must preserve a user's partially off-screen move"
        );
    }).unwrap();
}

fn assert_last_setting_reachable(
    cx: &mut VisualTestAppContext,
    repo: AnyWindowHandle,
    settings: AnyWindowHandle,
) {
    let view = cx.update_window(settings, |_, window, cx| {
        kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx).unwrap()
    }).unwrap();
    let scroll = cx.read(|cx| view.read(cx).scroll_handle());
    let viewport = crate::settings_controls::bounds(cx, settings, "settings-detail-scroll")
        .expect("existing page scrollbar viewport");
    cx.simulate_event(
        settings,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-10000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, settings);
    let last = crate::settings_controls::bounds(cx, settings, "settings-theme-guide")
        .expect("final Appearance action");
    assert!(
        last.top() >= viewport.top() && last.bottom() <= viewport.bottom(),
        "wheel reaches entire final control: {last:?} in {viewport:?}"
    );
    cx.simulate_event(
        settings,
        gpui::MouseMoveEvent {
            position: last.center(),
            ..Default::default()
        },
    );
    // Switching away/back uses the production scroll reset, rather than a test
    // setter that could hide an inaccessible keyboard path.
    for pane in ["settings-pane-general", "settings-pane-appearance"] {
        let at = crate::settings_controls::bounds(cx, settings, pane)
            .unwrap()
            .center();
        cx.simulate_click(settings, at, gpui::Modifiers::none());
        settle(cx, settings);
    }
    // Begin near the top with a real early control focused. Checking PageDown
    // only after Tab reveals the final action would not prove page scrolling.
    let first_card = format!("settings-theme-{}", theme::theme().slug);
    let mut first_focused = false;
    for _ in 0..80 {
        cx.simulate_keystrokes(settings, "tab");
        draw(cx, settings);
        first_focused = crate::settings_controls::probe(cx, settings, &first_card)
            .is_some_and(|probe| probe.focused);
        if first_focused {
            break;
        }
    }
    assert!(first_focused, "Tab reaches the selected theme card");
    let before_page = scroll.offset();
    cx.simulate_keystrokes(settings, "pagedown");
    draw(cx, settings);
    let after_down = scroll.offset();
    assert!(after_down.y < before_page.y - gpui::px(20.),
        "PageDown moves the actual detail scroll owner: {before_page:?} -> {after_down:?}");
    assert!(crate::settings_controls::probe(cx, settings, &first_card).unwrap().focused,
        "PageDown preserves the earlier control's keyboard focus");
    cx.simulate_keystrokes(settings, "pageup");
    draw(cx, settings);
    let after_up = scroll.offset();
    assert!(after_up.y > after_down.y + gpui::px(20.) && after_up.y <= gpui::px(0.),
        "PageUp returns toward the earlier offset: {after_down:?} -> {after_up:?}");
    assert!((after_up.y - before_page.y).abs() < (after_down.y - before_page.y).abs());
    assert!(crate::settings_controls::probe(cx, settings, &first_card).unwrap().focused,
        "PageUp preserves keyboard focus");
    let mut reached = false;
    for _ in 0..80 {
        cx.simulate_keystrokes(settings, "tab");
        draw(cx, settings);
        reached = crate::settings_controls::probe(cx, settings, "settings-theme-guide")
            .is_some_and(|probe| probe.focused);
        if reached {
            break;
        }
    }
    assert!(reached, "real Tab reaches the final Appearance action");
    let last = crate::settings_controls::bounds(cx, settings, "settings-theme-guide").unwrap();
    assert!(
        last.top() >= viewport.top() && last.bottom() <= viewport.bottom(),
        "keyboard reveals entire final control: {last:?} in {viewport:?}"
    );
    crate::native_ax::activate(cx);
    draw(cx, settings);
    let node = crate::native_ax::node(cx, settings, "settings-theme-guide".into());
    assert_eq!(
        node["aria"]["label"],
        kagi::ui::i18n::Msg::SettingsCustomThemesGuide.t()
    );
    assert_analyze_page_keys_stay_in_editor(cx, repo, settings);
}

fn assert_analyze_page_keys_stay_in_editor(
    cx: &mut VisualTestAppContext,
    repo: AnyWindowHandle,
    settings: AnyWindowHandle,
) {
    use gpui::Focusable as _;
    use gpui_component::scroll::ScrollbarHandle as _;
    let owner = cx.update_window(repo, |_, window, cx| {
        window.root::<gpui_component::Root>().flatten().unwrap()
            .read(cx).view().clone().downcast::<kagi::ui::KagiApp>().unwrap()
    }).unwrap();
    crate::settings_controls::click(cx, settings, "settings-pane-analyze");
    let input = cx.read(|cx| owner.read(cx).analyze_ignore_input.clone().unwrap());
    let view = cx.read(|cx| owner.read(cx).settings_entity().unwrap());
    let scroll = cx.read(|cx| view.read(cx).scroll_handle());
    let original = cx.read(|cx| input.read(cx).value());
    let draft = (0..120).map(|line| format!("page-key-fixture-{line}/\n")).collect::<String>();
    cx.update_window(settings, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(draft.clone(), window, cx));
        input.read(cx).focus_handle(cx).focus(window, cx);
    }).unwrap();
    draw(cx, settings);
    let viewport = crate::settings_controls::bounds(cx, settings, "settings-detail-scroll").unwrap();
    if scroll.content_size().height > viewport.size.height + gpui::px(20.) {
        // Where Analyze itself overflows, demonstrate that its outer owner
        // handles the same key with a button focused before testing the editor.
        cx.update_window(settings, |_, window, cx| {
            view.update(cx, |view, cx| view.focus_control("settings-analyze-save", window, cx));
        }).unwrap();
        draw(cx, settings);
        let button_offset = scroll.offset();
        cx.simulate_keystrokes(settings, "pageup");
        draw(cx, settings);
        assert!(scroll.offset().y > button_offset.y + gpui::px(1.),
            "Analyze PageUp moves the detail owner when a non-editor control is focused");
        cx.update_window(settings, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }).unwrap();
        draw(cx, settings);
    }
    let before = scroll.offset();
    for key in ["pagedown", "pageup"] {
        cx.simulate_keystrokes(settings, key);
        draw(cx, settings);
        assert_eq!(scroll.offset(), before,
            "{key} consumed by Analyze editor must not move the surrounding detail pane");
        assert!(cx.update_window(settings, |_, window, cx| {
            input.read(cx).focus_handle(cx).is_focused(window)
        }).unwrap(), "page navigation retains Analyze input focus");
        assert_eq!(cx.read(|cx| input.read(cx).value()).as_ref(), draft.as_str(),
            "page navigation cannot change the Analyze draft");
    }
    cx.update_window(settings, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(original, window, cx));
    }).unwrap();
    crate::settings_controls::click(cx, settings, "settings-pane-appearance");
}

pub fn scenario_settings_window_screen_fit(cx: &mut VisualTestAppContext) {
    configurations(cx, |cx| {
        for appearance in ["apple-light", "apple-dark"] {
            theme::set_active(appearance);
            kagi::ui::settings::write_setting("settings_window_frame", None);
            cx.update(theme::sync_gpui_component_theme);
            run_cell(cx, |cx, repo, settings| {
                let repo_before = cx
                    .update_window(repo, |_, window, _| window.window_bounds())
                    .unwrap();
                assert_screen_fit(cx, settings, "first content-driven resize");
                assert_manual_move_preserved(cx, settings);
                for pane in kagi_ui_settings::Pane::ALL {
                    let bounds = crate::settings_controls::bounds(
                        cx,
                        settings,
                        &format!("settings-pane-{}", pane.id()),
                    )
                    .expect("pane control");
                    cx.simulate_click(settings, bounds.center(), gpui::Modifiers::none());
                    assert_screen_fit(cx, settings, "pane switch");
                }
                crate::settings_controls::click(cx, settings, "settings-pane-appearance");
                let zoom = theme::zoom();
                theme::set_zoom(1.);
                cx.update(theme::sync_gpui_component_theme);
                assert_screen_fit(cx, settings, "100% zoom");
                theme::set_zoom(1.67);
                cx.update(theme::sync_gpui_component_theme);
                assert_screen_fit(cx, settings, "100% to 167% zoom");
                assert_last_setting_reachable(cx, repo, settings);
                theme::set_zoom(zoom);
                cx.update(theme::sync_gpui_component_theme);
                assert_screen_fit(cx, settings, "zoom restoration");
                cx.simulate_keystrokes(settings, "cmd-w");
                cx.run_until_parked();
                drain_native_events();

                // A titlebar on a connected screen does not qualify the entire
                // saved frame: both right and bottom edges need correction.
                let displays = cx.read(|cx| cx.displays());
                for display in displays {
                    let visible = display.visible_bounds();
                    kagi::ui::settings::write_settings_window_frame(
                        kagi::ui::settings::SettingsWindowFrame {
                            x: f32::from(visible.right()) - 120.,
                            y: f32::from(visible.bottom()) - 120.,
                            height: 900.,
                            display_uuid: display.uuid().ok().map(|uuid| uuid.as_u128()),
                        },
                    );
                    cx.simulate_keystrokes(repo, "cmd-,");
                    cx.run_until_parked();
                    let reopened = cx.read(|cx| {
                        cx.windows()
                            .into_iter()
                            .find(|w| w.window_id() != repo.window_id())
                            .expect("restored Settings")
                    });
                    assert_screen_fit(cx, reopened, "off-edge saved frame restore");
                    cx.update_window(reopened, |_, window, cx| {
                        assert_eq!(
                            window.display(cx).unwrap().id(),
                            display.id(),
                            "saved position chooses its own connected display"
                        );
                    })
                    .unwrap();
                    cx.simulate_keystrokes(reopened, "cmd-w");
                    cx.run_until_parked();
                    drain_native_events();
                    cx.simulate_keystrokes(repo, "cmd-,");
                    cx.run_until_parked();
                    let reopened = cx.read(|cx| {
                        cx.windows()
                            .into_iter()
                            .find(|w| w.window_id() != repo.window_id())
                            .unwrap()
                    });
                    assert_screen_fit(cx, reopened, "reopen");
                    cx.simulate_keystrokes(reopened, "cmd-w");
                    cx.run_until_parked();
                    drain_native_events();
                }
                assert_eq!(
                    cx.update_window(repo, |_, window, _| window.window_bounds())
                        .unwrap(),
                    repo_before,
                    "Settings fitting leaves repository geometry unchanged"
                );
            });
        }
    });
    eprintln!("[gui-e2e] PASS settings_window_screen_fit");
}

pub(crate) struct SettingsHostGuard {
    cx: VisualTestAppContext,
    previous: bool,
}

impl SettingsHostGuard {
    pub(crate) fn native(cx: &mut VisualTestAppContext) -> Self {
        Self::select(cx, true)
    }

    pub(crate) fn portable(cx: &mut VisualTestAppContext) -> Self {
        Self::select(cx, false)
    }

    fn select(cx: &mut VisualTestAppContext, native: bool) -> Self {
        let previous = cx.read(|cx| cx.global::<SettingsHostForE2e>().native);
        cx.update(|cx| cx.set_global(SettingsHostForE2e { native }));
        Self {
            cx: cx.clone(),
            previous,
        }
    }
}

impl Drop for SettingsHostGuard {
    fn drop(&mut self) {
        self.cx.update(|cx| {
            cx.set_global(SettingsHostForE2e {
                native: self.previous,
            })
        });
    }
}

struct AutoFetchGuard(bool);

impl Drop for AutoFetchGuard {
    fn drop(&mut self) {
        theme::set_auto_fetch(self.0);
    }
}

struct QuitGuard(VisualTestAppContext);

impl QuitGuard {
    fn capture(cx: &mut VisualTestAppContext) -> Self {
        cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
        Self(cx.clone())
    }
}

impl Drop for QuitGuard {
    fn drop(&mut self) {
        self.0.update(|cx| {
            cx.remove_global::<e2e::QuitForE2e>();
        });
    }
}

fn configurations(cx: &mut VisualTestAppContext, mut run: impl FnMut(&mut VisualTestAppContext)) {
    let _saved = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "lang",
        "ui_zoom",
        "settings_pane",
        "settings_window_position",
        "settings_window_height",
        "settings_window_frame",
        "auto_fetch",
    ]);
    let _host = SettingsHostGuard::native(cx);
    let _auto_fetch = AutoFetchGuard(theme::auto_fetch());
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            i18n::set_lang(lang);
            theme::set_zoom(zoom);
            theme::set_active("apple-light");
            kagi::ui::settings::write_setting("settings_pane", Some("appearance"));
            cx.update(theme::sync_gpui_component_theme);
            run(cx);
        }
    }
}

fn matrix(cx: &mut VisualTestAppContext, leg: Leg) {
    configurations(cx, |cx| run_cell(cx, leg));
}

fn run_cell(cx: &mut VisualTestAppContext, leg: Leg) {
    let lang = i18n::lang();
    let zoom = theme::zoom();
    let fixture = build_fixture();
    let (app, repo) = mount(cx, fixture.path());
    let tab_state = cx.read(|cx| {
        let app = app.read(cx);
        (app.tabs.clone(), app.active_tab, app.repo_path.clone())
    });
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let windows = cx.read(|cx| cx.windows());
    assert_eq!(
        windows.len(),
        2,
        "Cmd-, opens one separate Settings window ({lang:?}/{zoom})"
    );
    let settings = *windows
        .iter()
        .find(|w| w.window_id() != repo.window_id())
        .unwrap();
    draw(cx, settings);
    leg(cx, repo, settings);
    assert_eq!(
        cx.read(|cx| {
            let app = app.read(cx);
            (app.tabs.clone(), app.active_tab, app.repo_path.clone())
        }),
        tab_state,
        "Settings never changes repository tabs or selection"
    );
    if cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == settings.window_id())
    }) {
        cx.update_window(settings, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
    }
    unmount(cx, app, repo);
}

pub fn scenario_settings_window_open_raise(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(
            windows.len(),
            2,
            "second Cmd-, never creates a third window"
        );
        assert!(
            windows
                .iter()
                .any(|w| w.window_id() == settings.window_id()),
            "same host survives raise"
        );
        cx.simulate_keystrokes(settings, "cmd-,");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| cx.windows().len()), 2);
        cx.simulate_keystrokes(settings, "escape");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            2,
            "Escape does not close Settings"
        );
        let pane = crate::settings_controls::bounds(cx, settings, "settings-pane-general")
            .expect("General pane")
            .center();
        cx.simulate_click(settings, pane, gpui::Modifiers::none());
        draw(cx, settings);
        assert_eq!(
            kagi::ui::settings::read_setting("settings_pane").as_deref(),
            Some("general")
        );
        cx.simulate_keystrokes(settings, "cmd-w");
        cx.run_until_parked();
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let reopened = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .expect("reopened Settings")
        });
        draw(cx, reopened);
        assert!(
            crate::settings_controls::probe(cx, reopened, "settings-pane-general")
                .unwrap()
                .selected,
            "reopen restores the last pane"
        );
        cx.update_window(reopened, |_, window, _| {
            assert_eq!(
                window.viewport_size().width,
                gpui::px(715.),
                "fixed logical width at either zoom"
            );
        })
        .unwrap();
        cx.simulate_keystrokes(reopened, "cmd-w");
        cx.run_until_parked();
    });
    configurations(cx, |cx| {
        for (legacy, expected) in [
            ("0", kagi_ui_settings::Pane::Appearance),
            ("1", kagi_ui_settings::Pane::General),
            ("2", kagi_ui_settings::Pane::SmartCommit),
            ("3", kagi_ui_settings::Pane::Analyze),
        ] {
            kagi::ui::settings::write_setting("settings_pane", Some(legacy));
            let fixture = build_fixture();
            let (app, repo) = mount(cx, fixture.path());
            cx.simulate_keystrokes(repo, "cmd-,");
            cx.run_until_parked();
            let host = cx.read(|cx| {
                cx.windows()
                    .into_iter()
                    .find(|window| window.window_id() != repo.window_id())
                    .unwrap()
            });
            draw(cx, host);
            assert!(
                crate::settings_controls::probe(
                    cx,
                    host,
                    &format!("settings-pane-{}", expected.id())
                )
                .unwrap()
                .selected
            );
            assert_eq!(
                kagi::ui::settings::read_setting("settings_pane").as_deref(),
                Some(expected.id()),
                "legacy pane migrates to a stable ID"
            );
            cx.simulate_keystrokes(host, "cmd-w");
            cx.run_until_parked();
            unmount(cx, app, repo);
        }
    });
    eprintln!("[gui-e2e] PASS settings_window_open_raise");
}

pub fn scenario_settings_window_close_keeps_repo(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        // Record repository root identity as well as all measured tab geometry;
        // Cmd-W must act in the settings dispatch tree, not the repository one.
        let before = cx
            .update_window(repo, |root, _, _| root.entity_id())
            .unwrap();
        cx.simulate_keystrokes(settings, "cmd-w");
        cx.run_until_parked();
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(windows.len(), 1, "Cmd-W closes only Settings");
        assert_eq!(windows[0].window_id(), repo.window_id());
        assert_eq!(
            cx.update_window(repo, |root, _, _| root.entity_id())
                .unwrap(),
            before
        );
        draw(cx, repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_close_keeps_repo");
}

pub fn scenario_settings_window_live_apply(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        let fixture = build_fixture();
        let (other_app, other_repo) = mount(cx, fixture.path());
        let before_token = theme::theme().chrome_surface();
        crate::settings_controls::click(cx, settings, "settings-theme-apple-dark");
        cx.run_until_parked();
        assert_ne!(
            theme::theme().slug,
            "apple-light",
            "theme card applies a different theme through its production handler"
        );
        // The blank toolbar drag region shows the unified frame's chrome fill,
        // not the panel fill (distinct in Apple Dark's elevation ladder).
        let token = theme::theme().chrome_surface();
        assert_ne!(
            token, before_token,
            "the selected theme changes repository chrome"
        );
        for repo in [repo, other_repo] {
            draw(cx, repo);
            let at = crate::settings_controls::bounds(cx, repo, "toolbar-drag-region")
                .expect("blank repository chrome")
                .center();
            cx.update_window(repo, |_, window, _| {
                let image = window.render_to_image().expect("repository native image");
                let scale = window.scale_factor();
                let pixel = image
                    .get_pixel(
                        (f32::from(at.x) * scale) as u32,
                        (f32::from(at.y) * scale) as u32,
                    )
                    .0;
                for (got, expected) in
                    pixel[..3]
                        .iter()
                        .zip([(token >> 16) as u8, (token >> 8) as u8, token as u8])
                {
                    assert!(
                        got.abs_diff(expected) <= 2,
                        "repository paints new chrome token: {pixel:?}, expected {token:06x}"
                    );
                }
            })
            .unwrap();
        }
        unmount(cx, other_app, other_repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_live_apply");
}

pub fn scenario_settings_window_owner_release(cx: &mut VisualTestAppContext) {
    configurations(cx, |cx| {
        let fixture = build_fixture();
        let (app, repo) = mount(cx, fixture.path());
        theme::set_auto_fetch(true);
        app.update(cx, |app, cx| app.ensure_auto_fetch_ticker(cx));
        assert!(cx.read(|cx| app.read(cx).auto_fetch_ticker_alive));
        draw(cx, repo);
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let settings = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .expect("Settings opened")
        });
        draw(cx, settings);
        let other_fixture = build_fixture();
        let (other_app, other_repo) = mount(cx, other_fixture.path());
        let weak = app.downgrade();
        unmount(cx, app, repo);
        assert!(
            weak.upgrade().is_none(),
            "Settings and its rendered callbacks must not retain the closed repository"
        );
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(windows.len(), 1, "owner release also closes Settings");
        assert_eq!(windows[0].window_id(), other_repo.window_id());
        draw(cx, other_repo);
        unmount(cx, other_app, other_repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_owner_release");
}

pub fn scenario_settings_window_position_restore(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        let pane = crate::settings_controls::bounds(cx, settings, "settings-pane-general")
            .expect("General pane")
            .center();
        cx.simulate_click(settings, pane, gpui::Modifiers::none());
        settle(cx, settings);
        let before = cx
            .update_window(settings, |_, window, _| window.bounds())
            .unwrap();
        let mut current = settings;
        for _ in 0..3 {
            cx.simulate_keystrokes(current, "cmd-w");
            cx.run_until_parked();
            drain_native_events();
            cx.simulate_keystrokes(repo, "cmd-,");
            cx.run_until_parked();
            current = cx.read(|cx| {
                cx.windows()
                    .into_iter()
                    .find(|w| w.window_id() != repo.window_id())
                    .unwrap()
            });
            settle(cx, current);
            let after = cx
                .update_window(current, |_, window, _| window.bounds())
                .unwrap();
            assert!(
                (f32::from(before.origin.x) - f32::from(after.origin.x)).abs() <= 1.
                    && (f32::from(before.origin.y) - f32::from(after.origin.y)).abs() <= 1.
                    && (f32::from(before.size.height) - f32::from(after.size.height)).abs() <= 1.,
                "Settings frame must not drift across reopen: {before:?} -> {after:?}"
            );
        }
        cx.simulate_keystrokes(current, "cmd-w");
        cx.run_until_parked();
    });
    configurations(cx, restart_settings_after_app_quit);
    eprintln!("[gui-e2e] PASS settings_window_position_restore");
}

fn restart_settings_after_app_quit(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    // Start at a different saved height, then let the actual page resize.
    let mut frame = kagi::ui::settings::settings_window_frame().unwrap_or(
        kagi::ui::settings::SettingsWindowFrame {
            x: 100.,
            y: 100.,
            height: 500.,
            display_uuid: None,
        },
    );
    frame.height = 500.;
    kagi::ui::settings::write_settings_window_frame(frame);
    let (app, repo) = mount(cx, fixture.path());
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != repo.window_id())
            .unwrap()
    });
    settle(cx, settings);
    let before = cx
        .update_window(settings, |_, window, _| {
            (window.bounds(), f32::from(window.viewport_size().height))
        })
        .unwrap();
    assert!(before.1 > 500., "Appearance pane resized the initial frame");
    let _quit = QuitGuard::capture(cx);
    cx.simulate_keystrokes(repo, "cmd-q");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested),
        "repository Cmd-Q dispatched Quit"
    );
    let saved_height = kagi::ui::settings::settings_window_frame()
        .expect("frame persisted at app quit")
        .height;
    assert!(
        (saved_height - before.1).abs() <= 1.,
        "app quit persists Settings height without closing it"
    );
    // Process shutdown does not dispatch per-window close. Dispose Settings
    // first so owner-release cleanup cannot accidentally save missing geometry.
    cx.update_window(settings, |_, window, _| window.remove_window())
        .unwrap();
    unmount(cx, app, repo);
    let (app, repo) = mount(cx, fixture.path());
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != repo.window_id())
            .unwrap()
    });
    settle(cx, settings);
    let after = cx
        .update_window(settings, |_, window, _| window.bounds())
        .unwrap();
    assert!(
        (f32::from(before.0.origin.x) - f32::from(after.origin.x)).abs() <= 1.
            && (f32::from(before.0.origin.y) - f32::from(after.origin.y)).abs() <= 1.
            && (f32::from(before.0.size.height) - f32::from(after.size.height)).abs() <= 1.,
        "Settings frame must not drift after app quit/restart: {:?} -> {after:?}",
        before.0
    );
    cx.simulate_keystrokes(settings, "cmd-w");
    cx.run_until_parked();
    unmount(cx, app, repo);
}

pub fn scenario_settings_window_dock_reopen(cx: &mut VisualTestAppContext) {
    configurations(cx, |cx| {
        let _session = crate::gui_isolation::SavedKeys::keep(&["session_repos", "session_active"]);
        let fixture = build_fixture();
        let (app, repo) = mount(cx, fixture.path());
        draw(cx, repo);
        assert!(
            cx.update(e2e::dock_reopen_state).is_none(),
            "live repository prevents duplicate"
        );
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let settings = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .unwrap()
        });
        draw(cx, settings);
        // Keep the harness's observation handle until after the reopen decision:
        // the repository root is gone, but Settings still exists in this turn.
        cx.update_window(repo, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| cx.windows().len()), 1);
        kagi::ui::settings::write_setting("session_repos", fixture.path().to_str());
        kagi::ui::settings::write_setting("session_active", Some("0"));
        let fresh = cx
            .update(e2e::dock_reopen_state)
            .expect("Settings-only Dock reopen restores repo");
        assert_eq!(fresh.tabs.len(), 1);
        assert_eq!(fresh.tabs[0].path, fixture.path());
        let (restored, reopened) = mount_state(cx, fresh);
        assert_eq!(cx.read(|cx| cx.windows().len()), 2);
        assert!(cx.update(e2e::dock_reopen_state).is_none());
        let weak = app.downgrade();
        drop(app);
        cx.update(|_| {});
        cx.run_until_parked();
        drain_native_events();
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(
            weak.upgrade().is_none(),
            "closed repository is released after Dock reopen"
        );
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            1,
            "old owner's Settings closes on release"
        );
        draw(cx, reopened);
        unmount(cx, restored, reopened);
    });
    eprintln!("[gui-e2e] PASS settings_window_dock_reopen");
}

pub fn scenario_settings_window_host_isolation(cx: &mut VisualTestAppContext) {
    assert!(!cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _host = SettingsHostGuard::native(cx);
        assert!(cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
        std::panic::resume_unwind(Box::new("exercise Settings host guard Drop"));
    }));
    assert!(unwound.is_err());
    assert!(!cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
    eprintln!("[gui-e2e] PASS settings_window_host_isolation");
}
