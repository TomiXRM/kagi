//! #1348: observe and activate the real pinned code editor's Find/Replace bar.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, unmount};
use crate::pr_viewed::paint;
use gpui::{px, size, AnyWindowHandle, Entity, Focusable, Modifiers, Role, VisualTestAppContext};
use gpui_component::input::{InputState, SearchPanelProbe};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

const FILE: &str = "find.rs";
const TEXT: &str = "alpha Alpha alpha alpha\n";
pub(super) const ICONS: [&str; 5] = ["case", "mode", "prev", "next", "close"];

pub(super) fn probe(cx: &VisualTestAppContext, input: &Entity<InputState>) -> SearchPanelProbe {
    cx.read(|cx| {
        input
            .read(cx)
            .search_panel_probe(cx)
            .expect("actual search panel")
    })
}

fn expected(lang: Lang, id: &str) -> &'static str {
    match (lang, id) {
        (Lang::En, "case") => "Match case",
        (Lang::Ja, "case") => "大文字と小文字を区別",
        (Lang::En, "mode") => "Show replacement field",
        (Lang::Ja, "mode") => "置換欄を表示",
        (Lang::En, "prev") => "Previous match",
        (Lang::Ja, "prev") => "前の一致",
        (Lang::En, "next") => "Next match",
        (Lang::Ja, "next") => "次の一致",
        (Lang::En, "close") => "Close",
        (Lang::Ja, "close") => "閉じる",
        (Lang::En, "replace") => "Replace",
        (Lang::Ja, "replace") => "置換",
        (Lang::En, "all") => "Replace all",
        (Lang::Ja, "all") => "すべて置換",
        _ => unreachable!(),
    }
}

pub(super) fn names_and_help(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    lang: Lang,
    ids: &[&str],
) {
    for id in ids {
        let snapshot = probe(cx, input);
        let control = &snapshot.controls[id];
        assert_eq!(control.role, Role::Button);
        assert_eq!(
            control.name.as_deref(),
            Some(expected(lang, id)),
            "actual Find {id} accessible name"
        );
        let bounds = snapshot.bounds[id];
        let viewport = cx
            .update_window(window, |_, window, _| window.viewport_size())
            .unwrap();
        assert!(bounds.left() >= px(0.) && bounds.right() <= viewport.width);
        assert!(bounds.top() >= px(0.) && bounds.bottom() <= viewport.height);
        cx.read(|cx| input.read(cx).clear_search_help_probe(cx));
        cx.simulate_mouse_move(window, gpui::point(px(0.), px(0.)), None, Modifiers::none());
        paint(cx, window);
        cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
        for _ in 0..3 {
            cx.advance_clock(Duration::from_millis(400));
            paint(cx, window);
        }
        assert_eq!(
            probe(cx, input).help.get(id).map(|s| s.as_ref()),
            Some(expected(lang, id)),
            "{id} real hover constructs localized help"
        );
    }
}

pub(super) fn focus_next(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    id: &str,
) {
    cx.update_window(window, |_, window, cx| window.focus_next(cx))
        .unwrap();
    paint(cx, window);
    keys(cx, window, "f19");
    assert_eq!(
        probe(cx, input).focused_control,
        Some(id),
        "actual rendered Tab order"
    );
}

