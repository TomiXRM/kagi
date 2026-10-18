//! #1660: semantic reading context survives a same-source mode change.
//! Bounds come from the native list/cells, never a test-side split projection.
use crate::keyboard_nav::keys;
use crate::macos::{mount, unmount};
use gpui::{
    px, AnyWindowHandle, AppContext, Bounds, Entity, Modifiers, Pixels, VisualTestAppContext,
};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::file_history::FhDiffPane;
use kagi::ui::{e2e, theme, DiffRow, MainDiffPane, MainDiffView};
use kagi_ui_editor::EditorWorkspaceView;

const FILE: &str = "reading.rs";

struct Restore(bool);
impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_diff_split(self.0);
    }
}

enum Host {
    Main(Entity<MainDiffPane>),
    History(Entity<FhDiffPane>),
    Editor(Entity<EditorWorkspaceView>),
}

impl Host {
    fn view(&self, cx: &mut VisualTestAppContext) -> MainDiffView {
        cx.read(|cx| match self {
            Self::Main(pane) => pane.read(cx).view.clone(),
            Self::History(pane) => pane.read(cx).diff.clone().unwrap(),
            Self::Editor(pane) => pane
                .read(cx)
                .diff
                .as_ref()
                .unwrap()
                .downcast_ref::<MainDiffView>()
                .unwrap()
                .clone(),
        })
    }

    fn bounds(
        &self,
        cx: &mut VisualTestAppContext,
        index: usize,
    ) -> (Bounds<Pixels>, Option<Bounds<Pixels>>) {
        cx.read(|cx| match self {
            Self::Main(pane) => pane.read(cx).diff_row_bounds_for_e2e(index),
            Self::History(pane) => {
                let state = &pane.read(cx).scroll;
                (state.viewport_bounds(), state.bounds_for_item(index))
            }
            Self::Editor(pane) => {
                let state = &pane.read(cx).diff_scroll;
                (state.viewport_bounds(), state.bounds_for_item(index))
            }
        })
    }

    fn notify(&self, cx: &mut VisualTestAppContext) {
        match self {
            Self::Main(pane) => pane.update(cx, |_, cx| cx.notify()),
            Self::History(pane) => pane.update(cx, |_, cx| cx.notify()),
            Self::Editor(pane) => pane.update(cx, |_, cx| cx.notify()),
        }
    }

    fn mode(&self, cx: &mut VisualTestAppContext, window: AnyWindowHandle, split: bool) {
        let id = if split {
            "diff-mode-split"
        } else {
            "diff-mode-unified"
        };
        let bounds = cx.read(|cx| match self {
            Self::Main(pane) => pane.read(cx).fit.control_bounds(id),
            Self::History(pane) => pane.read(cx).fit.control_bounds(id),
            Self::Editor(_) => None,
        });
        match self {
            // Editor has no header-bounds recorder. This is a native owner
            // transition, not evidence of clicking its mode control (Tier B).
            Self::Editor(_) => {
                theme::set_diff_split(split);
                self.notify(cx);
            }
            _ => {
                let bounds = bounds.expect("real mode segment measured");
                cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
                cx.simulate_click(window, bounds.center(), Modifiers::none());
            }
        }
        frame(cx, self, window);
        assert_eq!(theme::diff_split(), split, "actual mode transition");
    }

    fn reveal_unified(&self, cx: &mut VisualTestAppContext, index: usize) {
        assert!(!theme::diff_split());
        match self {
            Self::Main(pane) => pane.update(cx, |pane, cx| {
                pane.reveal_diff_row_for_e2e(index, cx);
            }),
            Self::History(pane) => pane.update(cx, |pane, cx| {
                pane.scroll.scroll_to_reveal_item(index);
                cx.notify();
            }),
            Self::Editor(pane) => pane.update(cx, |pane, cx| {
                pane.diff_scroll.scroll_to_reveal_item(index);
                cx.notify();
            }),
        }
    }
}

fn frame(cx: &mut VisualTestAppContext, host: &Host, window: AnyWindowHandle) {
    for _ in 0..3 {
        cx.run_until_parked();
        host.notify(cx);
        crate::pr_viewed::paint(cx, window);
    }
}

fn wheel(cx: &mut VisualTestAppContext, host: &Host, window: AnyWindowHandle, delta: f32) {
    let viewport = host.bounds(cx, 0).0;
    assert!(viewport.size.height > px(100.), "real native diff viewport");
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    frame(cx, host, window);
}

fn row_index(view: &MainDiffView, needle: &str) -> usize {
    view.rows
        .iter()
        .position(|row| matches!(row, DiffRow::Line { text, .. } if text.contains(needle)))
        .unwrap_or_else(|| panic!("real backend row missing: {needle}"))
}

fn code(view: &MainDiffView, index: usize) -> String {
    match &view.rows[index] {
        DiffRow::Line { text, .. } => text[1..].to_owned(),
        _ => panic!("semantic code row required"),
    }
}

