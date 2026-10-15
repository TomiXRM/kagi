//! #1605: native URL delivery shares the repository opener and survives root closure.
use crate::macos::{build_fixture, drain_native_events, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{dispatch_open_urls_for_e2e, settings, FooterStatus};
use std::path::Path;

fn file_url(path: &Path) -> String {
    let encoded = percent_encoding::utf8_percent_encode(
        path.to_str().unwrap(),
        percent_encoding::NON_ALPHANUMERIC,
    )
    .to_string()
    .replace("%2F", "/");
    format!("file://{encoded}")
}

pub fn scenario_folder_urls_existing_root(cx: &mut VisualTestAppContext) {
    let _shell = crate::gui_isolation::StandInShell::install();
    let _ports = crate::gui_isolation::PortStore::keep();
    let initial = build_fixture();
    let target = build_fixture();
    let target_path = target.path().canonicalize().unwrap();
    crate::gui_evidence::fixture(&target_path);
    let aliases = tempfile::tempdir().unwrap();
    let alias = aliases.path().join("Repo 日本 #%+");
    std::os::unix::fs::symlink(target.path(), &alias).unwrap();
    let (app, window) = mount(cx, initial.path());
    let delivered = cx
        .update(|cx| dispatch_open_urls_for_e2e(vec![file_url(&alias)], cx))
        .expect("URL delivered to repository root");
    assert_eq!(delivered, app);
    drop(delivered);
    cx.run_until_parked();
    app.read_with(cx, |state, _| {
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(state.repo_path.as_deref(), Some(target_path.as_path()));
    });
    cx.update(|cx| {
        dispatch_open_urls_for_e2e(
            vec![
                file_url(target.path()),
                "https://example.com/ignored".into(),
                "file://remote.invalid/tmp/ignored".into(),
            ],
            cx,
        );
    });
    assert_eq!(app.read_with(cx, |state, _| state.tabs.len()), 2);
    // Files must not discover/open their containing repository.
    cx.update(|cx| {
        dispatch_open_urls_for_e2e(vec![file_url(&initial.path().join("README.md"))], cx);
    });
    app.read_with(cx, |state, _| {
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(state.repo_path.as_deref(), Some(target_path.as_path()));
        assert!(matches!(state.status_footer, FooterStatus::Failed(_)));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS folder_urls_existing_root");
}

pub fn scenario_folder_urls_without_root(cx: &mut VisualTestAppContext) {
    let _shell = crate::gui_isolation::StandInShell::install();
    let _ports = crate::gui_isolation::PortStore::keep();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "session_repos",
        "session_active",
        "window_frame",
        "window_size",
    ]);
    settings::write_setting("session_repos", None);
    settings::write_setting("session_active", None);
    let fixture = build_fixture();
    let fixture_path = fixture.path().canonicalize().unwrap();
    crate::gui_evidence::fixture(&fixture_path);
    assert!(cx.read(|cx| cx.windows().is_empty()));
    // A batch queued before any repository window exists must create a host.
    // Closing it must not disable the application-owned native request route.
    for _ in 0..2 {
        let app = cx
            .update(|cx| dispatch_open_urls_for_e2e(vec![file_url(fixture.path())], cx))
            .expect("URL must recreate repository host");
        cx.run_until_parked();
        app.read_with(cx, |state, _| {
            assert_eq!(state.tabs.len(), 1);
            assert_eq!(state.repo_path.as_deref(), Some(fixture_path.as_path()));
        });
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(windows.len(), 1, "one native repository host per delivery");
        unmount(cx, app, windows[0]);
        drain_native_events();
        assert!(cx.read(|cx| cx.windows().is_empty()));
    }
    eprintln!("[gui-e2e] PASS folder_urls_without_root");
}

pub fn scenario_folder_urls_settings_only(cx: &mut VisualTestAppContext) {
    let _shell = crate::gui_isolation::StandInShell::install();
    let _ports = crate::gui_isolation::PortStore::keep();
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "session_repos",
        "session_active",
        "window_frame",
        "window_size",
        "settings_pane",
        "settings_window_position",
        "settings_window_height",
        "settings_window_frame",
    ]);
    let initial = build_fixture();
    let target = build_fixture();
    let initial_path = initial.path().canonicalize().unwrap();
    let target_path = target.path().canonicalize().unwrap();
    crate::gui_evidence::fixture(&target_path);
    let (old_app, old_window) = mount(cx, initial.path());
    cx.simulate_keystrokes(old_window, "cmd-,");
    cx.run_until_parked();
    let settings_window = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|window| window.window_id() != old_window.window_id())
            .expect("native Settings window")
    });
    cx.update_window(old_window, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.windows().len()), 1);
    settings::write_setting("session_repos", None);
    settings::write_setting("session_active", None);
    let app = cx
        .update(|cx| dispatch_open_urls_for_e2e(vec![file_url(target.path())], cx))
        .expect("Settings is not a repository host");
    cx.run_until_parked();
    assert_ne!(app, old_app);
    assert_eq!(app.read_with(cx, |state, _| state.tabs.len()), 1);
    assert_eq!(
        app.read_with(cx, |state, _| state.repo_path.clone())
            .as_deref(),
        Some(target_path.as_path())
    );
    assert_eq!(
        old_app
            .read_with(cx, |state, _| state
                .repo_path
                .as_ref()
                .map(|path| path.canonicalize().unwrap()))
            .as_deref(),
        Some(initial_path.as_path()),
        "utility owner must not consume the new repository"
    );
    let reopened = cx.read(|cx| {
        let windows = cx.windows();
        assert_eq!(windows.len(), 2);
        windows
            .into_iter()
            .find(|window| window.window_id() != settings_window.window_id())
            .unwrap()
    });
    cx.update_window(settings_window, |_, window, _| window.remove_window())
        .unwrap();
    drop(old_app);
    cx.update(|_| {});
    cx.run_until_parked();
    drain_native_events();
    unmount(cx, app, reopened);
    eprintln!("[gui-e2e] PASS folder_urls_settings_only");
}
