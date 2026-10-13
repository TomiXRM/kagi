//! #1438–#1440: native menus share localization, focused undo and editor Save.
use crate::macos::{build_fixture, git, open_offscreen, unmount};
use crate::pr_viewed::paint;
use gpui::{px, size, Action, AnyWindowHandle, Entity, Focusable, MenuItem, VisualTestAppContext};
use kagi::ui::{commands, e2e, window_menu, KagiApp, SaveEditorFile};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{EditorWorkspaceView, MiddlePaneTab, RightPaneTab};
use std::{cell::RefCell, rc::Rc};
#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn item(cx: &mut VisualTestAppContext, section: usize, at: usize) -> Box<dyn Action> {
    match &cx.read(commands::build_menus)[section].items[at] {
        MenuItem::Action { action, .. } => action.boxed_clone(),
        _ => panic!("expected native action"),
    }
}
fn action_label(cx: &mut VisualTestAppContext, section: usize, at: usize) -> String {
    match &cx.read(commands::build_menus)[section].items[at] {
        MenuItem::Action { name, .. } => name.to_string(),
        _ => panic!("expected native action label"),
    }
}
fn dispatch(cx: &mut VisualTestAppContext, win: AnyWindowHandle, action: Box<dyn Action>) {
    cx.update_window(win, |_, w, cx| w.dispatch_action(action, cx))
        .unwrap();
    paint(cx, win);
}
fn available(cx: &mut VisualTestAppContext, win: AnyWindowHandle, action: &dyn Action) -> bool {
    cx.update_window(win, |_, w, cx| w.is_action_available(action, cx))
        .unwrap()
}
fn save_action(cx: &mut VisualTestAppContext) -> Box<dyn Action> {
    cx.read(commands::build_menus)[1]
        .items
        .iter()
        .find_map(|i| match i {
            MenuItem::Action { action, name, .. } if action.name() == SaveEditorFile.name() => {
                assert_eq!(name.as_ref(), Msg::MenuSave.t());
                Some(action.boxed_clone())
            }
            _ => None,
        })
        .expect("File menu exposes existing SaveEditorFile action")
}
const LOCALIZED_MENUS: [(Lang, [&str; 5], &str, &str, &str); 2] = [
    (
        Lang::En,
        ["File", "Edit", "Repository", "Window", "Help"],
        "Settings…",
        "Open Repository…",
        "Undo",
    ),
    (
        Lang::Ja,
        ["ファイル", "編集", "リポジトリ", "ウインドウ", "ヘルプ"],
        "設定…",
        "リポジトリを開く…",
        "取り消す",
    ),
];

