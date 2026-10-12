//! #1348 review: shared localized Find labels in the actual other code editors.
use crate::editor_find::{close_and_return, focus_next, names_and_help, probe, ICONS};
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_viewed::paint;
use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use gpui_component::input::InputState;
use kagi::ui::KagiApp;
use kagi_ui_core::{i18n::Lang, theme};

fn localized_bar(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    input: &Entity<InputState>,
    window: AnyWindowHandle,
    lang: Lang,
    query: &str,
) {
    let unchanged = cx.read(|cx| input.read(cx).value());
    cx.update_window(window, |_, window, cx| {
        input.read(cx).focus_handle(cx).focus(window, cx)
    })
    .unwrap();
    keys(cx, window, "cmd-f");
    cx.simulate_keystrokes(window, query);
    paint(cx, window);
    assert!(probe(cx, input).match_count > 0);
    names_and_help(cx, input, window, lang, &ICONS);
    focus_next(cx, input, window, "case");
    focus_next(cx, input, window, "mode");
    keys(cx, window, "cmd-a backspace cmd-v");
    paint(cx, window);
    assert_eq!(
        cx.read(|cx| input.read(cx).value()),
        unchanged,
        "host Find Button cannot edit hidden body/result"
    );
    assert!(probe(cx, input).match_count > 0);
    keys(cx, window, "enter");
    paint(cx, window);
    assert!(probe(cx, input).replace_mode);
    names_and_help(cx, input, window, lang, &["replace", "all"]);
    let before = probe(cx, input);
    let other = if lang == Lang::En { Lang::Ja } else { Lang::En };
    app.update(cx, |app, cx| app.set_lang(other, cx));
    paint(cx, window);
    names_and_help(cx, input, window, other, &ICONS);
    names_and_help(cx, input, window, other, &["replace", "all"]);
    let after = probe(cx, input);
    assert_eq!(
        after.match_count, before.match_count,
        "host locale switch retains query"
    );
    assert_eq!(after.replace_mode, before.replace_mode);
    assert_eq!(
        cx.read(|cx| input.read(cx).value()),
        unchanged,
        "Find only edits its own fields"
    );
    close_and_return(cx, input, window);
}

fn issue(cx: &mut VisualTestAppContext, lang: Lang) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_issue_title_for_e2e(window, cx));
    })
    .unwrap();
    cx.simulate_keystrokes(window, "Find host");
    keys(cx, window, "enter");
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_body_for_e2e("alpha alpha", window, cx)
        });
    })
    .unwrap();
    paint(cx, window);
    let input = cx
        .read(|cx| app.read(cx).issue_body_input_for_e2e())
        .expect("real New Issue body");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        localized_bar(cx, &app, &input, window, lang, "alpha")
    }));
    drop(input);
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn conflict(cx: &mut VisualTestAppContext, lang: Lang) {
    let fixture = crate::conflict_text_keyboard::fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    paint(cx, window);
    let conflict = cx
        .read(|cx| app.read(cx).ui().conflict.clone())
        .expect("real conflict view");
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(std::path::Path::new("notes.txt"));
        view.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    paint(cx, window);
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        localized_bar(cx, &app, &input, window, lang, "ours")
    }));
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

pub(super) fn exercise(cx: &mut VisualTestAppContext) {
    theme::set_zoom(1.);
    theme::set_active("apple-light");
    cx.update(theme::sync_gpui_component_theme);
    for lang in [Lang::En, Lang::Ja] {
        issue(cx, lang);
        conflict(cx, lang);
    }
    eprintln!(
        "[gui-e2e] PASS actual Issue/Conflict Find names and hover help, initial and live EN/JA"
    );
}
