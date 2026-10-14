//! #1566: literal commit URLs have safe pointer and keyboard actions.
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::{px, ClipboardItem, VisualTestAppContext};
use kagi::ui::{e2e, i18n, theme};

const URL: &str = "https://example.com/audit/round16?q=one&lang=ja";
const HTTP_URL: &str = "http://example.com/docs_(guide)";

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = tempfile::tempdir().unwrap();
    git(fixture.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(fixture.path().join("file.txt"), "links\n").unwrap();
    git(fixture.path(), &["add", "."]);
    let message = format!("Message links\n\n参照（{URL}）。 ({HTTP_URL}).\n<script>alert(1)</script> javascript:alert(1) file:///tmp/no\n[not Markdown](ftp://example.com)");
    git(fixture.path(), &["commit", "-qm", &message]);
    let before = repo_fingerprint(fixture.path());
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        for preset in ["apple-light", "apple-dark"] {
            assert!(theme::set_active(preset));
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                let (app, window) = mount(cx, fixture.path());
                cx.update(theme::sync_gpui_component_theme);
                app.update(cx, |app, cx| {
                    app.inspector_visible = true;
                    app.panel_width = 360.;
                    app.select(0);
                    cx.notify();
                });
                cx.run_until_parked();
                for _ in 0..4 {
                    cx.update_window(window, |_, w, cx| {
                        w.refresh();
                        w.draw(cx).clear();
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
                let bounds = e2e::control_bounds(window.window_id(), "inspector-message-link-0")
                    .expect("explicit HTTPS URL must expose an Inspector link");
                assert!(bounds.size.height >= px(18.));
                crate::native_ax::activate(cx);
                cx.update_window(window, |_, w, cx| {
                    w.refresh();
                    w.draw(cx).clear();
                })
                .unwrap();
                let link = crate::native_ax::node(cx, window, "inspector-message-link-0".into());
                assert_eq!(link["aria"]["role"], "Link");
                let open_name = format!("{}: {URL}", i18n::Msg::InspectorOpenLink.t());
                let copy_name = format!("{}: {URL}", i18n::Msg::InspectorCopyLink.t());
                assert_eq!(link["aria"]["label"], open_name);
                let copy_node =
                    crate::native_ax::node(cx, window, "inspector-message-copy-link-0".into());
                assert_eq!(copy_node["aria"]["role"], "Button");
                assert_eq!(copy_node["aria"]["label"], copy_name);
                e2e::take_opened_urls();
                // Buttons deliberately retain mouse focus elsewhere. Reach
                // this exact native Link node through ordinary Tab navigation.
                let identity = format!("{:?}", gpui::ElementId::from("inspector-message-link-0"));
                let mut reached = false;
                for _ in 0..160 {
                    crate::keyboard_nav::keys(cx, window, "tab");
                    cx.update_window(window, |_, w, cx| {
                        w.refresh();
                        w.draw(cx).clear();
                    })
                    .unwrap();
                    let tree = crate::native_ax::tree(cx, window);
                    if tree["gpui_focus"].as_str().is_some_and(|key| {
                        tree["nodes"][key]["element_id"].as_str() == Some(identity.as_str())
                    }) {
                        reached = true;
                        break;
                    }
                }
                assert!(reached, "Tab reaches the actual message Link");
                for _ in 0..3 {
                    cx.run_until_parked();
                    cx.update_window(window, |_, w, cx| {
                        w.refresh();
                        w.draw(cx).clear();
                    })
                    .unwrap();
                }
                let bounds =
                    e2e::control_bounds(window.window_id(), "inspector-message-link-0").unwrap();
                let viewport = cx.read(|cx| app.read(cx).ui().inspector_message_scroll.bounds());
                assert!(
                    bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
                    "focused Link must be revealed: {bounds:?} in {viewport:?}"
                );
                let revealed = cx.read(|cx| app.read(cx).ui().inspector_message_scroll.offset());
                if revealed.y < gpui::px(0.) {
                    cx.simulate_event(
                        window,
                        gpui::ScrollWheelEvent {
                            position: viewport.center(),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(
                                gpui::px(0.),
                                gpui::px(10_000.),
                            )),
                            touch_phase: gpui::TouchPhase::Moved,
                            ..Default::default()
                        },
                    );
                    for _ in 0..3 {
                        cx.run_until_parked();
                        cx.update_window(window, |_, w, cx| {
                            w.refresh();
                            w.draw(cx).clear();
                        })
                        .unwrap();
                    }
                    let after = cx.read(|cx| app.read(cx).ui().inspector_message_scroll.offset());
                    assert_eq!(
                        after.y,
                        gpui::px(0.),
                        "wheel scrolling must not be pulled back to the focused Link"
                    );
                    crate::keyboard_nav::keys(cx, window, "shift-tab tab");
                    for _ in 0..3 {
                        cx.run_until_parked();
                        cx.update_window(window, |_, w, cx| {
                            w.refresh();
                            w.draw(cx).clear();
                        })
                        .unwrap();
                    }
                }
                let bounds =
                    e2e::control_bounds(window.window_id(), "inspector-message-link-0").unwrap();
                cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
                cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
                assert_eq!(e2e::take_opened_urls(), vec![URL]);
                crate::keyboard_nav::keys(cx, window, "enter");
                assert_eq!(e2e::take_opened_urls(), vec![URL]);
                crate::keyboard_nav::keys(cx, window, "space");
                assert_eq!(e2e::take_opened_urls(), vec![URL]);
                crate::keyboard_nav::keys(cx, window, "shift-tab tab");
                assert!(crate::native_ax::press(cx, window, &open_name));
                cx.run_until_parked();
                assert_eq!(e2e::take_opened_urls(), vec![URL]);
                cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
                crate::keyboard_nav::keys(cx, window, "tab enter");
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some(URL)
                );
                let copy = e2e::control_bounds(window.window_id(), "inspector-message-copy-link-0")
                    .unwrap();
                cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
                cx.simulate_mouse_move(window, copy.center(), None, gpui::Modifiers::none());
                cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some(URL)
                );
                cx.write_to_clipboard(ClipboardItem::new_string("poison".into()));
                assert!(crate::native_ax::press(cx, window, &copy_name));
                cx.run_until_parked();
                assert_eq!(
                    cx.read_from_clipboard()
                        .and_then(|item| item.text())
                        .as_deref(),
                    Some(URL)
                );
                let http_name = format!("{}: {HTTP_URL}", i18n::Msg::InspectorOpenLink.t());
                crate::keyboard_nav::keys(cx, window, "tab");
                let http_node =
                    crate::native_ax::node(cx, window, "inspector-message-link-1".into());
                assert_eq!(http_node["aria"]["role"], "Link");
                assert_eq!(http_node["aria"]["label"], http_name);
                assert!(crate::native_ax::press(cx, window, &http_name));
                cx.run_until_parked();
                assert_eq!(e2e::take_opened_urls(), vec![HTTP_URL]);
                assert!(
                    e2e::control_bounds(window.window_id(), "inspector-message-link-2").is_none(),
                    "arbitrary schemes never become links"
                );
                unmount(cx, app, window);
            }
        }
    }
    assert_eq!(repo_fingerprint(fixture.path()), before);
}
