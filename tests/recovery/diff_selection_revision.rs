//! #1444: selection belongs to the displayed source revision and focused body.
use crate::keyboard_nav::keys;
use crate::macos::{git, mount, unmount};
use crate::recovery_layout::GlobalSettings;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::{e2e, theme, KagiApp};

fn fixture(value: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(temp.path().join("f.rs"), "fn base() {}\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-qm", "base"]);
    replace(temp.path(), value);
    temp
}

fn replace(repo: &std::path::Path, value: &str) {
    std::fs::write(repo.join("f.rs"), format!("fn {value}() {{}}\n")).unwrap();
}

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.run_until_parked();
    e2e::diff_controls::clear_frame();
    app.update(cx, |app, cx| {
        if let Some(pane) = &app.ui().main_diff {
            pane.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
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

fn selected(key: u64) -> usize {
    let fill: gpui::Fill = theme::selection_overlay().into();
    e2e::diff_controls::painted_rows(key)
        .iter()
        .filter(|(_, background)| background.as_ref() == Some(&fill))
        .count()
}

fn focus_body(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    token: u64,
) {
    for _ in 0..128 {
        draw(cx, app, window);
        if e2e::diff_controls::body_focused(token) {
            return;
        }
        keys(cx, window, "tab");
    }
    panic!("production diff body must be reachable by Tab");
}

fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

fn graph_copy(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let expected = app.update(cx, |app, cx| {
        app.ui_mut().unwrap().selected = Some(0);
        cx.notify();
        app.view().rows[0].id.0.clone()
    });
    draw(cx, app, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
    })
    .unwrap();
    keys(cx, window, "cmd-c");
    assert_eq!(
        clipboard(cx),
        expected,
        "Graph Copy must not use an unfocused or retired diff"
    );
}

fn root_without_graph_copy(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    draw(cx, app, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("sentinel".into()));
    })
    .unwrap();
    keys(cx, window, "cmd-c");
    assert_eq!(
        clipboard(cx),
        "sentinel",
        "an unfocused diff and the covered Graph cannot own root Copy"
    );
}

pub fn scenario_diff_selection_revision(cx: &mut VisualTestAppContext) {
    let _globals = GlobalSettings::capture();
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["diff_split", "theme", "graph_copy_target"]);
    let previous_split = theme::diff_split();
    kagi::ui::settings::write_setting("graph_copy_target", Some("hash"));
    let temp = fixture("OLD");
    let repo = temp.path().canonicalize().unwrap();
    let (app, window) =
        crate::diff_split_unicode_badge::mount_size(cx, &repo, (1392., 883.), "apple-light");
    for split in [false, true] {
        theme::set_diff_split(split);
        for dimensions in [(940., 660.), (1392., 883.)] {
            crate::pr_detail_adaptive::resize(cx, window, dimensions);
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
                    kagi::ui::i18n::set_lang(lang);
                    app.update(cx, |app, cx| app.set_theme("apple-light", cx));
                    replace(&repo, "OLD");
                    open(cx, &app);
                    revision_cell(cx, &app, window, &repo);
                }
            }
        }
    }
    unmount(cx, app, window);
    for split in [false, true] {
        theme::set_diff_split(split);
        owner_cell(cx);
    }
    theme::set_diff_split(previous_split);
    eprintln!("[gui-e2e] PASS diff_selection_revision unified/split replacement/highlight/owner/close/focus");
}

