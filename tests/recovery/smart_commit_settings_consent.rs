//! #1509: initial Settings enable must obtain the same privacy consent as Generate.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, drain_native_events, git, mount, mount_state, unmount};
use crate::settings_window::SettingsHostGuard;
use crate::smart_picker::Ollama;
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n, settings, smart_commit, theme, KagiApp};

const ENABLED: &str = "smart_commit_llm_enabled";
const CONSENT: &str = "smart_commit_consent_accepted";
const MODEL: &str = "smart_commit_model";
const MODEL_NAME: &str = "consent-fixture-model";

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "settings-smart-consent-alert");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..4 {
        // Settle host presentation before closing or remounting its window.
        cx.advance_clock(std::time::Duration::from_millis(200));
        drain_native_events();
        draw(cx, window);
    }
}

fn settings_body(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> gpui::Bounds<gpui::Pixels> {
    crate::settings_controls::bounds(cx, window, "settings-detail-scroll")
        .expect("the production Settings detail viewport")
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    crate::settings_controls::click(cx, window, id);
    cx.run_until_parked();
    settle(cx, window);
}

fn fresh_settings() {
    for key in [ENABLED, CONSENT, MODEL] {
        settings::write_setting(key, None);
    }
    settings::write_setting("smart_commit_provider", Some("ollama"));
    settings::write_setting("settings_pane", Some("smart_commit"));
}

fn open_settings(
    cx: &mut VisualTestAppContext,
    root: AnyWindowHandle,
    native: bool,
) -> AnyWindowHandle {
    draw(cx, root);
    keys(cx, root, "cmd-,");
    cx.run_until_parked();
    let host = if native {
        cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|window| window.window_id() != root.window_id())
                .expect("production native Settings host")
        })
    } else {
        root
    };
    settle(cx, host);
    click(cx, host, "settings-pane-smart_commit");
    assert!(crate::settings_controls::probe(cx, host, "sc-enabled-toggle").is_some());
    host
}

fn switch_on(
    cx: &mut VisualTestAppContext,
    _app: &Entity<KagiApp>,
    host: AnyWindowHandle,
    space: bool,
) {
    if space {
        // Traverse production Tab stops: no direct callback or test focus setter.
        let mut reached = false;
        for _ in 0..80 {
            keys(cx, host, "tab");
            draw(cx, host);
            reached = crate::settings_controls::probe(cx, host, "sc-enabled-toggle")
                .is_some_and(|probe| probe.focused);
            if reached {
                break;
            }
        }
        assert!(reached, "real Tab reaches the Smart Commit switch");
        settle(cx, host);
        crate::recovery_layout::contained(
            settings_body(cx, host),
            crate::settings_controls::bounds(cx, host, "sc-enabled-toggle").unwrap(),
            "real Tab reveals the focused Smart switch before consent activation",
        );
        keys(cx, host, "space");
        settle(cx, host);
    } else {
        click(cx, host, "sc-enabled-toggle");
    }
}

fn assert_switch(cx: &mut VisualTestAppContext, host: AnyWindowHandle, expected: bool) {
    draw(cx, host);
    let control = crate::settings_controls::probe(cx, host, "sc-enabled-toggle")
        .expect("production switch AX input");
    assert_eq!(control.role, gpui::Role::Switch);
    assert_eq!(control.name, i18n::Msg::SettingsSmartEnable.t());
    assert_eq!(
        control.selected, expected,
        "switch remains OFF until explicit consent"
    );
}

fn assert_no_post(server: &Ollama) {
    assert!(
        server.generated_models().is_empty(),
        "consent/settings must not POST /api/generate"
    );
}

fn assert_lines(cx: &mut VisualTestAppContext, host: AnyWindowHandle) {
    draw(cx, host);
    let viewport = cx
        .update_window(host, |_, window, _| window.viewport_size())
        .unwrap();
    // IDs are formed from the same localized text passed to each rendered child.
    // A translation-only assertion cannot satisfy this laid-out renderer oracle.
    for (index, line) in smart_commit::CONSENT_LINES.into_iter().enumerate() {
        let id = format!("smart-consent-line-{index}-{}", line.t());
        let bounds = e2e::control_bounds(host.window_id(), &id)
            .unwrap_or_else(|| panic!("localized consent line {index} is rendered: {}", line.t()));
        assert!(bounds.size.height > gpui::px(0.) && bounds.size.width > gpui::px(0.));
        assert!(
            bounds.left() >= gpui::px(0.)
                && bounds.right() <= viewport.width
                && bounds.top() >= gpui::px(0.)
                && bounds.bottom() <= viewport.height,
            "all four privacy statements fit the actual Settings/Generate host: {bounds:?}"
        );
    }
    for id in ["smart-consent-cancel", "smart-consent-confirm"] {
        assert!(
            e2e::control_bounds(host.window_id(), id).is_some(),
            "{id} rendered"
        );
    }
}

