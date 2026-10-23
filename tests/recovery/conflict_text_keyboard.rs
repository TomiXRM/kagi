//! #1311: production resolver Tab/arrow/activation dispatch, not native VoiceOver.
use crate::app_conflict::click_control;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::Entity;
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::conflict_editor_a11y as ax;
use kagi::ui::{conflict_view::ConflictView, e2e};
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use std::path::Path;

pub(super) fn fixture() -> tempfile::TempDir {
    fixture_for_operation(false)
}
fn fixture_for_operation(rebase: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    for side in ["base", "theirs", "ours"] {
        if side == "theirs" {
            git(dir.path(), &["checkout", "-qb", "feature"]);
        }
        if side == "ours" {
            git(dir.path(), &["checkout", "-q", "main"]);
        }
        let mut lines: Vec<String> = (1..=24).map(|n| format!("context {n}\n")).collect();
        for n in [1, 2, 3, 17, 18, 19] {
            lines[n] = format!("{side} {n}\n");
        }
        std::fs::write(dir.path().join("notes.txt"), lines.concat()).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-qm", side]);
    }
    if rebase {
        git(dir.path(), &["checkout", "-q", "feature"]);
    }
    assert!(!std::process::Command::new("git")
        .args(if rebase {
            ["rebase", "main"]
        } else {
            ["merge", "feature"]
        })
        .current_dir(dir.path())
        .output()
        .unwrap()
        .status
        .success());
    dir
}
fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn probe(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Option<String> {
    ax::take_focus_probe();
    cx.simulate_keystrokes(window, "f19");
    cx.run_until_parked();
    ax::take_focus_probe()
}
fn keys(cx: &mut VisualTestAppContext, window: AnyWindowHandle, key: &str) {
    crate::keyboard_nav::keys(cx, window, key);
    paint(cx, window);
}

fn assembled(cx: &mut VisualTestAppContext, conflict: &Entity<ConflictView>) -> String {
    cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .hunk_model(Path::new("notes.txt"))
            .unwrap()
            .assembled_text()
    })
}

fn horizontal_controls(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
    lang: Lang,
) {
    let labels = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().labels());
    let incoming = format!("{} ({})", labels.incoming.role, labels.incoming.name);
    keys(cx, window, "left");
    keys(cx, window, "space");
    assert_eq!(ax::control("resolver-hunk-1-current").unwrap().2, "all");
    keys(cx, window, "right right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-1-current-first")
    );
    keys(cx, window, "enter");
    let ours_first = assembled(cx, conflict);
    assert!(ours_first.find("ours 17").unwrap() < ours_first.find("theirs 17").unwrap());
    keys(cx, window, "right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-1-incoming-first")
    );
    assert_eq!(
        ax::control("resolver-hunk-1-incoming-first").unwrap().1,
        if lang == Lang::En {
            format!("Order: {incoming} first")
        } else {
            format!("順序: {incoming} を先に")
        }
    );
    keys(cx, window, "space");
    let theirs_first = assembled(cx, conflict);
    assert!(theirs_first.find("theirs 17").unwrap() < theirs_first.find("ours 17").unwrap());
    keys(cx, window, "right");
    let line = "resolver-hunk-1-current-line-0";
    assert_eq!(probe(cx, window).as_deref(), Some(line));
    assert_eq!(
        ax::control(line).unwrap().1,
        if lang == Lang::En {
            "Use Current branch (main) line 18"
        } else {
            "Current branch (main) の 18 行目を採用"
        }
    );
    keys(cx, window, "enter");
    assert_eq!(ax::control("resolver-hunk-1-current").unwrap().2, "partial");
    assert!(!assembled(cx, conflict).contains("ours 17"));
    let bounds = e2e::control_bounds(window.window_id(), line).unwrap();
    let viewport = e2e::control_bounds(window.window_id(), "resolver-current-viewport").unwrap();
    assert!(
        bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
        "focused line is inside the resolver viewport"
    );
    conflict.update(cx, |view, cx| {
        let buffer = &mut view.mode.as_mut().unwrap().buffer;
        let after = buffer.resolved_text(Path::new("notes.txt"));
        assert!(buffer.undo(Path::new("notes.txt")));
        assert_ne!(buffer.resolved_text(Path::new("notes.txt")), after);
        assert!(buffer.redo(Path::new("notes.txt")));
        assert_eq!(buffer.resolved_text(Path::new("notes.txt")), after);
        cx.notify();
    });
    keys(cx, window, "up");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-1-current")
    );
}
fn tab_to(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    for _ in 0..100 {
        keys(cx, window, "tab");
        if probe(cx, window).as_deref() == Some(id) {
            return;
        }
    }
    panic!("Tab must reach the resolver adoption controls: {id}");
}

