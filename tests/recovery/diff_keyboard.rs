//! #1233 / #1234: measured diff controls and native simulated keyboard paths.
use crate::keyboard_nav::keys;
use crate::macos::{git, mount, unmount};
use crate::recovery_operations::wait_idle;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::i18n::Msg;
use kagi::ui::{e2e, theme, KagiApp};

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q", "-b", "main"]);
    let base: String = (0..100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(temp.path().join("f.txt"), &base).unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "base"]);
    std::fs::write(temp.path().join("f.txt"), base.replace("line 2\n", "TWO\n")).unwrap();
    temp
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    e2e::diff_controls::clear_frame();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index: 0 }, cx)
    });
    cx.run_until_parked();
}

fn surface(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (u64, usize) {
    use std::hash::{Hash, Hasher};
    cx.read(|cx| {
        let pane = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (pane.view.title.as_ref(), pane.view.rows.len()).hash(&mut hash);
        (hash.finish(), pane.view.rows.len())
    })
}

fn selected_rows(key: u64) -> usize {
    let selection: gpui::Fill = theme::selection_overlay().into();
    e2e::diff_controls::painted_rows(key)
        .iter()
        .filter(|(_, fill)| fill.as_ref() == Some(&selection))
        .count()
}

pub fn scenario_diff_keyboard_selection(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split", "lang"]);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        for split in [false, true] {
            theme::set_diff_split(split);
            selection_cell(cx);
        }
    }
    eprintln!("[gui-e2e] PASS diff_keyboard_selection EN/JA unified/split");
}

fn selection_cell(cx: &mut VisualTestAppContext) {
    let temp = fixture();
    let repo = temp.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    open(cx, &app);
    draw(cx, window);
    let button = e2e::control_bounds(window.window_id(), "main-diff-hunk-stage-0").unwrap();
    cx.simulate_click(
        window,
        gpui::point(button.left() - gpui::px(100.), button.center().y),
        gpui::Modifiers::none(),
    );
    range_keys(cx, &app, window);
    assert!(
        cx.read(|cx| app.read(cx).ui().main_diff.is_some()),
        "selection Escape preserves diff"
    );
    caret_scroll(cx, &app, window, &repo);
    unmount(cx, app, window);
}

fn range_keys(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    keys(cx, window, "escape shift-down shift-down shift-down");
    draw(cx, window);
    let (key, count) = surface(cx, app);
    assert_eq!(
        selected_rows(key),
        3,
        "diff_keyboard_selection: Shift+Down x3 paints three content rows"
    );
    keys(cx, window, "cmd-c");
    assert_eq!(clipboard(cx).lines().count(), 3);
    keys(cx, window, "shift-up");
    draw(cx, window);
    assert_eq!(
        selected_rows(key),
        2,
        "Shift+Up retracts the anchored selection"
    );
    keys(cx, window, "down");
    draw(cx, window);
    assert_eq!(selected_rows(key), 1, "caret movement collapses selection");
    keys(cx, window, "shift-cmd-up cmd-c");
    draw(cx, window);
    assert_eq!(selected_rows(key), 3, "extend to the first row");
    assert_eq!(clipboard(cx).lines().count(), 3);
    keys(cx, window, "shift-cmd-down cmd-c");
    draw(cx, window);
    assert_eq!(
        clipboard(cx).lines().count(),
        count - 3,
        "boundary extension retains anchor"
    );
    keys(cx, window, "cmd-a cmd-c");
    draw(cx, window);
    assert_eq!(selected_rows(key), count - 1, "all content rows selected");
    assert_eq!(clipboard(cx).lines().count(), count - 1);
    let toast_count = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .len()
    });
    assert!(
        toast_count > 0,
        "copy notification is present before Escape"
    );
    keys(cx, window, "escape");
    draw(cx, window);
    assert_eq!(
        selected_rows(key),
        0,
        "Escape clears selection before copy toast"
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .len()),
        toast_count,
        "selection Escape does not dismiss a notification"
    );
}