pub(super) fn close_and_return(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
) {
    keys(cx, window, "escape");
    paint(cx, window);
    let snapshot = probe(cx, input);
    assert!(
        !snapshot.open,
        "Escape closes {:?}, replace={}, matches={}",
        snapshot.focused_control, snapshot.replace_mode, snapshot.match_count
    );
    assert!(
        cx.update_window(window, |_, window, cx| input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
            .unwrap(),
        "Escape returns focus to editable code"
    );
}

fn protected_edit_keys(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
) {
    for key in [
        "cmd-a",
        "backspace",
        "delete",
        "alt-backspace",
        "alt-delete",
        "cmd-v",
        "cmd-x",
        "cmd-z",
        "cmd-shift-z",
        "left",
        "right",
        "shift-left",
        "shift-right",
        "home",
        "end",
    ] {
        keys(cx, window, key);
        paint(cx, window);
        assert_eq!(
            cx.read(|cx| input.read(cx).value().to_string()),
            TEXT,
            "Find Button owns {key}; it must not edit the hidden code buffer"
        );
    }
}

fn exercise(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    cx.update_window(window, |_, window, cx| {
        input.read(cx).focus_handle(cx).focus(window, cx)
    })
    .unwrap();
    paint(cx, window);
    let menus = cx.read(kagi::ui::commands::build_menus);
    let find = menus
        .iter()
        .flat_map(|menu| &menu.items)
        .find_map(|item| match item {
            gpui::MenuItem::Submenu(menu)
                if menu.name.as_ref() == if lang == Lang::En { "Find" } else { "検索" } =>
            {
                Some(menu)
            }
            _ => None,
        })
        .expect("Edit menu exposes Find");
    assert_eq!(
        find.items.len(),
        1,
        "only working public Find actions are offered"
    );
    let gpui::MenuItem::Action {
        action, disabled, ..
    } = &find.items[0]
    else {
        panic!("Find action")
    };
    assert!(!disabled, "focused code Input is a Find target");
    assert!(
        cx.update_window(window, |_, window, cx| window
            .is_action_available(action.as_ref(), cx))
            .unwrap(),
        "native Find validates the focused code Input"
    );
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(action.boxed_clone(), cx)
    })
    .unwrap();
    paint(cx, window);
    assert!(
        probe(cx, input).open,
        "native Find menu opens the actual search bar"
    );
    cx.simulate_keystrokes(window, "alpha");
    paint(cx, window);
    assert_eq!(probe(cx, input).match_count, 4);
    names_and_help(cx, input, window, lang, &ICONS);
    assert_eq!(probe(cx, input).controls["case"].toggled, Some(false));
    assert_eq!(probe(cx, input).controls["mode"].toggled, Some(false));
    focus_next(cx, input, window, "case");
    keys(cx, window, "cmd-a backspace");
    paint(cx, window);
    assert_eq!(
        probe(cx, input).match_count,
        4,
        "case Button must not edit its containing query Input"
    );
    keys(cx, window, "space");
    paint(cx, window);
    assert_eq!(probe(cx, input).match_count, 3);
    assert_eq!(probe(cx, input).controls["case"].toggled, Some(true));
    focus_next(cx, input, window, "mode");
    protected_edit_keys(cx, input, window);
    focus_next(cx, input, window, "prev");
    keys(cx, window, "enter");
    paint(cx, window);
    assert_eq!(probe(cx, input).match_index, 2, "Previous is not Find Next");
    focus_next(cx, input, window, "next");
    keys(cx, window, "space");
    paint(cx, window);
    assert_eq!(probe(cx, input).match_index, 0);
    focus_next(cx, input, window, "close");
    close_and_return(cx, input, window);
}

fn padding_ownership(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
) {
    keys(cx, window, "cmd-f");
    paint(cx, window);
    focus_next(cx, input, window, "case");
    focus_next(cx, input, window, "mode");
    let panel = probe(cx, input).bounds["panel"];
    let padding = panel.origin + gpui::point(px(1.), px(1.));
    cx.simulate_mouse_move(window, padding, None, Modifiers::none());
    cx.simulate_click(window, padding, Modifiers::none());
    keys(cx, window, "cmd-a backspace");
    paint(cx, window);
    assert_eq!(
        probe(cx, input).match_count,
        0,
        "padding click focuses the real Find field"
    );
    cx.simulate_keystrokes(window, "alpha");
    paint(cx, window);
    assert_eq!(probe(cx, input).match_count, 3);
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        TEXT,
        "padding never edits the hidden buffer"
    );
    close_and_return(cx, input, window);
}