fn assert_pending(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    host: AnyWindowHandle,
    server: &Ollama,
) {
    assert_eq!(
        settings::read_setting(ENABLED),
        None,
        "initial Settings ON must not persist enabled before Confirm"
    );
    assert_eq!(settings::read_setting(CONSENT), None);
    assert_eq!(settings::read_setting(MODEL), None);
    assert!(!cx.read(|cx| app.read(cx).smart_commit.llm_enabled));
    assert!(
        cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
        "Settings consent is host-local, not Generate's modal"
    );
    assert!(
        e2e::control_bounds(host.window_id(), "settings-smart-consent-alert").is_some(),
        "consent is drawn in the existing Settings host"
    );
    assert_switch(cx, host, false);
    assert_lines(cx, host);
    assert_no_post(server);
}

fn assert_declined(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    host: AnyWindowHandle,
    server: &Ollama,
) {
    assert_eq!(settings::read_setting(ENABLED), None);
    assert_eq!(settings::read_setting(CONSENT), None);
    assert_eq!(settings::read_setting(MODEL), None);
    assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
    assert_switch(cx, host, false);
    assert_no_post(server);
    assert!(
        crate::settings_controls::probe(cx, host, "sc-enabled-toggle").unwrap().focused,
        "decline returns focus to the owning Settings switch"
    );
}

fn close_settings(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    root: AnyWindowHandle,
    host: AnyWindowHandle,
    native: bool,
) {
    if native {
        // Cmd-W removes the host on key-down; there is no window for key-up.
        cx.simulate_keystrokes(host, "cmd-w");
        cx.run_until_parked();
        drain_native_events();
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            1,
            "Settings close leaves its owner alive"
        );
    } else {
        // Close the existing portable Settings host through its production backdrop.
        cx.simulate_click(
            root,
            gpui::point(gpui::px(2.), gpui::px(100.)),
            Modifiers::none(),
        );
        cx.run_until_parked();
        draw(cx, root);
        assert!(
            cx.read(|cx| app.read(cx).menu_overlay.is_none()),
            "portable host closed"
        );
    }
}

