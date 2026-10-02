//! #922: JSON themes under `$KAGI_LOG_DIR/themes` join the one runtime theme
//! list. A file that extends a built-in and a file that spells out every
//! field both load at startup, appear in Settings, the command palette and
//! the View → Theme menu, switch the app's colours, persist their slug across
//! a restart and survive a reload; a broken file is named in a toast and
//! leaves the others alone; a removed file falls back to the default theme
//! for the run without rewriting `settings.json`.

use std::path::PathBuf;

use gpui::{AnyWindowHandle, Entity, SharedString, VisualTestAppContext};
use gpui_component::select::SelectEvent;
use gpui_component::ActiveTheme as _;
use kagi::ui::command_palette::{self, PaletteAction};
use kagi::ui::settings_view::ThemeOption;
use kagi::ui::{commands, settings, theme, KagiApp};
use serde_json::{json, Value};

use crate::macos::{build_fixture, mount, unmount};

const INHERITED: &str = "e2e-inherited";
const STANDALONE: &str = "e2e-standalone";
const BROKEN_FILE: &str = "broken.json";

/// The fixture files, removed (with the directory when this created it) and
/// unloaded again on drop, so a failing assertion cannot leave custom themes
/// for the scenarios that follow.
struct ThemeFiles {
    dir: PathBuf,
    created_dir: bool,
    before: String,
}

impl ThemeFiles {
    fn new(before: String) -> Self {
        let dir = theme::themes_dir().expect("the runner sets KAGI_LOG_DIR");
        let created_dir = !dir.exists();
        std::fs::create_dir_all(&dir).expect("themes dir");
        Self {
            dir,
            created_dir,
            before,
        }
    }

    fn write(&self, file: &str, body: &Value) {
        let text = serde_json::to_string_pretty(body).expect("fixture json");
        std::fs::write(self.dir.join(file), text).expect("write theme file");
    }

    fn write_text(&self, file: &str, text: &str) {
        std::fs::write(self.dir.join(file), text).expect("write theme file");
    }

    fn remove_all(&self) {
        for file in ["inherited.json", "standalone.json", BROKEN_FILE] {
            let _ = std::fs::remove_file(self.dir.join(file));
        }
    }
}

impl Drop for ThemeFiles {
    fn drop(&mut self) {
        self.remove_all();
        if self.created_dir {
            let _ = std::fs::remove_dir(&self.dir);
        }
        let _ = theme::reload_custom_themes();
        theme::set_active(&self.before);
    }
}

fn hex(rgb: u64) -> Value {
    Value::String(format!("#{rgb:06x}"))
}

fn rgb_of(channels: &[Value]) -> u64 {
    let c = |i: usize| channels[i].as_u64().expect("colour channel");
    (c(0) << 16) | (c(1) << 8) | c(2)
}

/// Every field of the built-in `base`, in the file format, under a new slug
/// and name with `bg_base` replaced — a theme file with no `extends`.
fn standalone_theme(base: &str, bg_base: u32) -> Value {
    let base = theme::theme_by_slug(base).expect("built-in base theme");
    let Value::Object(mut fields) = serde_json::to_value(&*base).expect("serialize theme") else {
        panic!("a theme serializes to an object");
    };
    for (key, value) in fields.iter_mut() {
        match key.as_str() {
            "slug" | "name" | "dark" | "lane_hsl" | "avatar_sat" | "avatar_light" => {}
            "syntax" => {
                for token in value.as_object_mut().expect("syntax object").values_mut() {
                    *token = hex(token.as_u64().expect("syntax colour"));
                }
            }
            "term_selection" => {
                let channels = value.as_array().expect("selection rgba").clone();
                *value = json!({ "color": hex(rgb_of(&channels)), "alpha": channels[3] });
            }
            term if term.starts_with("term_") => {
                *value = hex(rgb_of(value.as_array().expect("terminal rgb")));
            }
            _ => *value = hex(value.as_u64().unwrap_or_else(|| panic!("{key} colour"))),
        }
    }
    fields.insert("slug".into(), json!(STANDALONE));
    fields.insert("name".into(), json!("E2E Standalone"));
    fields.insert("bg_base".into(), hex(u64::from(bg_base)));
    Value::Object(fields)
}

