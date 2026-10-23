//! Conflict draft reset ownership and directly editable Result safety.
use crate::app_conflict::{click_control, wait_idle};
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use kagi_git::resolution::SelectionSide;
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use std::path::Path;

#[path = "conflict_progressive.rs"]
mod progressive;
pub use progressive::scenario_conflict_progressive_add_add;

fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    for (branch, text) in [
        ("main", "base"),
        ("feature", "incoming"),
        ("main", "current"),
    ] {
        if text == "incoming" {
            git(repo, &["checkout", "-qb", branch]);
        }
        if text == "current" {
            git(repo, &["checkout", "-q", branch]);
        }
        for name in ["a.txt", "b.txt"] {
            let middle = (0..20)
                .map(|i| format!("context {i}\n"))
                .collect::<String>();
            std::fs::write(
                repo.join(name),
                format!("{name} {text} first\n{middle}{name} {text} last\n"),
            )
            .unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", text]);
    }
    assert!(!std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .status()
        .unwrap()
        .success());
    fixture
}

fn reset_menu(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, hunk: usize) {
    click_control(cx, window, &format!("conflict-choice-{hunk}"));
    click_control(cx, window, "conflict-choice-menu-item-2-0");
}
fn reset_leg(cx: &mut VisualTestAppContext, lang: Lang, zoom: f32) {
    for navigation in ["conflict-next-hunk", "conflict-prev-hunk", "conflict-row-1"] {
        let fixture = fixture();
        let fingerprint = repo_fingerprint(fixture.path());
        let (app, window) = mount(cx, fixture.path());
        theme::set_zoom(zoom);
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.detect_conflict_mode(cx);
        });
        cx.run_until_parked();
        let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
        conflict.update(cx, |view, cx| {
            for (path, side) in [
                ("a.txt", SelectionSide::Current),
                ("b.txt", SelectionSide::Incoming),
            ] {
                view.conflict_open_editor(Path::new(path));
                assert_eq!(
                    view.mode
                        .as_ref()
                        .unwrap()
                        .buffer
                        .hunk_count(Path::new(path)),
                    2
                );
                view.conflict_editor_set_hunk_side(Path::new(path), 0, side, true);
            }
            view.conflict_select_file(0);
            if navigation == "conflict-prev-hunk" {
                view.conflict_editor_select_hunk(1);
            }
            cx.notify();
        });
        let before = cx.read(|cx| {
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("b.txt"))
        });
        let a_before = cx.read(|cx| {
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("a.txt"))
        });
        reset_menu(cx, window, usize::from(navigation == "conflict-prev-hunk"));
        cx.read(|cx| {
            assert_eq!(
                conflict.read(cx).reset_all_target.as_deref(),
                Some(Path::new("a.txt"))
            )
        });
        click_control(cx, window, navigation);
        cx.read(|cx| assert!(conflict.read(cx).reset_all_target.is_none()));
        click_control(cx, window, "conflict-row-1");
        reset_menu(cx, window, 0);
        cx.read(|cx| {
            let view = conflict.read(cx);
            assert_eq!(view.editing.as_deref(), Some(Path::new("b.txt")));
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new("b.txt")),
                before,
                "first Reset on another file must only arm"
            );
            assert_eq!(view.reset_all_target.as_deref(), Some(Path::new("b.txt")));
            assert_eq!(
                view.mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .hunk_model(Path::new("b.txt"))
                    .unwrap()
                    .resolved_hunk_count(),
                1
            );
        });
        reset_menu(cx, window, 0);
        cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().unwrap();
            assert_eq!(
                mode.buffer
                    .hunk_model(Path::new("b.txt"))
                    .unwrap()
                    .resolved_hunk_count(),
                0
            );
            assert_eq!(
                mode.buffer
                    .hunk_model(Path::new("a.txt"))
                    .unwrap()
                    .resolved_hunk_count(),
                1
            );
            assert_eq!(mode.buffer.resolved_text(Path::new("a.txt")), a_before);
        });
        conflict.update(cx, |view, cx| {
            view.conflict_select_file(0);
            assert!(view.reset_all_target.is_none());
            cx.notify();
        });
        reset_menu(cx, window, 0);
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            window.focus(&root, cx);
            window.draw(cx).clear();
        })
        .unwrap();
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(
                conflict.read(cx).reset_all_target.is_none(),
                "blur retires approval"
            )
        });
        reset_menu(cx, window, 0);
        conflict.update(cx, |view, cx| {
            view.conflict_editor_select_hunk(1);
            assert!(view.reset_all_target.is_none());
            view.writer_busy = true;
            cx.notify();
        });
        reset_menu(cx, window, 1);
        cx.read(|cx| {
            assert!(
                conflict.read(cx).reset_all_target.is_none(),
                "busy cannot arm"
            )
        });
        crate::keyboard_nav::keys(cx, window, "escape");
        conflict.update(cx, |view, cx| {
            view.writer_busy = false;
            cx.notify();
        });
        for key in ["enter", "space"] {
            conflict.update(cx, |view, cx| {
                view.conflict_editor_set_hunk_side(
                    Path::new("a.txt"),
                    0,
                    SelectionSide::Current,
                    true,
                );
                cx.notify();
            });
            cx.update_window(window, |_, window, cx| {
                let root = app.read(cx).root_focus.clone().unwrap();
                window.focus(&root, cx);
                window.draw(cx).clear();
            })
            .unwrap();
            let mut found = false;
            for _ in 0..80 {
                kagi::ui::conflict_editor_a11y::take_focus_probe();
                crate::keyboard_nav::keys(cx, window, "tab");
                crate::keyboard_nav::keys(cx, window, "f19");
                if kagi::ui::conflict_editor_a11y::take_focus_probe().as_deref()
                    == Some("conflict-choice-1")
                {
                    found = true;
                    break;
                }
            }
            assert!(
                found,
                "Reset menu launcher is reached through actual Tab navigation"
            );
            crate::keyboard_nav::keys(cx, window, "enter end");
            crate::keyboard_nav::keys(cx, window, key);
            cx.read(|cx| {
                assert_eq!(
                    conflict.read(cx).reset_all_target.as_deref(),
                    Some(Path::new("a.txt"))
                )
            });
            kagi::ui::conflict_editor_a11y::take_focus_probe();
            crate::keyboard_nav::keys(cx, window, "f19");
            assert_eq!(
                kagi::ui::conflict_editor_a11y::take_focus_probe().as_deref(),
                Some("conflict-choice-1"),
                "arming returns focus to the pill for explicit confirmation"
            );
            crate::keyboard_nav::keys(cx, window, "enter end");
            crate::keyboard_nav::keys(cx, window, key);
            cx.read(|cx| {
                let view = conflict.read(cx);
                assert!(view.reset_all_target.is_none());
                assert_eq!(
                    view.mode
                        .as_ref()
                        .unwrap()
                        .buffer
                        .hunk_model(Path::new("a.txt"))
                        .unwrap()
                        .resolved_hunk_count(),
                    0
                );
            });
        }
        assert_eq!(repo_fingerprint(fixture.path()), fingerprint);
        assert!(kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).is_empty());
        drop(conflict);
        unmount(cx, app, window);
    }
}

