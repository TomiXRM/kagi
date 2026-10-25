//! #1563: Help uses the same effective native shortcut glyphs as the palette.
use crate::close_window_initial_home::{draw, mount};
use crate::surface_scrollbars::PresentationRestore;
use gpui::VisualTestAppContext;
use kagi::ui::{command_palette, commands, e2e};
use kagi_ui_core::{i18n, theme};

pub fn scenario_help_shortcuts_glyphs(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let fixture = crate::macos::build_fixture();
    let before = crate::macos::repo_fingerprint(fixture.path());
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            for preset in ["apple-light", "apple-dark"] {
                theme::set_active(preset);
                for locale in ["en", "ja"] {
                    std::env::set_var("KAGI_LANG", locale);
                    i18n::init_lang();
                    check_help(cx, fixture.path(), dimensions);
                }
            }
        }
    }
    assert_eq!(crate::macos::repo_fingerprint(fixture.path()), before);
    eprintln!("[gui-e2e] PASS help_shortcuts_glyphs");
}

fn check_help(cx: &mut VisualTestAppContext, repo: &std::path::Path, (width, height): (f32, f32)) {
    let (app, window) = mount(cx, e2e::app_state(repo).unwrap(), width, height);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("help.shortcuts", window, cx);
        });
    })
    .unwrap();
    draw(cx, window);
    let lines = app.read_with(cx, |app, _| match app.menu_overlay.as_ref().unwrap() {
        commands::MenuOverlay::Info { title, lines, .. } => {
            assert_eq!(
                title.as_ref(),
                command_palette::command_label("help.shortcuts", i18n::lang())
            );
            lines.clone()
        }
        _ => panic!("Help opens the shortcut info panel"),
    });
    let rows = app.read_with(cx, |app, cx| command_palette::rows_for(app, "", cx));
    let listing = commands::shortcut_listing();
    let mut index = 0;
    for command in commands::COMMANDS {
        let label = command_palette::command_label(command.id, i18n::lang());
        let Some(binding) = commands::effective_keystroke(command.id) else {
            assert!(!listing.iter().any(|(name, _)| name.as_ref() == label));
            continue;
        };
        let expected = gpui_component::kbd::Kbd::format(&gpui::Keystroke::parse(&binding).unwrap());
        assert_eq!(listing[index].0.as_ref(), label);
        assert_eq!(
            listing[index].1.as_ref(),
            expected,
            "Help must display native glyphs for {}",
            command.id
        );
        assert_eq!(lines[index].as_ref(), format!("{expected}    {label}"));
        if let Some(row) = rows
            .iter()
            .find(|row| row.action == command_palette::PaletteAction::Command(command.id))
        {
            assert_eq!(
                row.keystroke.as_deref(),
                Some(expected.as_str()),
                "Help and palette agree"
            );
        }
        match command.id {
            "app.quit" => assert_eq!(expected, "⌘Q"),
            "app.settings" => assert_eq!(expected, "⌘,"),
            "view.toggleSidebar" => assert_eq!(expected, "⌃⌥⇧⌘K"),
            "view.toggleInspector" => assert_eq!(expected, "⌘⌫"),
            "file.openRepository" => assert_eq!(expected, "⇧⌘O"),
            _ => {}
        }
        index += 1;
    }
    assert_eq!(index, listing.len());
    assert!(
        commands::effective_keystroke("repo.fetch").is_none(),
        "startup fixture exercises unassignment"
    );
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(app.read_with(cx, |app, _| app.menu_overlay.is_none()));
    let sidebar_before = app.read_with(cx, |app, _| app.sidebar.visible);
    cx.simulate_keystrokes(window, "ctrl-alt-shift-cmd-k");
    cx.run_until_parked();
    assert_eq!(
        app.read_with(cx, |app, _| app.sidebar.visible),
        !sidebar_before,
        "displayed remapping dispatches its action"
    );
    crate::macos::unmount(cx, app, window);
}