pub fn scenario_menus_localized(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let old = i18n::lang();
    let mut identities = None;
    for (lang, titles, settings, open, undo) in LOCALIZED_MENUS {
        i18n::set_lang(lang);
        let menus = commands::menus_for_state(window_menu::ViewMenuState::default());
        for (at, title) in [1, 2, 4, 7, 8].into_iter().zip(titles) {
            assert_eq!(
                menus[at].name.as_ref(),
                title,
                "localized native menu heading"
            );
        }
        let names = |section: usize| {
            menus[section]
                .items
                .iter()
                .filter_map(|i| match i {
                    MenuItem::Action { name, .. } => Some(name.as_ref()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(names(0).contains(&settings));
        assert!(names(1).contains(&open));
        assert_eq!(names(2)[0], undo);
        let actions = menus
            .iter()
            .flat_map(|m| m.items.iter())
            .filter_map(|i| match i {
                MenuItem::Action { action, .. } => Some(action.name()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(previous) = &identities {
            assert_eq!(&actions, previous);
        }
        identities = Some(actions);
        cx.update(commands::publish_menus);
        assert_eq!(
            commands::effective_keystroke("file.openRepository").as_deref(),
            Some("secondary-o")
        );
    }
    i18n::set_lang(old);
    cx.update(commands::publish_menus);
}
struct Mounted {
    app: Entity<KagiApp>,
    win: AnyWindowHandle,
    editor: Entity<EditorWorkspaceView>,
    input: Entity<gpui_component::input::InputState>,
}
fn wait_editor(
    cx: &mut VisualTestAppContext,
    m: &Mounted,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, m.win);
        if cx.read(|cx| ready(m.editor.read(cx))) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "production editor read finished"
        );
        crate::macos::drain_native_events();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn live(cx: &mut VisualTestAppContext, repo: &std::path::Path, dimensions: (f32, f32)) -> Mounted {
    let state = e2e::app_state(repo).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let win = open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |w, cx| e2e::mount_root(state, w, cx, &output),
    )
    .into();
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |a, cx| {
        a.set_window_active_for_e2e(true, cx);
        a.open_editor_workspace(cx);
    });
    let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().unwrap());
    editor.update(cx, |v, cx| v.open_tab("menu.txt".into(), cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, win);
        if cx.read(|cx| !editor.read(cx).file_loading && editor.read(cx).editor.is_some()) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "editor load finished");
        crate::macos::drain_native_events();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
    cx.update_window(win, |_, w, cx| {
        w.focus(&input.read(cx).focus_handle(cx), cx)
    })
    .unwrap();
    paint(cx, win);
    Mounted {
        app,
        win,
        editor,
        input,
    }
}
fn matrix(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Mounted, &std::path::Path),
) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom", "sidebar_visible"]);
    let old_lang = i18n::lang();
    let old_zoom = theme::zoom();
    let old_theme = theme::theme().slug.to_string();
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for lang in [Lang::En, Lang::Ja] {
                i18n::set_lang(lang);
                theme::set_zoom(zoom);
                cx.update(|cx| {
                    theme::set_active(if lang == Lang::En {
                        "apple-light"
                    } else {
                        "apple-dark"
                    });
                    theme::sync_gpui_component_theme(cx);
                });
                let fixture = build_fixture();
                std::fs::write(fixture.path().join("menu.txt"), "original\n").unwrap();
                git(fixture.path(), &["add", "menu.txt"]);
                git(fixture.path(), &["commit", "-m", "menu fixture"]);
                let m = live(cx, fixture.path(), dimensions);
                check(cx, &m, fixture.path());
                let Mounted {
                    app,
                    win,
                    editor,
                    input,
                } = m;
                drop(input);
                drop(editor);
                unmount(cx, app, win);
            }
        }
    }
    i18n::set_lang(old_lang);
    theme::set_zoom(old_zoom);
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
}
fn dirty(cx: &mut VisualTestAppContext, m: &Mounted, text: &str) {
    cx.simulate_keystrokes(m.win, "cmd-a");
    cx.simulate_keystrokes(m.win, text);
    paint(cx, m.win);
    assert!(cx.read(|cx| m.editor.read(cx).dirty));
}
fn snapshot(cx: &mut VisualTestAppContext, m: &Mounted) {
    cx.update_window(m.win, |_, w, cx| {
        m.editor
            .update(cx, |v, cx| v.set_right_tab(RightPaneTab::History, w, cx))
    })
    .unwrap();
    wait_editor(cx, m, |v| v.history.is_some() && !v.history_loading());
    let hash = cx.read(|cx| {
        m.editor.read(cx).history.as_ref().unwrap().entries[0]
            .commit
            .as_ref()
            .unwrap()
            .full_hash
            .clone()
    });
    cx.update_window(m.win, |_, w, cx| {
        m.editor.update(cx, |v, cx| {
            v.set_middle_tab(MiddlePaneTab::Snapshot, cx);
            v.select_history_commit(hash, w, cx);
        })
    })
    .unwrap();
    wait_editor(cx, m, |v| v.snapshot.is_some() && !v.snapshot_loading());
    root(cx, m);
}
fn root(cx: &mut VisualTestAppContext, m: &Mounted) {
    cx.update_window(m.win, |_, w, cx| {
        let focus = m.app.read(cx).root_focus.clone().unwrap();
        w.focus(&focus, cx);
    })
    .unwrap();
    paint(cx, m.win);
}
pub fn scenario_edit_menu_undo_redo(cx: &mut VisualTestAppContext) {
    let _ports = crate::gui_isolation::PortStore::keep();
    matrix(cx, |cx, m, repo| {
        let head = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
        text_undo_redo(cx, m);
        assert_eq!(git_fixture::git_output(repo, &["rev-parse", "HEAD"]), head);
        snapshot_undo_redo(cx, m);
        assert_eq!(git_fixture::git_output(repo, &["rev-parse", "HEAD"]), head);
        m.editor.update(cx, |v, cx| v.reload_from_disk(cx));
        wait_editor(cx, m, |v| !v.file_loading && !v.dirty);
        m.app.update(cx, |a, cx| a.show_graph_mode(cx));
        root(cx, m);
        history_plans(cx, m, repo);
    });
}