fn open_panel(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, root: AnyWindowHandle) {
    cx.update_window(root, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    settle(cx, root);
}

fn settings_leg(cx: &mut VisualTestAppContext, native: bool, home: bool, space: bool) {
    fresh_settings();
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("consent.txt"),
        "staged consent fixture\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "consent.txt"]);
    let (app, root) = if home {
        mount_state(cx, KagiApp::with_error(""))
    } else {
        mount(cx, fixture.path())
    };
    assert_eq!(cx.read(|cx| app.read(cx).tabs.is_empty()), home);
    let mut host = open_settings(cx, root, native);
    let retained = cx.read(|cx| app.read(cx).settings_entity().unwrap().entity_id());
    assert_switch(cx, host, false);

    switch_on(cx, &app, host, space);
    assert_pending(cx, &app, host, &server);
    click(cx, host, "smart-consent-cancel");
    assert_declined(cx, &app, host, &server);

    for key in ["escape", "cmd-."] {
        switch_on(cx, &app, host, space);
        assert_pending(cx, &app, host, &server);
        keys(cx, host, key);
        settle(cx, host);
        assert_declined(cx, &app, host, &server);
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            if native { 2 } else { 1 },
            "{key} cancels consent, not native Settings"
        );
    }

    switch_on(cx, &app, host, space);
    assert_pending(cx, &app, host, &server);
    close_settings(cx, &app, root, host, native);
    assert_eq!(settings::read_setting(ENABLED), None);
    assert_eq!(settings::read_setting(CONSENT), None);
    assert!(
        cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
        "closing host cancels its consent slot"
    );
    assert_no_post(&server);
    host = open_settings(cx, root, native);
    assert_switch(cx, host, false);
    assert_eq!(cx.read(|cx| app.read(cx).settings_entity().unwrap().entity_id()), retained);
    assert!(
        e2e::control_bounds(host.window_id(), "settings-smart-consent-alert").is_none(),
        "reopening retains the view but never resurrects the closed host's consent"
    );

    switch_on(cx, &app, host, space);
    assert_pending(cx, &app, host, &server);
    click(cx, host, "smart-consent-confirm");
    assert_eq!(settings::read_setting(ENABLED).as_deref(), Some("1"));
    assert_eq!(settings::read_setting(CONSENT).as_deref(), Some("1"));
    assert_eq!(
        settings::read_setting(MODEL),
        None,
        "Confirm cannot auto-select even a single installed model"
    );
    assert!(
        cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
        "Settings consent never opens Generate's model picker"
    );
    assert_switch(cx, host, true);
    assert_no_post(&server);
    assert!(
        crate::settings_controls::probe(cx, host, "sc-enabled-toggle").unwrap().focused,
        "Confirm restores Settings focus"
    );

    // Settings model selection is explicit and still does not send a diff.
    click(cx, host, &format!("sc-model-{MODEL_NAME}"));
    assert_eq!(settings::read_setting(MODEL).as_deref(), Some(MODEL_NAME));
    assert_no_post(&server);
    close_settings(cx, &app, root, host, native);
    if home {
        app.update(cx, |app, cx| {
            assert!(app.open_repository(fixture.path().canonicalize().unwrap(), cx));
        });
        cx.run_until_parked();
    }
    open_panel(cx, &app, root);
    click(cx, root, "cp-smart-suggest");
    assert_eq!(
        server.generated_models(),
        vec![MODEL_NAME.to_string()],
        "only the later explicit Generate POSTs once"
    );
    unmount(cx, app, root);

    // Recreate the owner from persisted settings (the production restart read).
    let (app, root) = mount(cx, fixture.path());
    assert!(cx.read(|cx| app.read(cx).smart_commit.llm_enabled));
    let host = open_settings(cx, root, native);
    assert_switch(cx, host, true);
    click(cx, host, "sc-enabled-toggle");
    assert_switch(cx, host, false);
    assert_eq!(settings::read_setting(ENABLED).as_deref(), Some("0"));
    assert_eq!(
        settings::read_setting(CONSENT).as_deref(),
        Some("1"),
        "OFF does not revoke completed consent"
    );
    switch_on(cx, &app, host, space);
    assert_switch(cx, host, true);
    assert!(
        cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
        "accepted re-enable skips first-use consent"
    );
    assert_eq!(
        server.generated_models().len(),
        1,
        "restart/OFF/re-enable sends nothing"
    );
    close_settings(cx, &app, root, host, native);
    unmount(cx, app, root);
}

fn owner_close_leg(cx: &mut VisualTestAppContext, native: bool, home: bool) {
    fresh_settings();
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    let (app, root) = if home {
        mount_state(cx, KagiApp::with_error(""))
    } else {
        mount(cx, fixture.path())
    };
    let host = open_settings(cx, root, native);
    switch_on(cx, &app, host, false);
    assert_pending(cx, &app, host, &server);
    let weak = app.downgrade();
    unmount(cx, app, root);
    assert!(
        weak.upgrade().is_none(),
        "consent does not retain a closed owner"
    );
    assert!(
        cx.read(|cx| cx.windows().is_empty()),
        "native Settings closes with its owner"
    );
    assert_eq!(settings::read_setting(ENABLED), None);
    assert_eq!(settings::read_setting(CONSENT), None);
    assert_no_post(&server);
}

