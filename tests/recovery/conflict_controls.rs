//! #1511 / #1512: destructive confirmation ownership and explicit Result selection.
use crate::app_conflict::click_control;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::e2e;
use kagi_git::resolution::SelectionSide;
use kagi_ui_core::{
    i18n::{self, Lang},
    theme,
};
use std::path::Path;

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

fn fill(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, id: &str) -> [u8; 4] {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        let bounds = e2e::control_bounds(window.window_handle().window_id(), id).unwrap();
        let image = window.render_to_image().expect("native segment fill");
        let scale = window.scale_factor();
        image
            .get_pixel(
                ((f32::from(bounds.right()) - 5.) * scale) as u32,
                ((f32::from(bounds.top()) + 5.) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

fn reset_leg(cx: &mut VisualTestAppContext, lang: Lang, zoom: f32) {
    for navigation in [
        "conflict-editor-next",
        "conflict-editor-prev",
        "conflict-row-1",
    ] {
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
        click_control(cx, window, "conflict-editor-reset");
        cx.read(|cx| {
            assert_eq!(
                conflict.read(cx).reset_all_target.as_deref(),
                Some(Path::new("a.txt"))
            )
        });
        click_control(cx, window, navigation);
        cx.read(|cx| assert!(conflict.read(cx).reset_all_target.is_none()));
        click_control(cx, window, "conflict-editor-reset");
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
        click_control(cx, window, "conflict-editor-reset");
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
        click_control(cx, window, "conflict-editor-reset");
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
        click_control(cx, window, "conflict-editor-reset");
        conflict.update(cx, |view, cx| {
            view.conflict_editor_select_hunk(1);
            assert!(view.reset_all_target.is_none());
            view.writer_busy = true;
            cx.notify();
        });
        click_control(cx, window, "conflict-editor-reset");
        cx.read(|cx| {
            assert!(
                conflict.read(cx).reset_all_target.is_none(),
                "busy cannot arm"
            )
        });
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
                kagi::ui::conflict_editor::take_reset_focus_probe();
                crate::keyboard_nav::keys(cx, window, "tab");
                crate::keyboard_nav::keys(cx, window, "f19");
                if kagi::ui::conflict_editor::take_reset_focus_probe() {
                    found = true;
                    break;
                }
            }
            assert!(found, "Reset all is reached through actual Tab navigation");
            crate::keyboard_nav::keys(cx, window, key);
            cx.read(|cx| {
                assert_eq!(
                    conflict.read(cx).reset_all_target.as_deref(),
                    Some(Path::new("a.txt"))
                )
            });
            kagi::ui::conflict_editor::take_reset_focus_probe();
            crate::keyboard_nav::keys(cx, window, "f19");
            assert!(
                kagi::ui::conflict_editor::take_reset_focus_probe(),
                "arming preserves Reset Button focus and ring"
            );
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
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_select_file(0);
        cx.notify();
    });
    let selected_fill = fill(cx, window, "conflict-result-preview");
    let input = cx.read(|cx| {
        conflict
            .read(cx)
            .editor_inputs
            .as_ref()
            .unwrap()
            .result
            .clone()
    });
    for (control, editing) in [
        ("conflict-result-preview", false),
        ("conflict-result-preview", false),
        ("conflict-result-edit", true),
        ("conflict-result-edit", true),
        ("conflict-result-preview", false),
    ] {
        click_control(cx, window, control);
        for body in ["conflict-result-edit-body", "conflict-result-preview-body"] {
            e2e::clear_control_bounds(window.window_id(), body);
        }
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        assert!(e2e::control_bounds(
            window.window_id(),
            if editing {
                "conflict-result-edit-body"
            } else {
                "conflict-result-preview-body"
            }
        )
        .is_some());
        assert!(e2e::control_bounds(
            window.window_id(),
            if editing {
                "conflict-result-preview-body"
            } else {
                "conflict-result-edit-body"
            }
        )
        .is_none());
        let selected_id = if editing {
            "conflict-result-edit"
        } else {
            "conflict-result-preview"
        };
        let unselected_id = if editing {
            "conflict-result-preview"
        } else {
            "conflict-result-edit"
        };
        assert_eq!(
            fill(cx, window, selected_id),
            selected_fill,
            "selected fill agrees with Result mode"
        );
        assert_ne!(
            fill(cx, window, unselected_id),
            selected_fill,
            "only one segment is selected"
        );
        cx.read(|cx| {
            let view = conflict.read(cx);
            assert_eq!(
                view.result_editing, editing,
                "segment explicitly selects its own mode"
            );
            let tabs = view.result_mode_tabs_for_e2e();
            assert_eq!(tabs.len(), 2);
            for (slot, (role, name, selected, stop)) in tabs.iter().enumerate() {
                assert_eq!(*role, gpui::Role::Tab);
                assert_eq!(
                    name,
                    if slot == 0 {
                        i18n::Msg::EditorPreviewMode.t()
                    } else {
                        i18n::Msg::EditorEditMode.t()
                    }
                );
                assert_eq!(*selected, editing == (slot == 1));
                assert_eq!(*stop, *selected);
            }
        });
    }
    let group = e2e::control_bounds(window.window_id(), "conflict-result-group").unwrap();
    // The separator/border belongs to the list, never either segment.
    let border = gpui::point(group.center().x, group.top() + gpui::px(0.25));
    cx.simulate_click(window, border, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            !conflict.read(cx).result_editing,
            "list chrome cannot activate a segment"
        )
    });
    click_control(cx, window, "conflict-result-edit");
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("manual result\n", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
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
        click_control(cx, window, "conflict-result-edit");
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
    conflict.update(cx, |view, cx| {
        view.writer_busy = true;
        cx.notify();
    });
    click_control(cx, window, "conflict-result-preview");
    cx.read(|cx| assert!(conflict.read(cx).result_editing));
    conflict.update(cx, |view, cx| {
        view.writer_busy = false;
        view.conflict_editor_set_result_mode(false);
        view.writer_busy = true;
        cx.notify();
    });
    click_control(cx, window, "conflict-result-edit");
    cx.read(|cx| assert!(!conflict.read(cx).result_editing));
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