fn intersects(row: Bounds<Pixels>, viewport: Bounds<Pixels>) -> bool {
    row.bottom() > viewport.top() + px(1.) && row.top() < viewport.bottom() - px(1.)
}

fn clear_cell(window: AnyWindowHandle, index: usize) {
    for side in [0, 1] {
        e2e::clear_control_bounds(
            window.window_id(),
            &format!("split-cell-{}", index * 2 + side),
        );
    }
}

fn assert_painted(
    cx: &mut VisualTestAppContext,
    host: &Host,
    window: AnyWindowHandle,
    index: usize,
    expected: &str,
) {
    assert_eq!(
        code(&host.view(cx), index),
        expected,
        "same semantic source content"
    );
    let viewport = host.bounds(cx, 0).0;
    let bounds = if theme::diff_split() {
        // IDs identify original source cells, not projected list indices.
        // Clear stale offscreen geometry before asking the actual renderer.
        clear_cell(window, index);
        frame(cx, host, window);
        [0, 1]
            .into_iter()
            .filter_map(|side| {
                e2e::control_bounds(
                    window.window_id(),
                    &format!("split-cell-{}", index * 2 + side),
                )
            })
            .find(|bounds| intersects(*bounds, viewport))
    } else {
        host.bounds(cx, index)
            .1
            .filter(|bounds| intersects(*bounds, viewport))
    };
    assert!(bounds.is_some(), "#1660 reading-context reset: {expected:?} must be genuinely painted inside {viewport:?}, split={}", theme::diff_split());
}

fn seek(cx: &mut VisualTestAppContext, host: &Host, window: AnyWindowHandle, index: usize) {
    host.reveal_unified(cx, index);
    frame(cx, host, window);
    let (viewport, row) = host.bounds(cx, index);
    let row = row.expect("native reveal measured semantic source row");
    // Position the reading row at the top, independently of the selection.
    wheel(cx, host, window, f32::from(viewport.top() - row.top()));
}

fn clipboard(cx: &mut VisualTestAppContext) -> String {
    cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

fn copy_unchanged(cx: &mut VisualTestAppContext, window: AnyWindowHandle, expected: &str) {
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("sentinel".into())));
    keys(cx, window, "cmd-c");
    assert_eq!(
        clipboard(cx),
        expected,
        "mode change retains focused code-only Copy range"
    );
}

fn exercise(cx: &mut VisualTestAppContext, host: &Host, window: AnyWindowHandle) {
    frame(cx, host, window);
    let view = host.view(cx);
    assert!(view.rows.len() > 300, "real multi-hunk diff");
    // A real content click owns focus, and actual Cmd+A/Copy owns selection.
    let (_, first) = host.bounds(cx, 1);
    cx.simulate_click(
        window,
        first.expect("first native code row").center(),
        Modifiers::none(),
    );
    keys(cx, window, "cmd-a cmd-c");
    let selected = clipboard(cx);
    assert!(selected.contains("WIP_0390") && selected.contains("let line_0389"));
    assert!(
        !selected.contains("@@")
            && !selected
                .lines()
                .any(|line| line.starts_with('+') || line.starts_with('-')),
        "Copy contains code, not diff decoration"
    );
    // All selection stays unchanged while the independent reading position
    // visits context, unequal replacement, wrapping and a one-sided tail.
    for needle in [
        "line_0389",
        "line_0390 = 390;",
        "extra replacement B",
        "line_0610 = 610;",
        "TAIL_ONLY",
    ] {
        let index = row_index(&view, needle);
        let expected = code(&view, index);
        seek(cx, host, window, index);
        assert_painted(cx, host, window, index, &expected);
        if needle == "line_0610 = 610;" {
            assert!(
                host.bounds(cx, index).1.unwrap().size.height > theme::scaled_px(40.),
                "actual wrapped row must be taller than one code line"
            );
            let height = host.bounds(cx, index).1.unwrap().size.height;
            wheel(cx, host, window, -f32::from(height) / 4.);
            assert_painted(cx, host, window, index, &expected);
        }
        host.mode(cx, window, true);
        assert_painted(cx, host, window, index, &expected);
        copy_unchanged(cx, window, &selected);
        host.mode(cx, window, false);
        assert_painted(cx, host, window, index, &expected);
        copy_unchanged(cx, window, &selected);
    }
}

