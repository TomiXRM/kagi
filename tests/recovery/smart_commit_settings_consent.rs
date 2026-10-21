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

const PROVIDERS: [smart_commit::SmartProvider; 3] = [
    smart_commit::SmartProvider::Ollama,
    smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode),
    smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::Codex),
];

/// Fail closed before reading stdin: no test can reach a real external CLI.
struct NoSendCli {
    directory: tempfile::TempDir,
    previous_path: Option<std::ffi::OsString>,
}

impl NoSendCli {
    fn install() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        for binary in ["claude", "codex"] {
            let path = directory.path().join(binary);
            std::fs::write(&path, "#!/bin/sh\n: > \"$0.invoked\"\nexit 97\n").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let previous_path = std::env::var_os("PATH");
        let mut paths = vec![directory.path().to_path_buf()];
        paths.extend(std::env::split_paths(
            &previous_path.clone().unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self {
            directory,
            previous_path,
        }
    }

    fn assert_unused(&self) {
        for binary in ["claude", "codex"] {
            assert!(
                !self
                    .directory
                    .path()
                    .join(format!("{binary}.invoked"))
                    .exists(),
                "consent must not invoke {binary} or transmit staged code"
            );
        }
    }
}

impl Drop for NoSendCli {
    fn drop(&mut self) {
        match self.previous_path.take() {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    crate::presentation_support::paint(cx, window);
    cx.run_until_parked();
}

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..4 {
        // The pinned Switch retains its keyed state until its 150ms thumb
        // animation timer settles; finish it before closing/remounting hosts.
        cx.advance_clock(std::time::Duration::from_millis(200));
        drain_native_events();
        draw(cx, window);
    }
}

fn settings_body(window: AnyWindowHandle) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), "settings-window-scrollbar")
        .or_else(|| e2e::control_bounds(window.window_id(), "settings-scrollbar"))
        .expect("the actual Settings host's scroll viewport")
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    draw(cx, window);
    let mut bounds = e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("production control {id} is drawn"));
    let viewport = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    // Settings is a scrolling form. Reveal pointer targets with real wheel
    // input instead of relying on a fixture-written offset surviving Tab.
    if id == "smart-commit-enabled-key" || id.starts_with("sc-") {
        let body = settings_body(window);
        for _ in 0..20 {
            if bounds.top() >= body.top() && bounds.bottom() <= body.bottom() {
                break;
            }
            cx.simulate_event(
                window,
                gpui::ScrollWheelEvent {
                    position: body.center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(
                        gpui::px(0.),
                        gpui::px(if bounds.top() < body.top() {
                            300.
                        } else {
                            -300.
                        }),
                    )),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
            settle(cx, window);
            bounds = e2e::control_bounds(window.window_id(), id).unwrap();
        }
        crate::recovery_layout::contained(
            body,
            bounds,
            "Settings pointer target is inside its scroll body",
        );
    }
    assert!(
        bounds.top() >= gpui::px(0.) && bounds.bottom() <= viewport.height,
        "{id} is reachable in its actual host viewport: {bounds:?}"
    );
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
    settle(cx, window);
}

fn fresh_settings() {
    for key in [ENABLED, CONSENT, MODEL] {
        settings::write_setting(key, None);
    }
    settings::write_setting("smart_commit_provider", Some("ollama"));
    settings::write_setting("settings_pane", Some("2"));
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
    // The native host restores pane 2; the portable host may begin at General.
    if e2e::control_bounds(host.window_id(), "settings-pane-2").is_some() {
        click(cx, host, "settings-pane-2");
    }
    assert!(e2e::control_bounds(host.window_id(), "settings-smart-heading").is_some());
    host
}

fn switch_on(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    host: AnyWindowHandle,
    space: bool,
) {
    if space {
        // Traverse production Tab stops: no direct callback or test focus setter.
        let mut reached = false;
        for _ in 0..80 {
            keys(cx, host, "tab");
            draw(cx, host);
            reached = cx
                .update_window(host, |_, window, cx| {
                    app.read(cx).settings_switch_focused_for_e2e(window) == Some(5)
                })
                .unwrap();
            if reached {
                break;
            }
        }
        assert!(reached, "real Tab reaches the Smart Commit switch");
        settle(cx, host);
        crate::recovery_layout::contained(
            settings_body(host),
            e2e::control_bounds(host.window_id(), "smart-commit-enabled-key").unwrap(),
            "real Tab reveals the focused Smart switch before consent activation",
        );
        keys(cx, host, "space");
        settle(cx, host);
    } else {
        click(cx, host, "smart-commit-enabled-key");
    }
}