fn text_undo_redo(cx: &mut VisualTestAppContext, m: &Mounted) {
    dirty(cx, m, "x");
    let undo = item(cx, 2, 0);
    assert!(
        available(cx, m.win, undo.as_ref()),
        "Edit Undo reaches focused dirty Input"
    );
    dispatch(cx, m.win, undo);
    assert_eq!(
        cx.read(|cx| m.input.read(cx).value().to_string()),
        "original\n"
    );
    let redo = item(cx, 2, 1);
    dispatch(cx, m.win, redo);
    assert_eq!(cx.read(|cx| m.input.read(cx).value().to_string()), "x");
    cx.simulate_keystrokes(m.win, "cmd-z");
    paint(cx, m.win);
    assert_eq!(
        cx.read(|cx| m.input.read(cx).value().to_string()),
        "original\n"
    );
    cx.simulate_keystrokes(m.win, "cmd-shift-z");
    paint(cx, m.win);
    assert_eq!(cx.read(|cx| m.input.read(cx).value().to_string()), "x");
}

fn snapshot_undo_redo(cx: &mut VisualTestAppContext, m: &Mounted) {
    snapshot(cx, m);
    for at in [0, 1] {
        let action = item(cx, 2, at);
        assert!(!available(cx, m.win, action.as_ref()));
        dispatch(cx, m.win, action);
        assert!(cx.read(|cx| m.app.read(cx).history_modal().is_none()));
    }
    cx.simulate_keystrokes(m.win, "cmd-z cmd-shift-z");
    paint(cx, m.win);
}
fn history_plans(cx: &mut VisualTestAppContext, m: &Mounted, repo: &std::path::Path) {
    let before = git_fixture::git_output(repo, &["rev-parse", "HEAD~1"]);
    let after = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    m.app.update(cx, |a, cx| {
        a.ui_mut().unwrap().operation_history = Default::default();
        a.ui_mut().unwrap().history_seed_attempted = true;
        a.record_history(
            kagi_git::OperationKind::Commit,
            "main",
            kagi_git::CommitId(before.clone()),
            kagi_git::CommitId(after.clone()),
            "menu fixture",
        );
        cx.notify();
    });
    root(cx, m);
    for (at, key, is_undo) in [(0, "cmd-z", true), (1, "cmd-shift-z", false)] {
        if !is_undo {
            git(repo, &["update-ref", "refs/heads/main", &before]);
            m.app.update(cx, |a, cx| {
                a.ui_mut().unwrap().operation_history.undo();
                cx.notify();
            });
            root(cx, m);
        }
        history_move_plan(cx, m, at, key, is_undo);
    }
    terminal_history_disabled(cx, m);
    root(cx, m);
    m.app.update(cx, |a, cx| {
        a.ui_mut().unwrap().operation_history = Default::default();
        cx.notify();
    });
    paint(cx, m.win);
    for at in [0, 1] {
        let action = item(cx, 2, at);
        assert!(
            !available(cx, m.win, action.as_ref()),
            "no history disables native item"
        );
    }
}

fn history_move_plan(
    cx: &mut VisualTestAppContext,
    m: &Mounted,
    at: usize,
    key: &str,
    is_undo: bool,
) {
    let action = item(cx, 2, at);
    assert!(available(cx, m.win, action.as_ref()));
    let label = action_label(cx, 2, at);
    assert!(label.contains("menu fixture"));
    dispatch(cx, m.win, action);
    assert_eq!(
        action_label(cx, 2, at),
        label,
        "modal availability does not rebuild history menu labels"
    );
    let disabled = item(cx, 2, at);
    assert!(!available(cx, m.win, disabled.as_ref()));
    let entry = cx.read(|cx| {
        let modal = m.app.read(cx).history_modal().unwrap();
        assert_eq!(modal.is_undo, is_undo);
        modal.entry.clone()
    });
    m.app.update(cx, |a, cx| {
        a.clear_history_modal();
        cx.notify();
    });
    root(cx, m);
    assert_eq!(action_label(cx, 2, at), label);
    cx.simulate_keystrokes(m.win, key);
    paint(cx, m.win);
    assert_eq!(action_label(cx, 2, at), label);
    cx.read(|cx| {
        let modal = m.app.read(cx).history_modal().unwrap();
        assert_eq!(modal.entry.before, entry.before);
        assert_eq!(modal.entry.after, entry.after);
        assert_eq!(modal.is_undo, is_undo);
    });
    m.app.update(cx, |a, cx| {
        a.clear_history_modal();
        cx.notify();
    });
    root(cx, m);
    assert_eq!(action_label(cx, 2, at), label);
}

