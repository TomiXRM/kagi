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
    cx.read(|cx| {
        let pane = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
        (pane.selection_token_for_e2e(), pane.view.rows.len())
    })
}

fn selected_rows(key: u64) -> usize {
    let selection: gpui::Fill = theme::selection_overlay().into();
    e2e::diff_controls::painted_rows(key)
        .iter()
        .filter(|(ix, fill)| *ix != 0 && fill.as_ref() == Some(&selection))
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

pub fn scenario_split_diff_keyboard_order(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    theme::set_diff_split(true);
    let temp = fixture();
    std::fs::write(
        temp.path().join("f.txt"),
        (0..100)
            .filter(|i| !(2..7).contains(i))
            .map(|i| {
                if i == 7 {
                    format!("replacement a\nreplacement b\nreplacement c\nline {i}\n")
                } else {
                    format!("line {i}\n")
                }
            })
            .collect::<String>(),
    )
    .unwrap();
    let repo = temp.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    open(cx, &app);
    draw(cx, window);
    let (key, _) = surface(cx, &app);
    tab_to_body(cx, window, key);
    for _ in 0..7 {
        keys(cx, window, "down");
    }
    keys(cx, window, "cmd-c");
    assert_eq!(clipboard(cx), "line 6\n", "last left replacement row");
    keys(cx, window, "down");
    keys(cx, window, "cmd-c");
    assert_eq!(
        clipboard(cx),
        "line 7\n",
        "Down from last left row moves to the next visual row, never back up"
    );
    keys(cx, window, "up");
    keys(cx, window, "cmd-c");
    assert_eq!(clipboard(cx), "line 6\n");
    keys(cx, window, "shift-down cmd-c");
    assert_eq!(
        clipboard(cx),
        "line 6\nreplacement a\nreplacement b\nreplacement c\nline 7\n",
        "Shift preserves the unified anchor and code-only copy range"
    );
    keys(cx, window, "home");
    draw(cx, window);
    assert_header_caret(key, true);
    keys(cx, window, "end cmd-c");
    assert_eq!(clipboard(cx), "line 9\n");
    keys(cx, window, "pageup");
    draw(cx, window);
    assert_header_caret(key, true);
    keys(cx, window, "pagedown cmd-c");
    assert_eq!(clipboard(cx), "line 9\n");
    keys(cx, window, "home");
    draw(cx, window);
    let right = e2e::control_bounds(window.window_id(), "split-cell-17").unwrap();
    cx.simulate_click(window, right.center(), gpui::Modifiers::none());
    keys(cx, window, "shift-down shift-down shift-down cmd-c");
    assert_eq!(
        clipboard(cx),
        "replacement a\nreplacement b\nreplacement c\nline 7\n",
        "right-affinity Shift+Down skips opposite-side fillers without shrinking"
    );
    for expected in [
        "replacement c\n",
        "replacement b\n",
        "replacement a\n",
        "line 1\n",
    ] {
        keys(cx, window, "up cmd-c");
        assert_eq!(clipboard(cx), expected, "right-affinity Up reverses Down");
    }
    unmount(cx, app, window);
    split_page_wrapped(cx);
}

fn split_page_wrapped(cx: &mut VisualTestAppContext) {
    let temp = fixture();
    let contents: String = (0..100)
        .map(|i| {
            if i == 0 {
                format!("wrapped {}\n", "long content ".repeat(40))
            } else {
                format!("replacement {i}\n")
            }
        })
        .collect();
    std::fs::write(temp.path().join("f.txt"), contents).unwrap();
    let repo = temp.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    open(cx, &app);
    draw(cx, window);
    let (key, _) = surface(cx, &app);
    tab_to_body(cx, window, key);
    let last = cx.read(|cx| {
        let pane = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
        let (viewport, _) = pane.diff_row_bounds_for_e2e(0);
        (0..101)
            .take_while(|index| {
                pane.diff_row_bounds_for_e2e(*index)
                    .1
                    .is_some_and(|row| row.bottom() <= viewport.bottom())
            })
            .last()
            .unwrap()
    });
    keys(cx, window, "pagedown cmd-c");
    assert_eq!(
        clipboard(cx),
        format!("line {}\n", last - 1),
        "PageDown lands on the last fully visible Row despite wrapped cells"
    );
    draw(cx, window);
    assert_row_visible(cx, &app, last);
    let base: String = (0..100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(temp.path().join("f.txt"), format!("{base}added only\n")).unwrap();
    open(cx, &app);
    draw(cx, window);
    keys(cx, window, "home down down down down cmd-c");
    assert_eq!(
        clipboard(cx),
        "added only\n",
        "one-sided additions remain navigable"
    );
    unmount(cx, app, window);
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
    let (key, _) = surface(cx, &app);
    tab_to_body(cx, window, key);
    keys(cx, window, "down");
    draw(cx, window);
    assert_eq!(selected_rows(key), 1, "Tab enters body; Down moves caret");
    keys(cx, window, "up");
    draw(cx, window);
    assert_header_caret(key, true);
    pointer_ring(cx, window, key);
    range_keys(cx, &app, window);
    assert!(
        cx.read(|cx| app.read(cx).ui().main_diff.is_some()),
        "selection Escape preserves diff"
    );
    caret_scroll(cx, &app, window, &repo);
    unmount(cx, app, window);
}

fn tab_to_body(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: u64) {
    for _ in 0..128 {
        keys(cx, window, "tab");
        draw(cx, window);
        if e2e::diff_controls::body_focused(key) {
            assert!(
                e2e::diff_controls::body_ring_visible(key),
                "Tab reveals the body ring"
            );
            return;
        }
    }
    panic!("diff body must be reachable by Tab without a pointer click");
}

fn assert_header_caret(key: u64, selected: bool) {
    let fill = e2e::diff_controls::painted_rows(key)
        .into_iter()
        .find(|(ix, _)| *ix == 0)
        .expect("header paint measured after its background is configured")
        .1;
    assert_eq!(
        fill == Some(theme::selection_overlay().into()),
        selected,
        "header tint follows the keyboard caret"
    );
}

fn pointer_ring(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: u64) {
    let row = e2e::control_bounds(window.window_id(), "main-diff-hunk-row-0").unwrap();
    cx.simulate_click(
        window,
        gpui::point(row.left() + gpui::px(100.), row.center().y),
        gpui::Modifiers::none(),
    );
    draw(cx, window);
    assert!(
        e2e::diff_controls::body_focused(key),
        "click retains body focus"
    );
    assert!(
        !e2e::diff_controls::body_ring_visible(key),
        "pointer click hides the ring"
    );
    assert_header_caret(key, false);
    keys(cx, window, "down");
    draw(cx, window);
    assert!(
        e2e::diff_controls::body_ring_visible(key),
        "arrow input reveals the ring"
    );
    assert_eq!(selected_rows(key), 1, "caret still responds after a click");
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
    assert_header_caret(key, false);
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
    let (old_key, old_total) = surface(cx, app);
    assert!(e2e::diff_controls::body_focused(old_key));
    std::fs::write(repo.join("f.txt"), long).unwrap();
    open(cx, app);
    draw(cx, window);
    let (key, total) = surface(cx, app);
    assert_ne!(total, old_total, "real reload changes the row count");
    assert_ne!(key, old_key, "selection token still follows the content");
    assert!(
        e2e::diff_controls::body_focused(key),
        "row-count reload retains body focus without another Tab or click"
    );
    keys(cx, window, "down cmd-c");
    draw(cx, window);
    assert_eq!(selected_rows(key), 1);
    assert_eq!(clipboard(cx), "line 0\n");
    assert!(cx.read(|cx| app.read(cx).ui().main_diff.is_some()));
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