fn result_leg(cx: &mut VisualTestAppContext, lang: Lang, zoom: f32) {
    let fixture = fixture();
    let repository_before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    click_control(cx, window, "conflict-result-editor");
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    assert!(e2e::control_bounds(window.window_id(), "conflict-result-editor").is_some());
    cx.read(|cx| assert!(conflict.read(cx).result_editing));
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_input(window, "manual result\n");
    cx.run_until_parked();
    let manual = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("a.txt"))
    });
    assert_eq!(manual.as_deref(), Some("manual result\n"));
    for _ in 0..2 {
        click_control(cx, window, "conflict-result-editor");
        cx.run_until_parked();
    }
    cx.read(|cx| {
        assert_eq!(input.read(cx).value().as_str(), "manual result\n");
        assert_eq!(
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("a.txt")),
            manual
        );
    });
    cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(
            mode.buffer
                .hunk_model(Path::new("a.txt"))
                .unwrap()
                .resolved_hunk_count(),
            0
        );
        assert_eq!(
            mode.session.files[0].status,
            kagi_git::ConflictStatus::Resolved,
            "clean manual Result is ready without falsifying source-hunk choices"
        );
    });
    click_control(cx, window, "conflict-next-hunk");
    click_control(cx, window, "conflict-row-1");
    click_control(cx, window, "conflict-row-0");
    click_control(cx, window, "conflict-result-editor");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(
            view.mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new("a.txt")),
            manual
        );
        assert_eq!(
            view.editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            "manual result\n"
        );
    });
    assert_eq!(
        repo_fingerprint(fixture.path()),
        repository_before,
        "navigation never saves or stages"
    );
    assert!(
        kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 20).is_empty(),
        "direct editing and navigation emit no Git receipt"
    );
    // Busy refuses mode changes and cannot replace the authoritative draft.
    conflict.update(cx, |view, cx| {
        view.writer_busy = true;
        view.conflict_editor_set_result_mode(false);
        assert!(view.result_editing);
        cx.notify();
    });
    click_control(cx, window, "conflict-result-editor");
    cx.read(|cx| {
        assert!(conflict.read(cx).result_editing);
        assert_eq!(input.read(cx).value().as_str(), "manual result\n");
    });
    conflict.update(cx, |view, cx| {
        view.writer_busy = false;
        view.conflict_editor_set_result_mode(false);
        view.writer_busy = true;
        view.conflict_editor_set_result_mode(true);
        assert!(!view.result_editing);
        view.writer_busy = false;
        view.conflict_editor_set_result_mode(true);
        assert!(view.result_editing);
        cx.notify();
    });
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
}

fn matrix(cx: &mut VisualTestAppContext, leg: fn(&mut VisualTestAppContext, Lang, f32)) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let original = (i18n::lang(), theme::zoom());
    for lang in [Lang::En, Lang::Ja] {
        for zoom in [1., 1.67] {
            leg(cx, lang, zoom);
        }
    }
    i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    cx.update(theme::sync_gpui_component_theme);
}

pub fn scenario_conflict_reset_disarm(cx: &mut VisualTestAppContext) {
    matrix(cx, reset_leg);
}

pub fn scenario_conflict_result_segment(cx: &mut VisualTestAppContext) {
    matrix(cx, result_leg);
}