fn generate_leg(cx: &mut VisualTestAppContext, legacy_enabled: bool) {
    fresh_settings();
    if legacy_enabled {
        settings::write_setting(ENABLED, Some("1"));
        settings::write_setting(MODEL, Some(MODEL_NAME));
    }
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("consent.txt"),
        "staged consent fixture\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "consent.txt"]);
    let (app, root) = mount(cx, fixture.path());
    open_panel(cx, &app, root);
    let request = |cx: &mut VisualTestAppContext| {
        cx.update_window(root, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(app.active_session().unwrap(), window, cx)
            });
        })
        .unwrap();
        settle(cx, root);
    };
    // Preserve Generate's existing direct entry; legacy enabled settings must
    // now ask too, rather than treating the old enabled bit as consent.
    for escape in [false, true] {
        request(cx);
        assert!(cx.read(|cx| matches!(
            app.read(cx).smart_commit_modal(),
            Some(smart_commit::SmartCommitModal::Consent)
        )));
        assert_lines(cx, root);
        assert_no_post(&server);
        if escape {
            keys(cx, root, "escape");
            settle(cx, root);
        } else {
            click(cx, root, "smart-consent-cancel");
        }
        assert_eq!(settings::read_setting(CONSENT), None);
        assert_eq!(
            settings::read_setting(ENABLED).as_deref(),
            legacy_enabled.then_some("1")
        );
        assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
        assert_no_post(&server);
    }
    request(cx);
    click(cx, root, "smart-consent-confirm");
    assert_eq!(settings::read_setting(CONSENT).as_deref(), Some("1"));
    assert_eq!(settings::read_setting(ENABLED).as_deref(), Some("1"));
    assert_no_post(&server);
    if !legacy_enabled {
        assert_eq!(settings::read_setting(MODEL), None);
        assert!(cx.read(|cx| matches!(
            app.read(cx).smart_commit_modal(),
            Some(smart_commit::SmartCommitModal::ModelPicker { .. })
        )));
        keys(cx, root, "escape");
        settle(cx, root);
    } else {
        assert!(cx.read(|cx| app.read(cx).smart_commit_modal().is_none()));
        click(cx, root, "cp-smart-suggest");
        assert_eq!(server.generated_models(), vec![MODEL_NAME.to_string()]);
    }
    unmount(cx, app, root);
}

fn native_preserves_plan(cx: &mut VisualTestAppContext) {
    let _host = SettingsHostGuard::native(cx);
    fresh_settings();
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "consent-plan"]);
    let (app, root) = mount(cx, fixture.path());
    let host = open_settings(cx, root, true);
    app.update(cx, |app, cx| app.open_plan_modal("consent-plan", cx));
    cx.run_until_parked();
    let plan = cx.read(|cx| {
        app.read(cx)
            .plan_modal()
            .expect("main checkout plan")
            .plan
            .clone()
    });
    for confirm in [false, true] {
        switch_on(cx, &app, host, false);
        assert_pending(cx, &app, host, &server);
        assert!(
            cx.read(|cx| std::sync::Arc::ptr_eq(&plan, &app.read(cx).plan_modal().unwrap().plan)),
            "Settings consent preserves the existing main-window plan"
        );
        if confirm {
            click(cx, host, "smart-consent-confirm");
        } else {
            keys(cx, host, "escape");
            settle(cx, host);
        }
        assert!(
            cx.read(|cx| std::sync::Arc::ptr_eq(&plan, &app.read(cx).plan_modal().unwrap().plan)),
            "Settings cancellation/acceptance never supersedes or confirms the main plan"
        );
        assert_no_post(&server);
    }
    close_settings(cx, &app, root, host, true);
    app.update(cx, |app, _| app.clear_plan_modal());
    unmount(cx, app, root);
}

pub fn scenario_smart_commit_settings_consent(cx: &mut VisualTestAppContext) {
    let _presentation = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        ENABLED,
        CONSENT,
        MODEL,
        "smart_commit_provider",
        "smart_commit_lang",
        "settings_pane",
        "settings_window_position",
        "settings_window_height",
        "settings_window_frame",
        "lang",
        "theme",
        "ui_zoom",
    ]);
    theme::set_zoom(1.);
    theme::set_active("apple-light");
    cx.update(theme::sync_gpui_component_theme);
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        for native in [true, false] {
            let _host = if native {
                SettingsHostGuard::native(cx)
            } else {
                SettingsHostGuard::portable(cx)
            };
            for home in [false, true] {
                for space in [false, true] {
                    settings_leg(cx, native, home, space);
                }
                owner_close_leg(cx, native, home);
            }
        }
        generate_leg(cx, false);
        generate_leg(cx, true);
        native_preserves_plan(cx);
    }
    eprintln!("[gui-e2e] PASS smart_commit_settings_consent");
}