fn gpui_background(cx: &mut VisualTestAppContext) -> gpui::Hsla {
    cx.read(|cx| cx.theme().background)
}

fn expected_background(rgb: u32) -> gpui::Hsla {
    gpui::rgb(rgb).into()
}

fn toasts_naming(cx: &mut VisualTestAppContext, kagi: &Entity<KagiApp>, file: &str) -> usize {
    cx.read(|cx| {
        let stack = kagi.read(cx).toast_stack.clone().expect("toast stack");
        stack
            .read(cx)
            .toasts()
            .iter()
            .filter(|toast| toast.message.contains(file))
            .count()
    })
}

/// Type `query` into the real command palette and run its top row, the way
/// Enter does. Returns the row that ran.
fn run_palette(
    cx: &mut VisualTestAppContext,
    kagi: &Entity<KagiApp>,
    window: AnyWindowHandle,
    query: &str,
) -> command_palette::PaletteRow {
    let row = cx
        .update_window(window, |_, window, cx| {
            kagi.update(cx, |app, cx| {
                app.open_command_palette(window, cx);
                let input = app.command_palette_input.clone().expect("palette input");
                input.update(cx, |state, cx| state.set_value(query, window, cx));
                let row = command_palette::rows_for(app, query).into_iter().next();
                app.run_selected_command_palette(window, cx);
                row
            })
        })
        .expect("window");
    cx.run_until_parked();
    row.unwrap_or_else(|| panic!("palette has no row for {query:?}"))
}

/// The Settings picker's own items hold `slug`: selecting it by value finds
/// a row. The selection is put back to the active theme afterwards.
fn select_lists(
    cx: &mut VisualTestAppContext,
    kagi: &Entity<KagiApp>,
    window: AnyWindowHandle,
    slug: &str,
) -> bool {
    let select = cx.read(|cx| kagi.read(cx).theme_select.clone().expect("theme select"));
    let active = SharedString::from(theme::theme().slug.to_string());
    cx.update_window(window, |_, window, cx| {
        select.update(cx, |state, cx| {
            state.set_selected_value(&SharedString::from(slug.to_string()), window, cx);
            let found = state.selected_value().is_some_and(|value| value == slug);
            state.set_selected_value(&active, window, cx);
            found
        })
    })
    .expect("window")
}

fn select_shows(cx: &mut VisualTestAppContext, kagi: &Entity<KagiApp>) -> Option<String> {
    cx.read(|cx| {
        let select = kagi.read(cx).theme_select.clone()?;
        select
            .read(cx)
            .selected_value()
            .map(|value| value.to_string())
    })
}

fn menu_lists(slug: &str) -> bool {
    commands::theme_menu_entries()
        .iter()
        .any(|entry| entry.slug == slug)
}

fn assert_every_surface_lists(
    cx: &mut VisualTestAppContext,
    kagi: &Entity<KagiApp>,
    window: AnyWindowHandle,
    slug: &str,
    name: &str,
) {
    assert!(menu_lists(slug), "View → Theme lacks {slug}");
    let rows = cx.read(|cx| command_palette::rows_for(kagi.read(cx), name));
    assert!(
        rows.iter()
            .any(|row| row.action == PaletteAction::SetTheme(SharedString::from(slug.to_string()))),
        "the palette lacks {slug}"
    );
    assert!(
        select_lists(cx, kagi, window, slug),
        "Settings lacks {slug}"
    );
}

