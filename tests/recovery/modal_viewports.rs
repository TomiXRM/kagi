//! Remote Browse and owner-trust viewport regressions (#1368/#1369).
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    button_style, e2e, i18n::Lang, modals::TrustRepoModal, remote_browse::e2e_transport, theme,
    KagiApp,
};

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn contained(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) {
    let tolerance = gpui::px(1.);
    assert!(
        inner.left() >= outer.left() - tolerance
            && inner.right() <= outer.right() + tolerance
            && inner.top() >= outer.top() - tolerance
            && inner.bottom() <= outer.bottom() + tolerance,
        "must fit: {inner:?} inside {outer:?}"
    );
}

fn footer(cx: &mut VisualTestAppContext, window: AnyWindowHandle, card: &str, ids: &[&str]) {
    let viewport = cx
        .update_window(window, |_, w, _| {
            gpui::Bounds::new(gpui::point(gpui::px(0.), gpui::px(0.)), w.viewport_size())
        })
        .unwrap();
    let card = e2e::control_bounds(window.window_id(), card).unwrap();
    contained(card, viewport);
    for id in ids {
        let paint = e2e::control_paint(window.window_id(), id).unwrap();
        contained(paint.bounds, card);
        contained(paint.bounds, paint.mask);
        assert!(paint.bounds.size.height >= gpui::px(20.));
    }
}

fn diagnostics(lines: usize) -> String {
    (0..lines)
        .map(|i| format!("diagnostic {i:03}: connection or permission refused\n"))
        .collect()
}

fn tail(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    body: &str,
    tail: &str,
    pinned: &str,
) {
    let before = e2e::control_bounds(window.window_id(), pinned).unwrap();
    let viewport = e2e::control_paint(window.window_id(), body).unwrap().mask;
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    draw(cx, app, window);
    let last = e2e::control_paint(window.window_id(), tail).unwrap();
    contained(last.bounds, last.mask);
    assert_eq!(
        e2e::control_bounds(window.window_id(), pinned).unwrap(),
        before,
        "scroll moves diagnostics, not actions"
    );
}

fn activate(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
) {
    for _ in 0..12 {
        button_style::modal_focus_probe();
        cx.simulate_keystrokes(window, "tab");
        cx.run_until_parked();
        draw(cx, app, window);
        cx.simulate_keystrokes(window, "f19");
        if button_style::modal_focus_probe() == Some(id) {
            crate::keyboard_nav::keys(cx, window, "space");
            cx.run_until_parked();
            return;
        }
    }
    panic!("Tab must reach {id}");
}