fn revision_cell(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    let previous_theme = theme::theme().slug.to_string();
    draw(cx, &app, window);
    let (old, count) = surface(cx, &app);
    focus_body(cx, &app, window, old);
    keys(cx, window, "cmd-a cmd-c");
    let old_text = clipboard(cx);
    assert!(
        old_text.contains("OLD"),
        "real backend OLD fixture selected"
    );
    draw(cx, &app, window);
    let painted = selected(old);
    assert!(painted > 0);

    let pane = assert_highlights_preserve_selection(cx, app, window, old, painted, &old_text);

    replace(&repo, "NEW");
    open(cx, &app);
    draw(cx, &app, window);
    let (new, new_count) = surface(cx, &app);
    assert_eq!(
        count, new_count,
        "same path and row count intentionally collide in the old implementation"
    );
    assert_ne!(
        old, new,
        "same-sized replacement receives a fresh content revision"
    );
    assert_eq!(
        selected(new),
        0,
        "diff_selection_revision: adopting NEW must retire OLD selection paint"
    );
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("sentinel".into())));
    keys(cx, window, "cmd-c");
    assert!(
        !clipboard(cx).contains("OLD"),
        "unselected NEW cannot copy saved OLD code"
    );
    assert!(
        e2e::diff_controls::body_focused(new),
        "content replacement retains body focus"
    );
    keys(cx, window, "cmd-a cmd-c");
    assert!(clipboard(cx).contains("NEW"));
    assert!(!clipboard(cx).contains("OLD"));

    // Root focus alone cannot expose a saved diff range or the covered Graph.
    root_without_graph_copy(cx, &app, window);
    focus_body(cx, &app, window, new);
    keys(cx, window, "cmd-a cmd-c");
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    assert!(
        !e2e::diff_selection_present(),
        "close retires storage even while the old pane entity is retained"
    );
    graph_copy(cx, &app, window);
    app.update(cx, |app, cx| app.set_theme(&previous_theme, cx));
    drop(pane);
}

fn assert_highlights_preserve_selection(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    old: u64,
    painted: usize,
    old_text: &str,
) -> Entity<kagi::ui::MainDiffPane> {
    // Exercise real asynchronous syntax and theme-only span replacement.
    let pane = cx.read(|cx| app.read(cx).ui().main_diff.clone().unwrap());
    pane.update(cx, |pane, cx| {
        pane.view.highlighted = None;
        cx.notify();
    });
    draw(cx, &app, window);
    draw(cx, &app, window);
    assert_eq!(surface(cx, &app).0, old);
    assert_eq!(
        selected(old),
        painted,
        "syntax-only highlight preserves selection"
    );
    assert!(
        cx.read(|cx| pane.read(cx).view.highlighted == Some(theme::theme().key())),
        "requested syntax spans really landed without changing the selected revision",
    );
    keys(cx, window, "cmd-c");
    assert_eq!(clipboard(cx), old_text);
    app.update(cx, |app, cx| app.set_theme("apple-dark", cx));
    draw(cx, &app, window);
    draw(cx, &app, window);
    assert_eq!(surface(cx, &app).0, old);
    assert_eq!(
        selected(old),
        painted,
        "theme-only highlight preserves selection"
    );
    assert!(cx.read(|cx| pane.read(cx).view.highlighted == Some(theme::theme().key())));
    keys(cx, window, "cmd-c");
    assert_eq!(clipboard(cx), old_text);
    pane
}

fn owner_cell(cx: &mut VisualTestAppContext) {
    let first = fixture("OLD");
    let second = fixture("NEW");
    let repo = first.path().canonicalize().unwrap();
    let other = second.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    open(cx, &app);
    draw(cx, &app, window);
    let (old, count) = surface(cx, &app);
    focus_body(cx, &app, window, old);
    keys(cx, window, "cmd-a cmd-c");
    assert!(clipboard(cx).contains("OLD"));
    app.update(
        cx,
        |app, cx| assert!(app.open_repository(other.clone(), cx)),
    );
    cx.run_until_parked();
    assert!(
        !e2e::diff_selection_present(),
        "owner departure retires the saved OLD range"
    );
    graph_copy(cx, &app, window);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, other.clone(), cx)
    });
    open(cx, &app);
    draw(cx, &app, window);
    let (new, new_count) = surface(cx, &app);
    assert_eq!(count, new_count);
    assert_eq!(
        selected(new),
        0,
        "another repository's same path/count cannot inherit OLD paint"
    );
    focus_body(cx, &app, window, new);
    keys(cx, window, "cmd-a cmd-c");
    assert!(clipboard(cx).contains("NEW"));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    draw(cx, &app, window);
    let (returned, _) = surface(cx, &app);
    assert_eq!(
        selected(returned),
        0,
        "returning to retained owner does not resurrect selection"
    );
    root_without_graph_copy(cx, &app, window);
    unmount(cx, app, window);
}