fn fixture() -> tempfile::TempDir {
    // Extend the existing Editor reading fixture without changing its reload
    // oracle: real Git context, many hunks, same source shared by all hosts.
    let (fixture, wip) = crate::editor_diff_extent_reload::fixture();
    let repo = fixture.path();
    let base = std::fs::read_to_string(repo.join(FILE)).unwrap();
    // Commit a long line on the old side too, so a split->unified pair has
    // the documented left representative while both layouts really wrap.
    let long = format!(
        "let line_0610 = 610; // {}\n",
        "wrapped reading context ".repeat(32)
    );
    let committed: String = (1..=800)
        .map(|line| {
            if line == 610 {
                long.clone()
            } else {
                format!("let line_{line:04} = {line};\n")
            }
        })
        .collect();
    std::fs::write(repo.join(FILE), committed).unwrap();
    crate::macos::git(repo, &["add", FILE]);
    crate::macos::git(repo, &["commit", "-qm", "wrapped reading base"]);
    let wip =
        wip.replace(
            "let line_0390 = 391; // WIP_0390\n",
            "let line_0390 = 391; // WIP_0390\nextra replacement A\nextra replacement B\n",
        )
        .replace(
            "let line_0610 = 611; // WIP_0610\n",
            &format!("{} // WIP_0610\n", long.trim_end()),
        ) + "TAIL_ONLY\n";
    std::fs::write(repo.join(FILE), &wip).unwrap();
    // Same-count replacement/other-file fixture has visibly distinct source.
    std::fs::write(repo.join("replacement.rs"), base.replace("line_", "other_")).unwrap();
    fixture
}

fn prepare() -> (
    Restore,
    crate::recovery_layout::GlobalSettings,
    crate::gui_isolation::SavedKeys,
) {
    let restore = Restore(theme::diff_split());
    let globals = crate::recovery_layout::GlobalSettings::capture();
    let saved = crate::gui_isolation::SavedKeys::keep(&["diff_split"]);
    theme::set_diff_split(false);
    theme::set_zoom(1.);
    (restore, globals, saved)
}

pub fn scenario_main_diff_reading_context(cx: &mut VisualTestAppContext) {
    let _restore = prepare();
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index: 0 }, cx)
    });
    cx.run_until_parked();
    let pane = cx.read(|cx| app.read(cx).ui().main_diff.clone().unwrap());
    let host = Host::Main(pane);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        exercise(cx, &host, window);
        let before = host.view(cx).rows.len();
        let replacement = std::fs::read_to_string(repo.join(FILE))
            .unwrap()
            .replace("WIP_", "NEW_");
        std::fs::write(repo.join(FILE), replacement).unwrap();
        app.update(cx, |app, cx| {
            app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index: 0 }, cx)
        });
        cx.run_until_parked();
        let current = Host::Main(cx.read(|cx| app.read(cx).ui().main_diff.clone().unwrap()));
        frame(cx, &current, window);
        assert_eq!(
            current.view(cx).rows.len(),
            before,
            "same-count source replacement"
        );
        assert_top(cx, &current, window);
    }));
    drop(host);
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn assert_top(cx: &mut VisualTestAppContext, host: &Host, window: AnyWindowHandle) {
    let view = host.view(cx);
    let first = view
        .rows
        .iter()
        .position(|row| matches!(row, DiffRow::Line { .. }))
        .unwrap();
    assert_painted(cx, host, window, first, &code(&view, first));
    let (viewport, header) = host.bounds(cx, 0);
    assert!(
        header.is_some_and(|header| (header.top() - viewport.top()).abs() <= px(1.)),
        "new source starts at top, never inherits prior tail anchor"
    );
}

pub fn scenario_file_history_reading_context(cx: &mut VisualTestAppContext) {
    let _restore = prepare();
    let fixture = fixture();
    let (app, window) = mount(cx, fixture.path());
    let history = crate::file_history_open::open_history(cx, &app, window, (1440., 900.), FILE);
    let pane = cx.read(|cx| {
        history
            .read(cx)
            .diff_pane
            .clone()
            .downcast::<FhDiffPane>()
            .unwrap()
    });
    let host = Host::History(pane);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        exercise(cx, &host, window);
        // The initial add is tall too: inheriting the old tail cannot pass
        // merely because the replacement diff fits in one viewport.
        history.update(cx, |history, cx| history.select(2, cx));
        cx.run_until_parked();
        frame(cx, &host, window);
        let replacement = host.view(cx);
        assert!(replacement.rows.len() > 800);
        assert!(code(&replacement, 1).contains("line_0001"));
        assert_top(cx, &host, window);
    }));
    drop((history, host));
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

pub fn scenario_editor_diff_reading_context(cx: &mut VisualTestAppContext) {
    let _restore = prepare();
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx.read(|cx| app.read(cx).ui().editor_workspace.clone().unwrap());
    editor.update(cx, |editor, cx| editor.open_tab(FILE.into(), cx));
    crate::editor_diff_extent_reload::wait_loaded(cx, &editor, window);
    let host = Host::Editor(editor.clone());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        exercise(cx, &host, window);
        editor.update(cx, |editor, cx| {
            editor.open_tab("replacement.rs".into(), cx)
        });
        crate::editor_diff_extent_reload::wait_loaded(cx, &editor, window);
        frame(cx, &host, window);
        assert!(
            host.view(cx)
                .rows
                .iter()
                .any(|row| matches!(row, DiffRow::Line { text, .. } if text.contains("other_"))),
            "other file's actual backend diff"
        );
        assert_top(cx, &host, window);
    }));
    drop((host, editor));
    // Reuse the Editor fixture's retirement pattern: drop the dispatch root
    // before draining callbacks that hold the text editor's native resources.
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .unwrap();
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
