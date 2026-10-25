//! Native right-click delivery to the three production code-editor hosts.
use crate::macos::{build_fixture, open_offscreen, unmount};
use crate::recovery_operations::paint;
use gpui::{Action, AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::input::{Copy, Cut, InputState, Paste, SelectAll};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n::Lang, input};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

pub(super) fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    width: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = open_offscreen(
        cx,
        gpui::size(gpui::px(width), gpui::px(720.)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window.into())
}

fn assert_menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    state: &Entity<InputState>,
    window: AnyWindowHandle,
    host: &str,
) {
    // Retain the same field across language switches, not a freshly initialized
    // component whose labels could accidentally have been fixed at construction.
    for lang in [Lang::Ja, Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        crate::native_ax::activate(cx);
        cx.update_window(window, |_, window, cx| {
            state.read(cx).focus_handle(cx).focus(window, cx);
            state.update(cx, |state, cx| state.set_selected_range(0..1, cx));
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        paint(cx, window);
        let bounds = cx
            .read(|cx| state.read(cx).range_to_bounds(&(0..1)))
            .expect("first selected character is laid out in the actual input");
        // Hit before the glyph's midpoint: the Input chooses the nearest caret
        // offset, and the end of the one-character selection is exclusive.
        let selected_x = bounds.left() + bounds.size.width / 4.;
        // Result is independently clipped in the compact three-pane layout.
        // Hit its painted character fragment, not the off-clip line centre.
        let pointer = if let Some(viewport) =
            e2e::control_bounds(window.window_id(), "conflict-result-editor")
        {
            let top = bounds.top().max(viewport.top());
            let bottom = bounds.bottom().min(viewport.bottom());
            assert!(bottom > top, "a visible Result character is required");
            gpui::point(selected_x, (top + bottom) / 2.)
        } else {
            gpui::point(selected_x, bounds.center().y)
        };
        input::capture_next_menu_for_e2e();
        cx.simulate_mouse_down(
            window,
            pointer,
            gpui::MouseButton::Right,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_up(
            window,
            pointer,
            gpui::MouseButton::Right,
            gpui::Modifiers::none(),
        );
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_range()),
            0..1,
            "{host}: right-click inside the selected glyph preserves selection"
        );
        let menu = input::take_menu_for_e2e().unwrap_or_else(|| panic!(
            "{host}: actual right-click must build the Kagi menu; character={bounds:?}; result={:?}; focused={}",
            e2e::control_bounds(window.window_id(), "conflict-result-editor"),
            cx.update_window(window, |_, window, cx| state.read(cx).focus_handle(cx).is_focused(window)).unwrap(),
        ));
        let actions: Vec<_> = menu
            .items
            .iter()
            .filter_map(|item| match item {
                gpui::MenuItem::Action {
                    name,
                    action,
                    disabled,
                    ..
                } => Some((name.as_ref(), action.name(), *disabled)),
                _ => None,
            })
            .collect();
        let labels: Vec<_> = actions.iter().map(|item| item.0).collect();
        assert_eq!(
            labels,
            if lang == Lang::Ja {
                ["カット", "コピー", "ペースト", "すべてを選択"]
            } else {
                ["Cut", "Copy", "Paste", "Select All"]
            },
            "{host}: only applicable localized text-editing commands; no LSP items"
        );
        assert_eq!(
            actions.iter().map(|item| item.1).collect::<Vec<_>>(),
            [Cut.name(), Copy.name(), Paste.name(), SelectAll.name()]
        );
        assert!(
            !actions[0].2 && !actions[1].2,
            "{host}: selected text enables Cut/Copy; actions={actions:?}; selection={:?}",
            cx.read(|cx| state.read(cx).selected_range())
        );
        assert!(!actions[3].2, "Select All remains available");
        assert_eq!(menu.items.len(), 5, "four actions and their one separator");
    }
}

fn editor(cx: &mut VisualTestAppContext, width: f32) {
    let fixture = build_fixture();
    let (app, window) = mount_at(cx, fixture.path(), width);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| {
        view.open_tab("README.md".into(), cx);
        // Markdown now opens in Preview by default. This scenario exercises
        // the code Input's native menu, so show the actual editable buffer.
        view.set_markdown_preview(false, cx);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        paint(cx, window);
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "Editor file did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    let state = cx.read(|cx| editor.read(cx).editor.clone()).unwrap();
    assert_menu(cx, &app, &state, window, "Editor");
    drop(state);
    drop(editor);
    unmount(cx, app, window);
}

fn conflict(cx: &mut VisualTestAppContext, width: f32) {
    let fixture = crate::conflict_text_keyboard::fixture();
    let (app, window) = mount_at(cx, fixture.path(), width);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    paint(cx, window);
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("notes.txt"));
        view.conflict_editor_set_result_mode(true);
        cx.notify();
    });
    paint(cx, window);
    let state = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    assert_menu(cx, &app, &state, window, "Conflict Result");
    drop(state);
    drop(conflict);
    unmount(cx, app, window);
}

fn composer(cx: &mut VisualTestAppContext, width: f32) {
    let fixture = build_fixture();
    let (app, window) = mount_at(cx, fixture.path(), width);
    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
    })
    .unwrap();
    cx.simulate_keystrokes(window, "Menu test");
    crate::keyboard_nav::keys(cx, window, "enter");
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_body_for_e2e("alpha alpha", window, cx)
        });
    })
    .unwrap();
    paint(cx, window);
    let state = cx
        .read(|cx| app.read(cx).issue_body_input_for_e2e())
        .unwrap();
    assert_menu(cx, &app, &state, window, "Issue composer");
    drop(state);
    unmount(cx, app, window);
}