fn hunk_adoption(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
    lang: Lang,
) {
    tab_to(cx, window, "resolver-hunk-0-current");
    keys(cx, window, "down");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-0-current"),
        "roving focus cannot target an invisible hunk"
    );
    click_control(cx, window, "conflict-editor-next");
    paint(cx, window);
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.selected_hunk, 1);
    });
    tab_to(cx, window, "resolver-hunk-1-current");
    keys(cx, window, "right");
    let id = "resolver-hunk-1-incoming";
    assert_eq!(probe(cx, window).as_deref(), Some(id));
    let (role, name, state) = ax::control(id).expect("named adoption Button");
    assert_eq!(role, gpui::Role::Button);
    let labels = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().labels());
    let incoming = format!("{} ({})", labels.incoming.role, labels.incoming.name);
    assert_eq!(
        name,
        if lang == Lang::En {
            format!("Use {incoming} for hunk 2")
        } else {
            format!("hunk 2 に {incoming} を採用")
        }
    );
    assert_eq!(state, "none");
    keys(cx, window, "enter");
    let text = assembled(cx, conflict);
    assert!(text.contains("theirs 17"));
    assert_eq!(ax::control(id).unwrap().2, "all");
    conflict.update(cx, |view, cx| {
        view.conflict_editor_reset_all(Path::new("notes.txt"));
        cx.notify();
    });
    paint(cx, window);
    click_control(cx, window, id);
    paint(cx, window);
    assert_eq!(text, assembled(cx, conflict));
    horizontal_controls(cx, window, conflict, lang);
}

fn matrix_leg(
    cx: &mut VisualTestAppContext,
    lang: Lang,
    zoom: f32,
    slug: &str,
    dimensions: (f32, f32),
) {
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_theme(slug, cx);
        app.detect_conflict_mode(cx);
    });
    crate::pr_detail_adaptive::resize(cx, window, dimensions);
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("notes.txt"));
        cx.notify();
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    hunk_adoption(cx, window, &conflict, lang);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    click_control(cx, window, "conflict-editor-prev");
    paint(cx, window);
    tab_to(cx, window, "resolver-hunk-0-current");
    keys(cx, window, "space");
    assert_eq!(ax::control("resolver-hunk-0-current").unwrap().2, "all");
    assert!(
        kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 20).is_empty(),
        "adoption emits no Git receipt"
    );
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "adoption remains buffer-only"
    );
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_text_keyboard(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let original = (i18n::lang(), theme::zoom(), theme::theme().slug.to_string());
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            for slug in ["apple-light", "apple-dark"] {
                for dimensions in [(940., 660.), (1392., 883.)] {
                    matrix_leg(cx, lang, zoom, slug, dimensions);
                }
            }
        }
    }
    i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    theme::set_active(&original.2);
    cx.update(theme::sync_gpui_component_theme);
}

fn replace_model(
    cx: &mut VisualTestAppContext,
    conflict: &Entity<ConflictView>,
    repo: &Path,
    hunks: usize,
    lines: usize,
) {
    let backend = kagi_git::Backend::open(repo).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let mut markers = String::new();
    for hunk in 0..hunks {
        markers.push_str("context\n<<<<<<< Current\n");
        for line in 0..lines {
            markers.push_str(&format!("current {hunk} {line}\n"));
        }
        markers.push_str("=======\n");
        for line in 0..lines {
            markers.push_str(&format!("incoming {hunk} {line}\n"));
        }
        markers.push_str(">>>>>>> Incoming\n");
    }
    assert!(buffer.ensure_hunks(Path::new("notes.txt"), &markers));
    conflict.update(cx, |view, cx| {
        view.mode.as_mut().unwrap().buffer = buffer;
        cx.notify();
    });
}