fn assert_switch(cx: &mut VisualTestAppContext, host: AnyWindowHandle, expected: bool) {
    draw(cx, host);
    let (label, checked) =
        e2e::recorded_switch("smart-commit-enabled").expect("production Switch AX input");
    assert_eq!(label, i18n::Msg::SettingsSmartEnable.t());
    assert_eq!(
        checked, expected,
        "switch remains OFF until explicit consent"
    );
}

fn assert_no_post(server: &Ollama) {
    assert!(
        server.generated_models().is_empty(),
        "consent/settings must not POST /api/generate"
    );
}

fn assert_provider_lines(
    cx: &mut VisualTestAppContext,
    host: AnyWindowHandle,
    provider: smart_commit::SmartProvider,
) {
    draw(cx, host);
    let viewport = cx
        .update_window(host, |_, window, _| window.viewport_size())
        .unwrap();
    // IDs are formed from the same localized text passed to each rendered child.
    // A translation-only assertion cannot satisfy this laid-out renderer oracle.
    use i18n::Msg;
    let (intro, destination) = match provider {
        smart_commit::SmartProvider::Ollama => (Msg::SmartConsentIntro, Msg::SmartConsentLocalhost),
        smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode) => {
            (Msg::SmartConsentClaudeIntro, Msg::SmartConsentCliAccount)
        }
        smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::Codex) => {
            (Msg::SmartConsentCodexIntro, Msg::SmartConsentCliAccount)
        }
    };
    let intro = intro.t();
    let service = match provider {
        smart_commit::SmartProvider::Ollama => "localhost Ollama",
        smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode) => {
            "Anthropic"
        }
        smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::Codex) => "OpenAI",
    };
    assert!(
        intro.contains(service),
        "disclosure names the actual destination"
    );
    assert!(
        e2e::control_bounds(host.window_id(), &format!("smart-consent-intro-{intro}")).is_some(),
        "the actual provider-specific destination is rendered: {intro}"
    );
    let lines = [
        Msg::SmartConsentStagedOnly,
        Msg::SmartConsentUnstagedExcluded,
        destination,
        Msg::SmartConsentSecretsWarning,
    ];
    for (index, line) in lines.into_iter().enumerate() {
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

fn assert_lines(cx: &mut VisualTestAppContext, host: AnyWindowHandle) {
    assert_provider_lines(cx, host, smart_commit::SmartProvider::Ollama);
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
    assert_eq!(
        cx.update_window(host, |_, window, cx| {
            app.read(cx).settings_switch_focused_for_e2e(window)
        })
        .unwrap(),
        Some(5),
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

    switch_on(cx, &app, host, space);
    assert_pending(cx, &app, host, &server);
    click(cx, host, "smart-consent-confirm");
    assert_eq!(settings::read_setting(ENABLED).as_deref(), Some("1"));
    assert_eq!(settings::read_setting(CONSENT).as_deref(), Some("ollama"));
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
    assert_eq!(
        cx.update_window(host, |_, window, cx| app
            .read(cx)
            .settings_switch_focused_for_e2e(window))
            .unwrap(),
        Some(5),
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
    click(cx, host, "smart-commit-enabled-key");
    assert_switch(cx, host, false);
    assert_eq!(settings::read_setting(ENABLED).as_deref(), Some("0"));
    assert_eq!(
        settings::read_setting(CONSENT).as_deref(),
        Some("ollama"),
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
            Some(smart_commit::SmartCommitModal::Consent { .. })
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
    assert_eq!(settings::read_setting(CONSENT).as_deref(), Some("ollama"));
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

fn provider_switch_leg(cx: &mut VisualTestAppContext, native: bool) {
    fresh_settings();
    let cli = NoSendCli::install();
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    let (mut app, mut root) = mount(cx, fixture.path());
    for provider in PROVIDERS.into_iter().chain([PROVIDERS[0]]) {
        let mut host = open_settings(cx, root, native);
        if cx.read(|cx| app.read(cx).smart_commit.provider) != provider {
            click(cx, host, &format!("sc-provider-{}", provider.slug()));
        } else {
            switch_on(cx, &app, host, false);
        }
        assert_switch(cx, host, false);
        assert_provider_lines(cx, host, provider);
        assert_eq!(
            settings::read_setting(CONSENT),
            None,
            "switch revokes approval"
        );
        assert!(!cx.read(|cx| app.read(cx).smart_commit.has_consent()));
        assert_no_post(&server);
        cli.assert_unused();
        // Closing/reopening the actual Settings host cannot retain approval.
        close_settings(cx, &app, root, host, native);
        host = open_settings(cx, root, native);
        switch_on(cx, &app, host, false);
        assert_provider_lines(cx, host, provider);
        click(cx, host, "smart-consent-confirm");
        assert_eq!(
            settings::read_setting(CONSENT).as_deref(),
            Some(provider.slug())
        );
        assert!(cx.read(|cx| app.read(cx).smart_commit.has_consent()));
        assert_switch(cx, host, true);
        assert_no_post(&server);
        cli.assert_unused();
        close_settings(cx, &app, root, host, native);
        unmount(cx, app, root);
        (app, root) = mount(cx, fixture.path());
        assert!(
            cx.read(|cx| app.read(cx).smart_commit.has_consent()),
            "restart accepts only the persisted selected provider"
        );
    }
    // A Settings alert cannot approve a different provider, or an away/back revision.
    let host = open_settings(cx, root, native);
    click(cx, host, &format!("sc-provider-{}", PROVIDERS[1].slug()));
    assert_provider_lines(cx, host, PROVIDERS[1]);
    app.update(cx, |app, _| {
        app.smart_commit.set_provider(PROVIDERS[2]);
        app.smart_commit.set_provider(PROVIDERS[1]);
    });
    click(cx, host, "smart-consent-confirm");
    assert!(!cx.read(|cx| app.read(cx).smart_commit.has_consent()));
    assert_switch(cx, host, false);
    assert_eq!(settings::read_setting(CONSENT), None);
    assert_no_post(&server);
    cli.assert_unused();
    close_settings(cx, &app, root, host, native);
    unmount(cx, app, root);
}

fn provider_generate_leg(cx: &mut VisualTestAppContext) {
    fresh_settings();
    let cli = NoSendCli::install();
    let server = Ollama::new(&[MODEL_NAME.to_string()]);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("consent.txt"),
        "staged consent fixture\n",
    )
    .unwrap();
    git(fixture.path(), &["add", "consent.txt"]);
    settings::write_setting(MODEL, Some(MODEL_NAME));
    for (index, provider) in PROVIDERS.into_iter().enumerate() {
        settings::write_setting("smart_commit_provider", Some(provider.slug()));
        settings::write_setting(CONSENT, Some("1"));
        let (app, root) = mount(cx, fixture.path());
        assert!(
            !cx.read(|cx| app.read(cx).smart_commit.has_consent()),
            "legacy unbound approval must be reacquired for every provider"
        );
        open_panel(cx, &app, root);
        let request = |cx: &mut VisualTestAppContext| {
            cx.update_window(root, |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.smart_generate(app.active_session().unwrap(), window, cx);
                });
            })
            .unwrap();
            settle(cx, root);
        };
        request(cx);
        assert_provider_lines(cx, root, provider);
        let next = PROVIDERS[(index + 1) % PROVIDERS.len()];
        app.update(cx, |app, _| app.smart_commit.set_provider(next));
        click(cx, root, "smart-consent-confirm");
        assert_eq!(
            settings::read_setting(CONSENT),
            None,
            "stale Generate approval rejected"
        );
        assert!(!cx.read(|cx| app.read(cx).smart_commit.has_consent()));
        request(cx);
        assert_provider_lines(cx, root, next);
        click(cx, root, "smart-consent-cancel");
        app.update(cx, |app, _| app.smart_commit.set_provider(provider));
        request(cx);
        click(cx, root, "smart-consent-confirm");
        assert_eq!(
            settings::read_setting(CONSENT).as_deref(),
            Some(provider.slug())
        );
        assert!(
            cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
            "CLI consent must not open Ollama's model picker"
        );
        assert_no_post(&server);
        cli.assert_unused();
        unmount(cx, app, root);
        // Simulate a persisted provider change outside the live Settings view.
        settings::write_setting("smart_commit_provider", Some(next.slug()));
        let (app, root) = mount(cx, fixture.path());
        assert!(
            !cx.read(|cx| app.read(cx).smart_commit.has_consent()),
            "persisted approval for another provider cannot authorize generation"
        );
        open_panel(cx, &app, root);
        cx.update_window(root, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.smart_generate(app.active_session().unwrap(), window, cx);
            });
        })
        .unwrap();
        settle(cx, root);
        assert_provider_lines(cx, root, next);
        click(cx, root, "smart-consent-cancel");
        assert_no_post(&server);
        cli.assert_unused();
        unmount(cx, app, root);
    }
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
            provider_switch_leg(cx, native);
        }
        generate_leg(cx, false);
        generate_leg(cx, true);
        native_preserves_plan(cx);
        provider_generate_leg(cx);
    }
    eprintln!("[gui-e2e] PASS smart_commit_settings_consent");
}
