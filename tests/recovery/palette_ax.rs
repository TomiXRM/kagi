//! #1513: actual AccessKit actions and native AppKit Press delivery.
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::native_ax;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    command_palette::{rows_for, PaletteAction},
    commands, e2e, KagiApp,
};
use kagi_ui_core::i18n::{self, Lang};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, query: &str) {
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |app, cx| {
            app.open_command_palette(w, cx);
            app.command_palette_input
                .clone()
                .unwrap()
                .update(cx, |input, cx| input.set_value(query, w, cx));
        })
    })
    .unwrap();
    draw(cx, win);
}

pub fn scenario_palette_rows_ax_action(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    for locale in ["en", "ja"] {
        for preset in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                std::env::set_var("KAGI_LANG", locale);
                kagi_ui_core::i18n::init_lang();
                kagi_ui_core::theme::set_active(preset);
                kagi_ui_core::theme::set_zoom(zoom);
                let (app, win) = mount(cx, fixture.path());
                // In-process predecessors may have dispatched palette actions.
                e2e::palette::take_executed();
                native_ax::activate(cx);
                for kind in ["command", "theme", "language"] {
                    let query = match kind {
                        "theme" => kagi_ui_core::i18n::Msg::SettingsTheme.t(),
                        "language" => kagi_ui_core::i18n::Msg::SettingsLanguage.t(),
                        _ => "",
                    };
                    open(cx, &app, win, query);
                    let rows = cx.read(|cx| rows_for(app.read(cx), query, cx));
                    for (index, row) in rows.iter().enumerate() {
                        let node = native_ax::node(cx, win, ("palette-row", index).into());
                        assert_eq!(node["aria"]["role"], "ListBoxOption");
                        assert_eq!(node["aria"]["label"], row.label);
                        assert!(
                            node["aria"]["on_action"]
                                .as_array()
                                .is_some_and(|actions| actions.iter().any(|a| a == "Click")),
                            "palette row must expose native Click: {node}"
                        );
                    }
                    let target = rows
                        .iter()
                        .position(|row| {
                            row.enabled
                                && match (&row.action, kind) {
                                    (PaletteAction::SetTheme(slug), "theme") => {
                                        slug.as_ref() == preset
                                    }
                                    (PaletteAction::SetLang(lang), "language") => {
                                        *lang == kagi_ui_core::i18n::lang()
                                    }
                                    (PaletteAction::Command(id), "command") => {
                                        *id == "view.toggleSidebar"
                                    }
                                    _ => false,
                                }
                        })
                        .expect("exact enabled fixture action");
                    let action = rows[target].action.clone();
                    assert!(
                        native_ax::press(cx, win, &rows[target].label),
                        "native row Press is delivered"
                    );
                    draw(cx, win);
                    assert_eq!(e2e::palette::take_executed(), vec![action.clone()]);
                    open(cx, &app, win, query);
                    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                    scroll.scroll_to_item(target);
                    draw(cx, win);
                    let at = scroll.bounds_for_item(target).unwrap().center()
                        + gpui::point(gpui::px(0.), scroll.offset().y);
                    cx.simulate_click(win, at, gpui::Modifiers::none());
                    draw(cx, win);
                    assert_eq!(
                        e2e::palette::take_executed(),
                        vec![action.clone()],
                        "down/up must dispatch exactly once"
                    );
                    open(cx, &app, win, query);
                    for _ in 0..target {
                        cx.simulate_keystrokes(win, "down");
                    }
                    draw(cx, win);
                    assert_eq!(cx.read(|cx| app.read(cx).command_palette_selected), target);
                    cx.simulate_keystrokes(win, if target == 0 { "down up" } else { "up down" });
                    draw(cx, win);
                    assert_eq!(cx.read(|cx| app.read(cx).command_palette_selected), target);
                    let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                    let selected = scroll.bounds_for_item(target).unwrap();
                    assert!(selected.top() + scroll.offset().y >= scroll.bounds().top());
                    assert!(selected.bottom() + scroll.offset().y <= scroll.bounds().bottom());
                    let node = native_ax::node(cx, win, ("palette-row", target).into());
                    assert_eq!(node["aria"]["selected"], true);
                    cx.simulate_keystrokes(win, "enter");
                    draw(cx, win);
                    assert_eq!(
                        e2e::palette::take_executed(),
                        vec![action],
                        "Input Enter dispatches highlighted identity"
                    );
                }
                open(cx, &app, win, "");
                let rows = cx.read(|cx| rows_for(app.read(cx), "", cx));
                let alternate_lang = rows.iter().find(|row| matches!(row.action, PaletteAction::SetLang(lang) if lang != kagi_ui_core::i18n::lang())).unwrap();
                assert!(native_ax::press(cx, win, &alternate_lang.label));
                draw(cx, win);
                assert_eq!(
                    e2e::palette::take_executed(),
                    vec![alternate_lang.action.clone()]
                );
                if let PaletteAction::SetLang(lang) = alternate_lang.action {
                    assert_eq!(kagi_ui_core::i18n::lang(), lang);
                }
                open(cx, &app, win, "");
                let rows = cx.read(|cx| rows_for(app.read(cx), "", cx));
                let disabled_index = rows
                    .iter()
                    .position(|row| !row.enabled)
                    .expect("disabled fixture command");
                let disabled = &rows[disabled_index];
                assert!(native_ax::press(cx, win, &disabled.label));
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                assert_eq!(
                    cx.read(|cx| app.read(cx).command_palette_selected),
                    disabled_index
                );
                let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
                scroll.scroll_to_item(disabled_index);
                draw(cx, win);
                let at = scroll.bounds_for_item(disabled_index).unwrap().center()
                    + gpui::point(gpui::px(0.), scroll.offset().y);
                cx.simulate_click(win, at, gpui::Modifiers::none());
                cx.simulate_keystrokes(win, "enter");
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                open(cx, &app, win, "palette-no-matching-result-1513");
                assert!(cx
                    .read(|cx| rows_for(app.read(cx), "palette-no-matching-result-1513", cx))
                    .is_empty());
                cx.simulate_keystrokes(win, "up down enter");
                draw(cx, win);
                assert!(e2e::palette::take_executed().is_empty());
                unmount(cx, app, win);
            }
        }
    }
    eprintln!("[gui-e2e] PASS palette_rows_ax_action");
}