pub fn scenario_remote_browse_viewport(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    crate::pr_detail_adaptive::resize(cx, window, (940., 660.));
    theme::set_zoom(1.67);
    app.update(cx, |app, cx| {
        app.open_remote_browse(cx);
        app.remote_browse_mut().unwrap().error = Some(diagnostics(20).into());
    });
    draw(cx, &app, window);
    footer(
        cx,
        window,
        "remote-browse-card",
        &["remote-connect-cancel", "remote-connect-go"],
    );
    let path = format!("/home/{}/repository", "long-path-".repeat(20));
    let listing = (0..300)
        .map(|i| format!("dir-{i:03}/\n"))
        .collect::<String>();
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for lang in [Lang::En, Lang::Ja] {
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                app.update(cx, |app, cx| {
                    app.set_lang(lang, cx);
                    app.open_remote_browse(cx);
                    app.remote_browse_mut().unwrap().error = Some(diagnostics(400).into());
                });
                draw(cx, &app, window);
                footer(
                    cx,
                    window,
                    "remote-browse-card",
                    &["remote-connect-cancel", "remote-connect-go"],
                );
                tail(
                    cx,
                    &app,
                    window,
                    "remote-browse-body",
                    "remote-browse-error-tail",
                    "remote-connect-cancel",
                );
                cx.update_window(window, |_, window, cx| {
                    let state = app
                        .read(cx)
                        .remote_browse()
                        .unwrap()
                        .host_state
                        .clone()
                        .unwrap();
                    state.update(cx, |input, cx| {
                        input.set_value(format!("dev@{}", "host-".repeat(40)), window, cx);
                    });
                })
                .unwrap();
                draw(cx, &app, window);
                let outcome = e2e_transport::connected(&path, &listing, "false\n", "");
                e2e_transport::queue_remote_connect(
                    cx.background_executor.spawn(async move { outcome }),
                );
                app.update(cx, |app, cx| app.start_remote_connect(cx));
                cx.run_until_parked();
                app.update(cx, |app, _| {
                    app.remote_browse_mut().unwrap().error = Some(diagnostics(400).into())
                });
                draw(cx, &app, window);
                footer(cx, window, "remote-browse-card", &["remote-browse-close"]);
                assert!(
                    e2e::control_paint(window.window_id(), "remote-browse-path")
                        .unwrap()
                        .middle_ellipsis
                );
                crate::keyboard_nav::keys(cx, window, "end");
                crate::keyboard_nav::keys(cx, window, "pagedown");
                cx.run_until_parked();
                draw(cx, &app, window);
                cx.read(|cx| {
                    let modal = app.read(cx).remote_browse().unwrap();
                    assert_eq!(modal.cwd, path);
                    assert_eq!(modal.error.as_deref(), Some(diagnostics(400).as_str()));
                    contained(
                        e2e_transport::row_bounds(modal, 300).unwrap(),
                        e2e_transport::viewport(modal),
                    );
                });
                tail(
                    cx,
                    &app,
                    window,
                    "remote-browse-body",
                    "remote-browse-error-tail",
                    "remote-browse-close",
                );
                activate(cx, &app, window, "remote-browse-close");
                assert!(cx.read(|cx| app.read(cx).remote_browse().is_none()));
            }
        }
    }
    unmount(cx, app, window);
}

pub fn scenario_trust_prompt_viewport(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let fixture = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let repo = parent.path().join("long-path-".repeat(20));
    let cloned = std::process::Command::new("git")
        .args(["clone", "--quiet"])
        .arg(fixture.path())
        .arg(&repo)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    let repo = repo.canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    for dimensions in [(940., 660.), (600., 500.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [Lang::En, Lang::Ja] {
                theme::set_zoom(1.67);
                app.update(cx, |app, cx| {
                    app.set_lang(lang, cx);
                    app.set_trust_repo_modal(TrustRepoModal {
                        repo_path: repo.clone(),
                        error: Some(diagnostics(20)),
                    });
                });
                draw(cx, &app, window);
                footer(
                    cx,
                    window,
                    "trust-repo-card",
                    &["trust-repo-cancel", "trust-repo-confirm"],
                );
                assert!(
                    e2e::control_paint(window.window_id(), "trust-repo-path")
                        .unwrap()
                        .middle_ellipsis
                );
                tail(
                    cx,
                    &app,
                    window,
                    "trust-repo-body",
                    "trust-repo-error-tail",
                    "trust-repo-cancel",
                );
                cx.read(|cx| {
                    let modal = app.read(cx).trust_repo_modal().unwrap();
                    assert_eq!(modal.repo_path, repo);
                    assert_eq!(modal.error, Some(diagnostics(20)));
                });
                activate(cx, &app, window, "trust-repo-cancel");
                assert!(cx.read(|cx| app.read(cx).trust_repo_modal().is_none()));
                assert!(!kagi_git::trust::is_trusted(&repo), "Cancel grants nothing");
            }
        }
    }
    app.update(cx, |app, _| {
        app.set_trust_repo_modal(TrustRepoModal {
            repo_path: repo.clone(),
            error: None,
        })
    });
    draw(cx, &app, window);
    // Trust retains its existing non-destructive root Return approval policy.
    crate::keyboard_nav::keys(cx, window, "enter");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).trust_repo_modal().is_none()));
    assert!(kagi_git::trust::is_trusted(&repo));
    assert!(!kagi_git::trust::is_trusted(
        &fixture.path().canonicalize().unwrap()
    ));
    unmount(cx, app, window);
}