fn replacement(
    cx: &mut VisualTestAppContext,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    keys(cx, window, "cmd-f");
    paint(cx, window);
    focus_next(cx, input, window, "case");
    focus_next(cx, input, window, "mode");
    keys(cx, window, "enter");
    paint(cx, window);
    assert!(probe(cx, input).replace_mode);
    assert_eq!(probe(cx, input).controls["mode"].toggled, Some(true));
    names_and_help(cx, input, window, lang, &["replace", "all"]);
    cx.simulate_keystrokes(window, "beta");
    focus_next(cx, input, window, "replace");
    keys(cx, window, "space");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "beta Alpha alpha alpha\n"
    );
    // Input history groups edits within one real-time second. Separate the two
    // user operations without changing that existing grouping policy.
    std::thread::sleep(Duration::from_millis(1100));
    focus_next(cx, input, window, "all");
    keys(cx, window, "enter");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "beta Alpha beta beta\n"
    );
    close_and_return(cx, input, window);
    keys(cx, window, "cmd-z");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "beta Alpha alpha alpha\n",
        "replace all remains undoable"
    );
    keys(cx, window, "cmd-z");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        TEXT,
        "replace remains undoable"
    );
}

fn retained_locale_and_close(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    keys(cx, window, "cmd-f");
    paint(cx, window);
    let before = probe(cx, input);
    let other = if lang == Lang::En { Lang::Ja } else { Lang::En };
    app.update(cx, |app, cx| app.set_lang(other, cx));
    paint(cx, window);
    names_and_help(cx, input, window, other, &ICONS);
    names_and_help(cx, input, window, other, &["replace", "all"]);
    let after = probe(cx, input);
    assert_eq!(
        after.match_count, before.match_count,
        "live locale retains query"
    );
    assert_eq!(after.case_sensitive, before.case_sensitive);
    assert_eq!(after.replace_mode, before.replace_mode);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    paint(cx, window);
    close_and_return(cx, input, window);
    keys(cx, window, "cmd-f");
    paint(cx, window);
    for id in ICONS {
        focus_next(cx, input, window, id);
    }
    keys(cx, window, "enter");
    paint(cx, window);
    assert!(
        !probe(cx, input).open,
        "Close Button activates from the keyboard"
    );
    assert!(cx
        .update_window(window, |_, window, cx| input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window))
        .unwrap());
}

fn mount(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
    slug: &str,
    lang: Lang,
    zoom: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    i18n::set_lang(lang);
    theme::set_zoom(zoom);
    let state = e2e::app_state(repo).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow_mut().take().unwrap();
    app.update(cx, |app, cx| {
        app.set_window_active_for_e2e(true, cx);
        app.set_theme(slug, cx);
        app.open_editor_workspace(cx);
    });
    (app, window)
}

fn leg(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
    slug: &str,
    lang: Lang,
    zoom: f32,
) {
    let (app, window) = mount(cx, repo, dimensions, slug, lang, zoom);
    let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().unwrap());
    editor.update(cx, |view, cx| view.open_tab(FILE.into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| editor.read(cx).editor.is_some() && !editor.read(cx).file_loading) {
            break;
        }
        assert!(Instant::now() < deadline, "real editor file load settles");
        crate::macos::drain_native_events();
        std::thread::sleep(Duration::from_millis(2));
    }
    let input = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        exercise(cx, &input, window, lang);
        padding_ownership(cx, &input, window);
        replacement(cx, &input, window, lang);
        retained_locale_and_close(cx, &app, &input, window, lang);
        assert_eq!(
            std::fs::read_to_string(repo.join(FILE)).unwrap(),
            TEXT,
            "find replacement never saves without approval"
        );
    }));
    drop(input);
    drop(editor);
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

pub fn scenario_editor_find_controls_a11y(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    std::fs::write(fixture.path().join(FILE), TEXT).unwrap();
    for dimensions in [(940., 660.), (1000., 720.), (1392., 883.)] {
        for slug in ["apple-light", "apple-dark"] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    leg(cx, fixture.path(), dimensions, slug, lang, zoom);
                }
            }
        }
    }
    crate::editor_find_hosts::exercise(cx);
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
    eprintln!("[gui-e2e] PASS editor_find_controls_a11y");
}
