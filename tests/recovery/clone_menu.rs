use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    commands::{command_state, CommandState},
    home_work::HomePane,
    i18n::Lang,
};

pub fn scenario_clone_menu_routes_home(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _gh = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\ncase \"$1 $2\" in 'repo list') echo '[{\"nameWithOwner\":\"acme/widgets\",\"url\":\"https://github.com/acme/widgets\",\"description\":\"\",\"isPrivate\":false,\"isFork\":false,\"updatedAt\":\"2026-10-01T00:00:00Z\"}]' ;; *) exit 1 ;; esac\n",
    );
    let fixture = build_fixture();
    let (app, window) = mount(cx, &fixture.path().canonicalize().unwrap());
    let before = crate::macos::repo_fingerprint(fixture.path());
    let lang = kagi::ui::i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        kagi::ui::i18n::set_lang(language);
        assert!(
            cx.read(|cx| matches!(
                command_state(app.read(cx), "file.cloneRepository"),
                CommandState::Enabled
            )),
            "Clone Repository must be enabled, not an unimplemented placeholder"
        );
        app.update(cx, |app, cx| {
            app.set_home_pane(HomePane::Issues, cx);
            assert!(app.open_platform_menu_for_e2e("File", cx));
        });
        cx.run_until_parked();
        crate::app_conflict::click_control(cx, window, "platform-menu-cmd-file.cloneRepository");
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.home_in_front());
            assert_eq!(app.home_github.work.pane, HomePane::Repos);
            assert_eq!(app.tabs.len(), 1);
        });
        crate::app_conflict::click_control(cx, window, "home-gh-acme/widgets");
        assert!(
            cx.read(|cx| app.read(cx).clone_modal().is_some()),
            "Home selection must reach the existing clone card"
        );
        app.update(cx, |app, _| app.cancel_clone());
        app.update(cx, |app, cx| app.close_home_tab(cx));
        crate::recovery_operations::press_key(cx, &app, window, "cmd-shift-o");
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).home_in_front()),
            "Clone shortcut must route to Home"
        );
        crate::app_conflict::click_control(cx, window, "home-gh-acme/widgets");
        assert!(
            cx.read(|cx| app.read(cx).clone_modal().is_some()),
            "Shortcut picker must reach the clone card"
        );
        app.update(cx, |app, _| app.cancel_clone());
        app.update(cx, |app, cx| app.close_home_tab(cx));
    }
    kagi::ui::i18n::set_lang(lang);
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS clone_menu_routes_home");
}