pub fn scenario_theme_custom(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let before = theme::theme().slug.to_string();
    let files = ThemeFiles::new(before.clone());
    files.write(
        "inherited.json",
        &json!({
            "slug": INHERITED,
            "name": "E2E Inherited",
            "extends": "dracula",
            "bg_base": "#123456",
        }),
    );
    files.write(
        "standalone.json",
        &standalone_theme("tokyo-night", 0x204060),
    );
    files.write_text(
        BROKEN_FILE,
        r##"{"slug":"e2e-broken","name":"Broken","extends":"dracula","bg_bse":"#000000"}"##,
    );

    // Startup: the files load before the window exists; the broken one is
    // reported once the window is up.
    theme::init_active();
    let fixture = build_fixture();
    let (kagi, window) = mount(cx, fixture.path());
    assert_eq!(
        toasts_naming(cx, &kagi, BROKEN_FILE),
        1,
        "one toast names the broken file"
    );
    assert!(theme::theme_by_slug("e2e-broken").is_none());
    assert_every_surface_lists(cx, &kagi, window, INHERITED, "E2E Inherited");
    assert_every_surface_lists(cx, &kagi, window, STANDALONE, "E2E Standalone");

    // The View → Theme menu item's action: inherits Dracula, overrides bg.
    cx.dispatch_action(
        window,
        commands::SetTheme {
            slug: SharedString::from(INHERITED),
        },
    );
    cx.run_until_parked();
    let dracula = theme::theme_by_slug("dracula").expect("dracula");
    let active = theme::theme();
    assert_eq!(active.slug, INHERITED);
    assert_eq!(active.bg_base, 0x123456);
    assert_eq!(active.text_main, dracula.text_main, "unset keys inherit");
    assert_eq!(gpui_background(cx), expected_background(0x123456));
    assert_eq!(settings::read_setting("theme").as_deref(), Some(INHERITED));

    // Opening Settings after a menu selection must highlight the theme that
    // was just applied, not the startup theme retained by its Select entity.
    cx.dispatch_action(window, commands::OpenSettings);
    cx.run_until_parked();
    assert_eq!(select_shows(cx, &kagi).as_deref(), Some(INHERITED));

    // The Settings picker's Confirm, as a click on its row emits it.
    let select = cx.read(|cx| kagi.read(cx).theme_select.clone().expect("theme select"));
    select.update(cx, |_, cx| {
        cx.emit(SelectEvent::<Vec<ThemeOption>>::Confirm(Some(
            SharedString::from(STANDALONE),
        )))
    });
    cx.run_until_parked();
    assert_eq!(theme::theme().slug, STANDALONE);
    assert_eq!(theme::theme().bg_base, 0x204060);
    assert_eq!(gpui_background(cx), expected_background(0x204060));

    // The palette row switches too; back to the standalone theme for the restart.
    let row = run_palette(cx, &kagi, window, "E2E Inherited");
    assert_eq!(
        row.action,
        PaletteAction::SetTheme(SharedString::from(INHERITED))
    );
    assert_eq!(theme::theme().slug, INHERITED);
    run_palette(cx, &kagi, window, "E2E Standalone");
    assert_eq!(settings::read_setting("theme").as_deref(), Some(STANDALONE));

    // Restart: the saved slug resolves to the custom theme again.
    unmount(cx, kagi, window);
    theme::set_active(&before);
    settings::write_setting("theme", Some(STANDALONE));
    theme::init_active();
    let (kagi, window) = mount(cx, fixture.path());
    assert_eq!(theme::theme().slug, STANDALONE, "restored after restart");
    assert_eq!(select_shows(cx, &kagi).as_deref(), Some(STANDALONE));

    // Reload from the palette: the edited file recolours the active theme,
    // which stays selected; the broken file is reported again.
    files.write(
        "standalone.json",
        &standalone_theme("tokyo-night", 0x306090),
    );
    let broken_before = toasts_naming(cx, &kagi, BROKEN_FILE);
    let row = run_palette(cx, &kagi, window, "Reload Themes");
    assert_eq!(row.action, PaletteAction::Command("view.reloadThemes"));
    assert_eq!(
        theme::theme().slug,
        STANDALONE,
        "a reload keeps the selection"
    );
    assert_eq!(theme::theme().bg_base, 0x306090);
    assert_eq!(gpui_background(cx), expected_background(0x306090));
    assert_eq!(select_shows(cx, &kagi).as_deref(), Some(STANDALONE));
    assert_eq!(toasts_naming(cx, &kagi, BROKEN_FILE), broken_before + 1);

    // Removed files: the default theme for this run, the saved slug untouched.
    files.remove_all();
    run_palette(cx, &kagi, window, "Reload Themes");
    let default_slug = theme::THEMES[0].slug.to_string();
    assert_eq!(theme::theme().slug, default_slug);
    assert_eq!(settings::read_setting("theme").as_deref(), Some(STANDALONE));
    assert!(!menu_lists(INHERITED) && !menu_lists(STANDALONE));
    assert!(!select_lists(cx, &kagi, window, STANDALONE));

    kagi.update(cx, |app, cx| app.set_theme(&before, cx));
    unmount(cx, kagi, window);
    drop(files);
    eprintln!(
        "[gui-e2e] PASS theme_custom extends+standalone listed, switched, restored, reloaded"
    );
}

