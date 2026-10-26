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
#[path = "conflict_manual_reset.rs"]
mod manual_reset;
pub use manual_reset::scenario_conflict_manual_reset_guard;
#[path = "conflict_resume.rs"]
mod resume;
pub use resume::scenario_conflict_partial_resume;
#[path = "conflict_leave_and_return.rs"]
mod leave_and_return;
pub use leave_and_return::scenario_conflict_leave_and_return;

fn fixture() -> tempfile::TempDir {
    fixture_with_context(20)
}

fn fixture_with_context(context_lines: usize) -> tempfile::TempDir {
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
            let middle = (0..context_lines)
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

fn reset_menu(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, _hunk: usize) {
    click_control(cx, window, "conflict-reset-all");
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
            view.conflict_select_file(0, cx);
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
            view.conflict_select_file(0, cx);
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
            let mut focus_sequence = Vec::new();
            for _ in 0..80 {
                kagi::ui::conflict_editor_a11y::take_focus_probe();
                crate::keyboard_nav::keys(cx, window, "tab");
                crate::keyboard_nav::keys(cx, window, "f19");
                let focused = kagi::ui::conflict_editor_a11y::take_focus_probe();
                focus_sequence.push(focused.clone());
                match focused.as_deref() {
                    Some("conflict-reset-all") => {
                        assert_eq!(
                            kagi::ui::conflict_editor_a11y::control("conflict-reset-all")
                                .unwrap()
                                .0,
                            gpui::Role::Button,
                            "Result reset is reached from the actual Tab stop"
                        );
                        found = true;
                        break;
                    }
                    _ => {}
                }
            }
            assert!(
                found,
                "Result reset is reached through actual Tab navigation: {lang:?} {zoom} {navigation}; focus_sequence={focus_sequence:?}"
            );
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
                Some("conflict-reset-all"),
                "arming retains Reset all focus for explicit confirmation"
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

fn assert_incoming_lines(selected: [bool; 3]) {
    use kagi::ui::conflict_editor_a11y as ax;
    for (line, selected) in selected.into_iter().enumerate() {
        let (role, _, state) =
            ax::control(&format!("resolver-hunk-0-incoming-line-{line}")).unwrap();
        assert_eq!(role, gpui::Role::CheckBox);
        assert_eq!(state, if selected { "all" } else { "none" });
    }
}

fn drag_three_lines(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    use gpui::{Modifiers, MouseButton};
    let points: Vec<_> = (0..3)
        .map(|line| {
            let bounds = e2e::control_bounds(
                window.window_id(),
                &format!("resolver-hunk-0-incoming-line-{line}"),
            )
            .expect("dragged checkbox is laid out");
            gpui::point(bounds.left() + theme::scaled_px(12.), bounds.center().y)
        })
        .collect();
    cx.simulate_mouse_move(window, points[0], None, Modifiers::none());
    cx.simulate_mouse_down(window, points[0], MouseButton::Left, Modifiers::none());
    for point in &points[1..] {
        cx.simulate_mouse_move(window, *point, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
    }
    cx.simulate_mouse_up(window, points[2], MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn assert_action_rows(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, lang: Lang) {
    crate::native_ax::activate(cx);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let tree = crate::native_ax::tree(cx, window);
    let nodes = tree["nodes"].as_object().unwrap();
    assert_eq!(
        nodes
            .values()
            .filter(|node| node["aria"]["role"] == "MenuItem")
            .count(),
        0,
        "action rows replace the four-command menu"
    );
    let labels = if lang == Lang::Ja {
        [
            "この側を使う",
            "両方（現在の側 → 取り込む側）",
            "この側を使う",
            "両方（取り込む側 → 現在の側）",
        ]
    } else {
        [
            "Use this side",
            "Both (current → incoming)",
            "Use this side",
            "Both (incoming → current)",
        ]
    };
    for (id, label) in [
        "conflict-hunk-0-current-use",
        "conflict-hunk-0-current-both",
        "conflict-hunk-0-incoming-use",
        "conflict-hunk-0-incoming-both",
    ]
    .into_iter()
    .zip(labels)
    {
        let identity = format!("{:?}", gpui::ElementId::Name(id.into()));
        let node = nodes
            .values()
            .find(|node| node["element_id"] == identity)
            .unwrap_or_else(|| panic!("missing action row command {id}: {tree}"));
        assert_eq!(node["aria"]["label"], label);
        assert_eq!(node["aria"]["role"], "Button");
        assert!(e2e::control_bounds(window.window_id(), id).is_some());
    }
    for side in ["current", "incoming"] {
        let use_button =
            e2e::control_bounds(window.window_id(), &format!("conflict-hunk-0-{side}-use"))
                .unwrap();
        let both = e2e::control_bounds(window.window_id(), &format!("conflict-hunk-0-{side}-both"))
            .unwrap();
        assert!(
            use_button.right() <= both.left(),
            "Use precedes Both within its pane"
        );
        assert_eq!(
            use_button.top(),
            both.top(),
            "commands share one action row"
        );
    }
    let gutter = e2e::control_bounds(window.window_id(), "resolver-gutter-viewport").unwrap();
    assert!((f32::from(gutter.size.width) - f32::from(theme::scaled_px(12.))).abs() <= 1.);
    assert!(
        nodes.values().all(|node| {
            let id = node["element_id"].as_str().unwrap_or("");
            !id.contains("resolver-gutter")
                || !matches!(
                    node["aria"]["role"].as_str(),
                    Some("Button" | "CheckBox" | "MenuItem" | "Toolbar")
                )
        }),
        "gutter exposes no AX interactions"
    );
}

fn arrow_checkbox_leg(cx: &mut VisualTestAppContext, lang: Lang, zoom: f32) {
    use kagi::ui::conflict_editor_a11y as ax;
    let fixture = crate::conflict_text_keyboard::fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    crate::pr_detail_adaptive::resize(cx, window, (1392., 883.));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    assert_action_rows(cx, window, lang);
    click_control(cx, window, "conflict-hunk-0-incoming-use");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_incoming_lines([true; 3]);
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-state").unwrap().1,
        i18n::Msg::ConflictUsed.t()
    );
    assert_eq!(
        ax::control("conflict-hunk-0-current-state").unwrap().1,
        i18n::Msg::ConflictNotUsed.t()
    );
    for side in ["current", "incoming"] {
        assert_eq!(
            ax::control(&format!("conflict-hunk-0-{side}-change"))
                .unwrap()
                .1,
            i18n::Msg::ConflictChangeAccessible.t()
        );
    }
    let tree = crate::native_ax::tree(cx, window);
    let expected = if lang == Lang::Ja {
        "conflict 2 件 · 1 解決済み"
    } else {
        "2 conflicts · 1 resolved"
    };
    assert!(
        tree.to_string().contains(expected),
        "file header reports total and resolved: {tree}"
    );
    cx.read(|cx| {
        let view = conflict.read(cx);
        let result = view
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("notes.txt"))
            .unwrap();
        assert!(result.contains("theirs 1\ntheirs 2\ntheirs 3\n"));
        assert!(!result.contains("ours 1\n"));
        assert_eq!(
            view.editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            result
        );
    });
    let line = e2e::control_bounds(window.window_id(), "resolver-hunk-0-incoming-line-1").unwrap();
    let checkbox = gpui::point(line.left() + theme::scaled_px(12.), line.center().y);
    cx.simulate_mouse_move(window, checkbox, None, gpui::Modifiers::none());
    cx.simulate_click(window, checkbox, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_incoming_lines([true, false, true]);
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-state").unwrap().1,
        i18n::Msg::ConflictPartlyUsed.t()
    );
    cx.read(|cx| {
        let result = conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("notes.txt"))
            .unwrap();
        assert!(result.contains("theirs 1\ntheirs 3\n"));
        assert!(!result.contains("theirs 2\n"));
    });
    click_control(cx, window, "conflict-hunk-0-incoming-change");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_incoming_lines([false; 3]);
    assert_action_rows(cx, window, lang);
    click_control(cx, window, "conflict-hunk-0-current-use");
    click_control(cx, window, "conflict-reset-hunk");
    assert_incoming_lines([false; 3]);
    assert_action_rows(cx, window, lang);
    cx.read(|cx| {
        let view = conflict.read(cx);
        let model = view
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .hunk_model(Path::new("notes.txt"))
            .unwrap();
        assert!(
            !model.hunks()[0].is_resolved(),
            "Change returns this hunk to unresolved"
        );
    });
    let unresolved = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("notes.txt"))
            .unwrap()
    });
    drag_three_lines(cx, window);
    assert_incoming_lines([true; 3]);
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-state").unwrap().1,
        i18n::Msg::ConflictUsed.t()
    );
    cx.read(|cx| {
        let result = conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("notes.txt"))
            .unwrap();
        assert!(result.contains("theirs 1\ntheirs 2\ntheirs 3\n"));
    });
    drag_three_lines(cx, window);
    assert_incoming_lines([false; 3]);
    assert_eq!(
        ax::control("conflict-hunk-0-incoming-use").unwrap().2,
        "none"
    );
    cx.read(|cx| {
        let buffer = &conflict.read(cx).mode.as_ref().unwrap().buffer;
        let result = buffer.resolved_text(Path::new("notes.txt")).unwrap();
        assert_eq!(
            result, unresolved,
            "clearing the final selected line restores the exact unresolved Result, including markers"
        );
        let model = buffer.hunk_model(Path::new("notes.txt")).unwrap();
        let hunk = model.hunks()[0];
        assert_eq!(hunk.choice, kagi_git::resolution::HunkChoice::Unresolved);
        assert!(hunk.line_select.is_none());
        assert!(!hunk.is_resolved());
        assert!(buffer.cached_marker_residue(Path::new("notes.txt")));
    });
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "Use, Change, checkbox and drag never write HEAD/index/worktree"
    );
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).is_empty());
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_arrow_checkboxes(cx: &mut VisualTestAppContext) {
    matrix(cx, arrow_checkbox_leg);
}