struct TerminalShell;
impl Drop for TerminalShell {
    fn drop(&mut self) {
        KagiApp::set_terminal_shell_for_e2e(None);
    }
}
fn terminal_history_disabled(cx: &mut VisualTestAppContext, m: &Mounted) {
    KagiApp::set_terminal_shell_for_e2e(Some("/bin/cat".into()));
    let _shell = TerminalShell;
    cx.update_window(m.win, |_, w, cx| {
        m.app.update(cx, |a, cx| {
            a.bottom_panel_open = true;
            a.bottom_tab = kagi::ui::BottomTab::Terminal;
            a.ensure_terminal(w, cx);
            cx.notify();
        })
    })
    .unwrap();
    paint(cx, m.win);
    for at in [0, 1] {
        let action = item(cx, 2, at);
        assert!(
            !available(cx, m.win, action.as_ref()),
            "terminal cannot target repository history"
        );
        dispatch(cx, m.win, action);
    }
    assert!(cx.read(|cx| m.app.read(cx).history_modal().is_none()));
}
pub fn scenario_file_menu_save(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, m, repo| {
        assert!(
            !available(cx, m.win, &SaveEditorFile),
            "clean buffer is unavailable"
        );
        dirty(cx, m, "x");
        let save = save_action(cx);
        assert!(available(cx, m.win, save.as_ref()));
        cx.update_window(m.win, |_, w, _| {
            let key = w.keystroke_text_for(&SaveEditorFile);
            assert!(
                key.contains('⌘') && key.to_uppercase().contains('S'),
                "native Save glyph is ⌘S: {key}"
            );
        })
        .unwrap();
        dispatch(cx, m.win, save);
        cx.run_until_parked();
        paint(cx, m.win);
        assert_eq!(std::fs::read_to_string(repo.join("menu.txt")).unwrap(), "x");
        assert!(!cx.read(|cx| m.editor.read(cx).dirty));
        dirty(cx, m, "y");
        cx.simulate_keystrokes(m.win, "cmd-s");
        cx.run_until_parked();
        paint(cx, m.win);
        assert_eq!(std::fs::read_to_string(repo.join("menu.txt")).unwrap(), "y");
        assert!(!cx.read(|cx| m.editor.read(cx).dirty));
        unavailable_save(cx, m, repo);
    });
}

fn unavailable_save(cx: &mut VisualTestAppContext, m: &Mounted, repo: &std::path::Path) {
    dirty(cx, m, "z");
    snapshot(cx, m);
    assert!(
        !available(cx, m.win, &SaveEditorFile),
        "snapshot cannot save hidden dirty buffer"
    );
    dispatch(cx, m.win, SaveEditorFile.boxed_clone());
    cx.simulate_keystrokes(m.win, "cmd-s");
    paint(cx, m.win);
    assert_eq!(std::fs::read_to_string(repo.join("menu.txt")).unwrap(), "y");
    m.editor.update(cx, |v, cx| v.reload_from_disk(cx));
    wait_editor(cx, m, |v| !v.file_loading && !v.dirty);
    m.editor.update(cx, |v, cx| {
        v.close_tab_now(std::path::Path::new("menu.txt"), cx)
    });
    paint(cx, m.win);
    assert!(
        !available(cx, m.win, &SaveEditorFile),
        "no target is unavailable"
    );
    m.app.update(cx, |a, cx| a.show_graph_mode(cx));
    root(cx, m);
    assert!(
        !available(cx, m.win, &SaveEditorFile),
        "Graph cannot save retained Editor"
    );
}