fn assert_conflict_admission(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    expected: [bool; 3],
) {
    let rows = cx.read(|cx| rows_for(app.read(cx), "", cx));
    let menus = cx.read(commands::build_menus);
    let actions: [(&str, &dyn gpui::Action); 3] = [
        ("repo.conflictAbort", &commands::ConflictAbort),
        ("repo.conflictSkip", &commands::ConflictSkip),
        ("repo.conflictContinue", &commands::ConflictContinue),
    ];
    for ((id, action), expected) in actions.into_iter().zip(expected) {
        let row = rows
            .iter()
            .find(|row| row.id == id)
            .expect("registered row");
        let menu_enabled = cx
            .update_window(win, |_, window, cx| window.is_action_available(action, cx))
            .unwrap();
        assert_eq!(menu_enabled, expected, "menu admission for {id}");
        assert_eq!(row.enabled, menu_enabled, "palette/menu parity for {id}");
        assert_eq!(row.disabled_reason.is_none(), menu_enabled);
        let menu_label = menus
            .iter()
            .flat_map(|menu| &menu.items)
            .find_map(|item| match item {
                gpui::MenuItem::Action {
                    action: menu_action,
                    name,
                    ..
                } if menu_action.as_any().type_id() == action.as_any().type_id() => {
                    Some(name.as_ref())
                }
                _ => None,
            })
            .expect("registered menu command");
        assert_eq!(row.label, menu_label, "palette/menu label parity for {id}");
    }
}

fn activate_conflict_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    id: &'static str,
    input: &str,
    enabled: bool,
) {
    native_ax::activate(cx);
    e2e::palette::take_executed();
    let label = cx.read(|cx| {
        rows_for(app.read(cx), "", cx)
            .into_iter()
            .find(|row| row.id == id)
            .expect("conflict command")
            .label
    });
    open(cx, app, win, &label);
    let rows = cx.read(|cx| rows_for(app.read(cx), &label, cx));
    let index = rows
        .iter()
        .position(|row| row.id == id)
        .expect("conflict command");
    assert_eq!(rows[index].enabled, enabled);
    match input {
        "enter" => {
            for _ in 0..index {
                cx.simulate_keystrokes(win, "down");
            }
            cx.simulate_keystrokes(win, "enter");
        }
        "pointer" => {
            let scroll = cx.read(|cx| e2e::command_palette_scroll(app.read(cx)).unwrap());
            scroll.scroll_to_item(index);
            draw(cx, win);
            let at = scroll.bounds_for_item(index).unwrap().center()
                + gpui::point(gpui::px(0.), scroll.offset().y);
            cx.simulate_click(win, at, gpui::Modifiers::none());
        }
        "ax" => assert!(native_ax::press(cx, win, &rows[index].label)),
        _ => unreachable!(),
    }
    draw(cx, win);
    let executed = e2e::palette::take_executed();
    if enabled {
        assert_eq!(executed, vec![PaletteAction::Command(id)], "{input}: {id}");
        assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    } else {
        assert!(executed.is_empty(), "disabled {input}: {id} must be inert");
        assert!(cx.read(|cx| matches!(
            app.read(cx).menu_overlay,
            Some(commands::MenuOverlay::CommandPalette)
        )));
        cx.simulate_keystrokes(win, "escape");
        draw(cx, win);
    }
}