pub fn scenario_conflict_resolver_rebuild(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("notes.txt"));
        cx.notify();
    });
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    tab_to(cx, window, "resolver-hunk-0-current");
    replace_model(cx, &conflict, fixture.path(), 3, 1);
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    click_control(cx, window, "conflict-editor-next");
    click_control(cx, window, "conflict-editor-next");
    paint(cx, window);
    tab_to(cx, window, "resolver-hunk-2-current");
    keys(cx, window, "right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-2-incoming")
    );
    keys(cx, window, "enter");
    assert!(assembled(cx, &conflict).contains("incoming 2 0"));
    replace_model(cx, &conflict, fixture.path(), 3, 4);
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx)
    })
    .unwrap();
    tab_to(cx, window, "resolver-hunk-2-current");
    keys(cx, window, "right right right right right right right");
    assert_eq!(
        probe(cx, window).as_deref(),
        Some("resolver-hunk-2-current-line-3")
    );
    keys(cx, window, "space");
    assert!(assembled(cx, &conflict).contains("current 2 3"));
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_resolver_roles(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        for rebase in [false, true] {
            let fixture = fixture_for_operation(rebase);
            let (app, window) = mount(cx, fixture.path());
            app.update(cx, |app, cx| {
                app.set_lang(lang, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
            conflict.update(cx, |view, cx| {
                view.conflict_open_editor(Path::new("notes.txt"));
                cx.notify();
            });
            paint(cx, window);
            let labels = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().labels());
            assert_eq!(
                labels.current.role,
                if rebase { "New base" } else { "Current branch" }
            );
            assert_eq!(
                labels.incoming.role,
                if rebase {
                    "Your commit being replayed"
                } else {
                    "Merging in"
                }
            );
            for (side, label) in [("current", &labels.current), ("incoming", &labels.incoming)] {
                let identity = format!("{} ({})", label.role, label.name);
                let name = ax::control(&format!("resolver-hunk-0-{side}")).unwrap().1;
                assert_eq!(
                    name,
                    if lang == Lang::En {
                        format!("Use {identity} for hunk 1")
                    } else {
                        format!("hunk 1 に {identity} を採用")
                    }
                );
                assert_eq!(
                    ax::control(&format!("resolver-hunk-0-{side}")).unwrap().2,
                    "none"
                );
                let order = format!("resolver-hunk-0-{side}-first");
                assert_eq!(
                    ax::control(&order).unwrap().2,
                    if side == "current" {
                        "selected"
                    } else {
                        "unselected"
                    }
                );
                let selected = format!("{} ({})", labels.current.role, labels.current.name);
                assert_eq!(
                    ax::description(&order).unwrap(),
                    if lang == Lang::En {
                        format!("Selected output order: {selected} first")
                    } else {
                        format!("選択中の出力順序: {selected} を先に")
                    }
                );
            }
            assert_order_only(cx, window, &conflict);
            drop(conflict);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original);
}

fn assert_order_only(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    let before = assembled(cx, conflict);
    click_control(cx, window, "resolver-hunk-0-incoming-first");
    paint(cx, window);
    assert_eq!(assembled(cx, conflict), before);
    cx.read(|cx| {
        assert!(
            !conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .has_resolution(Path::new("notes.txt")),
            "ordering an untouched hunk must not adopt an empty resolution"
        );
    });
    assert_eq!(ax::control("resolver-hunk-0-current").unwrap().2, "none");
    assert_eq!(ax::control("resolver-hunk-0-incoming").unwrap().2, "none");
    assert_eq!(
        ax::control("resolver-hunk-0-incoming-first").unwrap().2,
        "unselected"
    );
    assert_eq!(
        ax::control("resolver-hunk-0-current-first").unwrap().2,
        "selected"
    );
}
