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
    click_control(cx, window, &format!("conflict-hunk-{hunk}-more"));
    click_control(cx, window, "conflict-choice-menu-item-1-1");
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
                match kagi::ui::conflict_editor_a11y::take_focus_probe().as_deref() {
                    Some("conflict-hunk-0-left") => {
                        // The whole-file resolver has one roving Tab stop.
                        // Adoption of hunk 0 moved that entry point back to 0.
                        crate::keyboard_nav::keys(cx, window, "down right f19");
                        assert_eq!(
                            kagi::ui::conflict_editor_a11y::take_focus_probe().as_deref(),
                            Some("conflict-hunk-1-more"),
                            "Down reaches the next hunk from the actual Tab stop"
                        );
                        found = true;
                        break;
                    }
                    Some("conflict-hunk-1-left") => {
                        crate::keyboard_nav::keys(cx, window, "right");
                        found = true;
                        break;
                    }
                    _ => {}
                }
            }
            assert!(
                found,
                "Reset menu launcher is reached through actual Tab navigation: {lang:?} {zoom} {navigation}"
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
                Some("conflict-hunk-1-more"),
                "arming returns focus to More for explicit confirmation"
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

fn assert_more_menu(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, lang: Lang) {
    crate::native_ax::activate(cx);
    click_control(cx, window, "conflict-hunk-0-more");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let tree = crate::native_ax::tree(cx, window);
    let items: Vec<_> = tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .filter(|node| node["aria"]["role"] == "MenuItem")
        .collect();
    assert_eq!(items.len(), 4, "More contains exactly four commands");
    let labels = if lang == Lang::Ja {
        [
            "両方を使う（← → の順）",
            "両方を使う（→ ← の順）",
            "この hunk をリセット",
            "すべてリセット",
        ]
    } else {
        [
            "Use both, left first",
            "Use both, right first",
            "Reset this hunk",
            "Reset all",
        ]
    };
    let mut previous_bottom = None;
    for (item, label) in ["0-0", "0-1", "1-0", "1-1"].into_iter().zip(labels) {
        let id = format!("conflict-choice-menu-item-{item}");
        let identity = format!("{:?}", gpui::ElementId::Name(id.clone().into()));
        let node = items
            .iter()
            .find(|node| node["element_id"] == identity)
            .unwrap_or_else(|| panic!("missing More item {id}: {tree}"));
        assert_eq!(node["aria"]["label"], label);
        let bounds = e2e::control_bounds(window.window_id(), &id).unwrap();
        if let Some(bottom) = previous_bottom {
            assert!(
                bounds.top() >= bottom,
                "More commands retain their specified order"
            );
        }
        previous_bottom = Some(bounds.bottom());
    }
    crate::keyboard_nav::keys(cx, window, "escape");
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
    click_control(cx, window, "conflict-hunk-0-right");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_incoming_lines([true; 3]);
    assert_eq!(ax::control("conflict-hunk-0-right").unwrap().2, "all");
    assert_eq!(ax::control("conflict-hunk-0-left").unwrap().2, "none");
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
    assert_eq!(ax::control("conflict-hunk-0-right").unwrap().2, "partial");
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
    assert_more_menu(cx, window, lang);
    click_control(cx, window, "conflict-hunk-0-more");
    click_control(cx, window, "conflict-choice-menu-item-1-0");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_incoming_lines([false; 3]);
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
    assert_eq!(ax::control("conflict-hunk-0-right").unwrap().2, "all");
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
    assert_eq!(ax::control("conflict-hunk-0-right").unwrap().2, "none");
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
        "arrow, checkbox, drag and More never write HEAD/index/worktree"
    );
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100).is_empty());
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_arrow_checkboxes(cx: &mut VisualTestAppContext) {
    matrix(cx, arrow_checkbox_leg);
}