/// Settings exposes the custom-theme folder and reload without requiring the
/// command palette. The OS opener is replaced by a local executable: no Finder
/// window or desktop session escapes this GUI scenario.
pub fn scenario_theme_folder_controls(cx: &mut VisualTestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let sandbox = tempfile::tempdir().expect("theme folder sandbox");
    let previous_log = std::env::var_os("KAGI_LOG_DIR");
    let previous_path = std::env::var_os("PATH");
    struct Restore(Option<std::ffi::OsString>, Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(log) = self.0.take() {
                std::env::set_var("KAGI_LOG_DIR", log);
            } else {
                std::env::remove_var("KAGI_LOG_DIR");
            }
            if let Some(path) = self.1.take() {
                std::env::set_var("PATH", path);
            } else {
                std::env::remove_var("PATH");
            }
        }
    }
    let _restore = Restore(previous_log, previous_path.clone());
    std::env::set_var("KAGI_LOG_DIR", sandbox.path());
    let bin = sandbox.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    for name in ["open", "xdg-open"] {
        let script = bin.join(name);
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$1/../opened.txt\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        previous_path.as_deref().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let folder = theme::themes_dir().expect("sandbox theme dir");
    assert!(!folder.exists());

    let fixture = build_fixture();
    let (kagi, window) = mount(cx, fixture.path());
    cx.dispatch_action(window, commands::OpenSettings);
    cx.run_until_parked();
    let id = window.window_id();
    for name in [
        "settings-custom-themes-path",
        "settings-custom-themes-open",
        "settings-custom-themes-reload",
    ] {
        kagi::ui::e2e::clear_control_bounds(id, name);
    }
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    for name in [
        "settings-custom-themes-path",
        "settings-custom-themes-open",
        "settings-custom-themes-reload",
    ] {
        assert!(
            kagi::ui::e2e::control_bounds(id, name).is_some(),
            "{name} not painted"
        );
    }
    crate::app_conflict::click_control(cx, window, "settings-custom-themes-open");
    cx.run_until_parked();
    assert!(
        folder.is_dir(),
        "Open folder creates missing themes directory"
    );
    // Wait for the spawned opener before the sandbox is dropped, and confirm
    // it received the actual resolved theme directory.
    let opened = sandbox.path().join("opened.txt");
    for _ in 0..100 {
        if opened.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(opened).unwrap(),
        folder.to_string_lossy()
    );
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    crate::app_conflict::click_control(cx, window, "settings-custom-themes-reload");
    cx.run_until_parked();
    assert_eq!(toasts_naming(cx, &kagi, "Themes reloaded"), 1);
    std::fs::remove_dir(&folder).unwrap();
    std::fs::write(&folder, "not a directory").unwrap();
    crate::app_conflict::click_control(cx, window, "settings-custom-themes-open");
    cx.run_until_parked();
    assert_eq!(toasts_naming(cx, &kagi, "Couldn't open theme folder"), 1);
    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS theme_folder_controls creates, reloads, and reports folder errors");
}