fn caret_scroll(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    let long: String = (0..100).map(|i| format!("changed {i}\n")).collect();
    std::fs::write(repo.join("f.txt"), long).unwrap();
    open(cx, app);
    draw(cx, window);
    let row = e2e::control_bounds(window.window_id(), "main-diff-hunk-row-0").unwrap();
    cx.simulate_click(
        window,
        gpui::point(row.left() + gpui::px(100.), row.center().y),
        gpui::Modifiers::none(),
    );
    keys(cx, window, "escape shift-cmd-down");
    draw(cx, window);
    let (_, total) = surface(cx, app);
    let tail = if theme::diff_split() { 100 } else { total - 1 };
    assert_row_visible(cx, app, tail);
    keys(cx, window, "shift-cmd-up");
    draw(cx, window);
    assert_row_visible(cx, app, 0);
    keys(cx, window, "escape");
}

fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

fn assert_row_visible(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, index: usize) {
    let (viewport, row) = cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .as_ref()
            .unwrap()
            .read(cx)
            .diff_row_bounds_for_e2e(index)
    });
    let row = row.expect("caret row measured by native list");
    assert!(
        row.top() >= viewport.top() - gpui::px(1.)
            && row.bottom() <= viewport.bottom() + gpui::px(1.),
        "caret row {index} scrolls into view: {row:?} within {viewport:?}"
    );
}

pub fn scenario_diff_hunk_buttons(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split", "lang"]);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        hunk_cell(cx);
    }
    eprintln!("[gui-e2e] PASS diff_hunk_buttons");
}

fn hunk_cell(cx: &mut VisualTestAppContext) {
    theme::set_diff_split(false);
    let temp = fixture();
    let repo = temp.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    open(cx, &app);
    draw(cx, window);
    hunk_attributes(window, Msg::DiffStageHunk);
    tab_to_hunk(cx, window);
    keys(cx, window, "enter");
    wait_idle(cx, &app);
    assert!(
        git_index(&repo).contains("TWO"),
        "Enter activates the existing approved hunk path"
    );
    theme::set_diff_split(true);
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Staged { index: 0 }, cx)
    });
    draw(cx, window);
    hunk_attributes(window, Msg::DiffUnstageHunk);
    tab_to_hunk(cx, window);
    keys(cx, window, "space");
    wait_idle(cx, &app);
    assert!(
        !git_index(&repo).contains("TWO"),
        "Space unstages the approved hunk"
    );
    unmount(cx, app, window);
}

fn hunk_attributes(window: AnyWindowHandle, label: Msg) {
    let header = e2e::control_bounds(window.window_id(), "main-diff-hunk-row-0").unwrap();
    assert_eq!(
        header.size.height,
        gpui::px(22.5),
        "preserve the measured before header height"
    );
    eprintln!(
        "[gui-e2e] diff_hunk_buttons geometry {:?}: header={:?}",
        kagi::ui::i18n::lang(),
        header.size
    );
    assert_eq!(
        e2e::diff_controls::button_attributes(0),
        Some((gpui::Role::Button, label.t().to_owned())),
        "diff_hunk_buttons: hunk action has Button role and localized AX name"
    );
}

fn tab_to_hunk(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let header = e2e::control_bounds(window.window_id(), "main-diff-hunk-row-0").unwrap();
    let before = e2e::control_bounds(window.window_id(), "main-diff-hunk-stage-0").unwrap();
    let mut reached = false;
    for _ in 0..100 {
        keys(cx, window, "tab");
        draw(cx, window);
        e2e::diff_controls::take_focus();
        keys(cx, window, "f19");
        if e2e::diff_controls::take_focus() == Some(0) {
            reached = true;
            break;
        }
    }
    assert!(reached, "hunk action is reachable by Tab");
    let focused = e2e::control_bounds(window.window_id(), "main-diff-hunk-stage-0").unwrap();
    assert_eq!(
        before, focused,
        "keyboard focus leaves hunk geometry unchanged"
    );
    assert_eq!(
        header,
        e2e::control_bounds(window.window_id(), "main-diff-hunk-row-0").unwrap(),
        "focus leaves the complete hunk row unchanged"
    );
}

fn git_index(repo: &std::path::Path) -> String {
    let out = std::process::Command::new("git")
        .args(["show", ":f.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}