fn mount_palette_repo(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let (app, win) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.set_window_active_for_e2e(true, cx);
        app.detect_conflict_mode(cx);
    });
    draw(cx, win);
    (app, win)
}

pub fn scenario_palette_conflict_admission(cx: &mut VisualTestAppContext) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for input in ["enter", "pointer", "ax"] {
            let fixture = crate::app_conflict::content_fixture();
            let repo = fixture.path();
            let before = repo_fingerprint(repo);
            let (app, win) = mount_palette_repo(cx, repo);
            assert_conflict_admission(cx, &app, win, [true, false, false]);
            for id in ["repo.conflictSkip", "repo.conflictContinue"] {
                activate_conflict_row(cx, &app, win, id, input, false);
            }
            activate_conflict_row(cx, &app, win, "repo.conflictAbort", input, true);
            assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()));
            assert_conflict_admission(cx, &app, win, [false, false, false]);
            assert_eq!(
                repo_fingerprint(repo),
                before,
                "Abort only opens confirmation"
            );
            app.update(cx, |app, cx| {
                app.cancel_conflict_abort();
                cx.notify();
            });
            unmount(cx, app, win);

            git(repo, &["merge", "--abort"]);
            let (app, win) = mount_palette_repo(cx, repo);
            assert_conflict_admission(cx, &app, win, [false, false, false]);
            let before = repo_fingerprint(repo);
            for id in [
                "repo.conflictAbort",
                "repo.conflictSkip",
                "repo.conflictContinue",
            ] {
                activate_conflict_row(cx, &app, win, id, input, false);
            }
            assert_eq!(repo_fingerprint(repo), before);
            unmount(cx, app, win);

            assert!(!git_fixture::git_succeeds(
                repo,
                &["cherry-pick", "feature"]
            ));
            let before = repo_fingerprint(repo);
            let (app, win) = mount_palette_repo(cx, repo);
            assert_conflict_admission(cx, &app, win, [true, true, false]);
            activate_conflict_row(cx, &app, win, "repo.conflictSkip", input, true);
            assert!(cx.read(|cx| app
                .read(cx)
                .ui()
                .conflict
                .as_ref()
                .unwrap()
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .skip_approval
                .is_some()));
            let expected = match lang {
                Lang::En => "Confirm Skip",
                Lang::Ja => "スキップを確定",
            };
            open(cx, &app, win, expected);
            assert_conflict_admission(cx, &app, win, [true, true, false]);
            let rows = cx.read(|cx| rows_for(app.read(cx), expected, cx));
            let index = rows
                .iter()
                .position(|row| row.id == "repo.conflictSkip")
                .expect("armed Skip must match its confirmation label");
            assert_eq!(rows[index].label, expected);
            let node = native_ax::node(cx, win, ("palette-row", index).into());
            assert_eq!(node["aria"]["label"], expected);
            cx.simulate_keystrokes(win, "escape");
            draw(cx, win);
            assert_eq!(
                repo_fingerprint(repo),
                before,
                "first Skip only arms confirmation"
            );
            unmount(cx, app, win);

            let saved = crate::app_conflict::content_fixture();
            std::fs::write(saved.path().join("file.txt"), "resolved\n").unwrap();
            git(saved.path(), &["add", "file.txt"]);
            let head = git_fixture::git_output(saved.path(), &["rev-parse", "HEAD"]);
            let (app, win) = mount_palette_repo(cx, saved.path());
            assert_conflict_admission(cx, &app, win, [true, false, true]);
            activate_conflict_row(cx, &app, win, "repo.conflictContinue", input, true);
            crate::app_conflict::wait_idle(cx, &app);
            draw(cx, win);
            cx.read(|cx| {
                let ui = app.read(cx).ui();
                assert!(ui.conflict_merge_pending && ui.commit_panel_open);
            });
            assert!(e2e::control_bounds(win.window_id(), "cp-body").is_some());
            assert_eq!(
                git_fixture::git_output(saved.path(), &["rev-parse", "HEAD"]),
                head
            );
            assert!(saved.path().join(".git/MERGE_HEAD").exists());
            unmount(cx, app, win);
        }
    }
}
