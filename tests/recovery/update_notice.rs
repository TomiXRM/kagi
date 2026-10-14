//! #1514: pre-install decisions cannot change an in-flight installer.
use crate::{
    evidence_support::deferred,
    macos::{build_fixture, mount_state, unmount},
};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{button_style, e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    settings, theme,
};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    draw(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), id).expect("rendered update action");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}
fn unchanged(cx: &VisualTestAppContext, app: &Entity<KagiApp>, skipped: &Option<String>) {
    assert_eq!(&settings::read_setting("update_skipped"), skipped);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.update_installing);
        assert_eq!(app.update_available.as_ref().unwrap().0.tag, "v1.0.1");
    });
}

pub fn scenario_update_notice_in_flight(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["update_skipped", "lang", "theme", "ui_zoom"]);
    let original = (i18n::lang(), theme::zoom(), theme::theme().slug.to_string());
    for slug in ["apple-light", "apple-dark"] {
        for language in [Lang::En, Lang::Ja] {
            for zoom in [1., 1.67] {
                let fixture = build_fixture();
                let mut state = e2e::app_state(fixture.path()).expect("build update fixture");
                state.update_checked = true;
                let (app, window) = mount_state(cx, state);
                let skipped = settings::read_setting("update_skipped");
                theme::set_zoom(zoom);
                app.update(cx, |app, cx| {
                    app.set_lang(language, cx);
                    app.set_theme(slug, cx);
                    app.update_available = Some(crate::recovery_operations::fake_update_offer());
                    app.open_update_modal();
                    cx.notify();
                });
                let (task, reply) = deferred(cx);
                e2e::queue_update_install(task);
                click(cx, window, "update-now");
                assert!(
                    !e2e::update_install_queued(),
                    "Update now must consume the held installer"
                );
                unchanged(cx, &app, &skipped);
                draw(cx, window);
                let skip = button_style::recorded_modal_button("update-skip").unwrap();
                assert!(
                    skip.disabled,
                    "update in flight must disable Skip this version"
                );
                assert_eq!(
                    skip.description.as_deref(),
                    Some(Msg::UpdateInstallingReason.t())
                );
                let dismiss = button_style::recorded_modal_button("update-cancel").unwrap();
                assert_eq!(
                    dismiss.label,
                    if language == Lang::En {
                        "Hide — update continues"
                    } else {
                        "非表示 — 更新は続行します"
                    }
                );
                assert!(!dismiss.disabled);
                click(cx, window, "update-skip");
                unchanged(cx, &app, &skipped);
                // Traverse the real dispatch tree: the unavailable Skip is not
                // a keyboard activation target, including after pointer input.
                for _ in 0..6 {
                    cx.simulate_keystrokes(window, "tab");
                    cx.simulate_keystrokes(window, "f19");
                    assert_ne!(
                        button_style::modal_focus_probe(),
                        Some("update-skip"),
                        "in-flight Skip must not acquire keyboard focus"
                    );
                }
                for key in ["enter", "space"] {
                    crate::recovery_operations::press_key(cx, &app, window, key);
                    cx.run_until_parked();
                    unchanged(cx, &app, &skipped);
                }
                click(cx, window, "update-cancel");
                assert!(cx.read(|cx| app.read(cx).update_modal().is_none()));
                unchanged(cx, &app, &skipped);
                app.update(cx, |app, cx| {
                    app.open_update_modal();
                    cx.notify();
                });
                draw(cx, window);
                assert!(e2e::control_bounds(window.window_id(), "update/status").is_some());
                assert!(
                    button_style::recorded_modal_button("update-now")
                        .unwrap()
                        .disabled
                );
                let (duplicate, duplicate_reply) = deferred(cx);
                e2e::queue_update_install(duplicate);
                click(cx, window, "update-now");
                assert!(
                    e2e::update_install_queued(),
                    "reopening must not consume a second installer"
                );
                e2e::discard_queued_update_install();
                duplicate_reply.send(Err("unused duplicate".into()));
                click(cx, window, "update-cancel");
                reply.send(Err("held download failed".into()));
                cx.run_until_parked();
                app.update(cx, |app, cx| {
                    app.open_update_modal();
                    cx.notify();
                });
                draw(cx, window);
                assert!(
                    !button_style::recorded_modal_button("update-skip")
                        .unwrap()
                        .disabled
                );
                assert_eq!(
                    button_style::recorded_modal_button("update-cancel")
                        .unwrap()
                        .label,
                    Msg::UpdateLater.t()
                );
                cx.read(|cx| {
                    let app = app.read(cx);
                    assert!(!app.update_installing);
                    assert!(app.update_available.is_some());
                    assert!(app
                        .update_status
                        .as_deref()
                        .unwrap()
                        .contains("held download failed"));
                });
                assert_eq!(settings::read_setting("update_skipped"), skipped);
                unmount(cx, app, window);
            }
        }
    }
    i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    theme::set_active(&original.2);
    cx.update(theme::sync_gpui_component_theme);
    eprintln!("[gui-e2e] PASS update_notice_in_flight");
}