pub fn scenario_input_code_context_menus(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    for width in [1000., 940.] {
        editor(cx, width);
        conflict(cx, width);
        composer(cx, width);
    }
    eprintln!("[gui-e2e] PASS input_code_context_menus: actual Editor/Conflict/Issue menus, live JA/EN/JA at 1000/940x720");
}

pub fn scenario_command_period_overflow(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::{settings, theme};
    let _keys = crate::gui_isolation::SavedKeys::keep(&["toolbar_display"]);
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    settings::set_toolbar_display(settings::ToolbarDisplay::IconAndText);
    theme::set_zoom(1.67);
    cx.update(theme::sync_gpui_component_theme);
    let fixture = build_fixture();
    let (app, window) = mount_at(cx, fixture.path(), 1000.);
    app.update(cx, |app, cx| {
        app.select(0);
        app.view_mut().status_summary.repo_name = "long-repository-name".repeat(6);
        app.view_mut().status_summary.branch = "feature/long-branch-name".repeat(6);
        cx.notify();
    });
    paint(cx, window);
    crate::toolbar_overflow::open_keyboard_overflow(cx, &app, window);
    assert!(e2e::control_bounds(window.window_id(), "tb-overflow-menu").is_some());
    let before = cx.read(|cx| {
        (
            app.read(cx).ui().selected,
            app.read(cx).ui().main_diff.is_some(),
        )
    });
    crate::keyboard_nav::keys(cx, window, "cmd-.");
    e2e::clear_control_bounds(window.window_id(), "tb-overflow-menu");
    paint(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "tb-overflow-menu").is_none(),
        "Command-Period must dismiss only the focused overflow PopupMenu"
    );
    assert_eq!(
        cx.read(|cx| (
            app.read(cx).ui().selected,
            app.read(cx).ui().main_diff.is_some()
        )),
        before,
        "overflow cancellation must preserve the background selection and diff"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS command_period_overflow");
}

pub fn scenario_command_period_settings_select(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::{settings, theme};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    settings::write_setting("settings_pane", Some("0"));
    theme::set_zoom(1.);
    cx.update(theme::sync_gpui_component_theme);
    let fixture = build_fixture();
    let (app, window) = mount_at(cx, fixture.path(), 1000.);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
    })
    .unwrap();
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.read(cx)
            .theme_select
            .as_ref()
            .unwrap()
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "enter");
    paint(cx, window);
    assert_eq!(
        kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
            .unwrap()
            .expanded,
        Some(true)
    );
    let before = theme::theme().slug.to_string();
    crate::keyboard_nav::keys(cx, window, "cmd-.");
    paint(cx, window);
    assert!(
        cx.read(|cx| matches!(
            app.read(cx).menu_overlay,
            Some(kagi::ui::commands::MenuOverlay::Settings)
        )),
        "Command-Period in the theme dropdown must not close Settings"
    );
    assert_eq!(
        kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
            .unwrap()
            .expanded,
        Some(false),
        "Command-Period must close only the focused Select dropdown"
    );
    assert_eq!(theme::theme().slug, before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS command_period_settings_select");
}

fn compact_input_cancel(cx: &mut VisualTestAppContext, key: &str) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    kagi_ui_core::theme::set_zoom(1.);
    cx.update(kagi_ui_core::theme::sync_gpui_component_theme);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("README.md"),
        "unsaved compact changes\n",
    )
    .unwrap();
    let (app, window) = mount_at(cx, fixture.path(), 940.);
    for _ in 0..4 {
        paint(cx, window);
    }
    crate::compact_layout::open_wip(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "compact-composer").is_some());
    let panel = cx.read(|cx| app.read(cx).ui().commit_panel.clone().unwrap());
    let title = cx.read(|cx| panel.read(cx).title_input.clone().unwrap());
    let stack = cx.read(|cx| app.read(cx).toast_stack.clone().unwrap());
    stack.update(cx, |stack, cx| {
        stack.push_notify(kagi::ui::ToastKind::Error, "Background notice", cx);
    });
    cx.update_window(window, |_, window, cx| {
        title.update(cx, |input, cx| {
            input.set_value("Retained compact draft", window, cx);
        });
        title.read(cx).focus_handle(cx).focus(window, cx);
    })
    .unwrap();
    paint(cx, window);
    let before = cx.read(|cx| {
        (
            app.read(cx).ui().selected,
            app.read(cx).ui().main_diff.is_some(),
        )
    });
    crate::keyboard_nav::keys(cx, window, key);
    e2e::clear_control_bounds(window.window_id(), "compact-composer");
    paint(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "compact-composer").is_none(),
        "{key} in the composer Input must dismiss its Popover before background notices"
    );
    cx.read(|cx| {
        assert!(
            stack
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| toast.message.as_ref() == "Background notice"
                    && toast.dismissing.is_none()),
            "{key} must leave background notices unchanged"
        );
        assert_eq!(title.read(cx).value().as_ref(), "Retained compact draft");
        assert_eq!(
            (
                app.read(cx).ui().selected,
                app.read(cx).ui().main_diff.is_some()
            ),
            before
        );
        assert_eq!(app.read(cx).ui().commit_panel.as_ref().unwrap(), &panel);
    });
    drop(title);
    drop(panel);
    drop(stack);
    unmount(cx, app, window);
}

pub fn scenario_compact_input_escape(cx: &mut VisualTestAppContext) {
    compact_input_cancel(cx, "escape");
    eprintln!("[gui-e2e] PASS compact_input_escape");
}

pub fn scenario_compact_input_command_period(cx: &mut VisualTestAppContext) {
    compact_input_cancel(cx, "cmd-.");
    eprintln!("[gui-e2e] PASS compact_input_command_period");
}
