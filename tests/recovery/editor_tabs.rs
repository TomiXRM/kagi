//! #1262/#1263: actual Editor tab overflow and keyboard-operated pane selectors.
//! Tier A cannot deliver physical native Tab: traverse the rendered stops with
//! Window::focus_next, then deliver real key-down/key-up within each selector.
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    point, px, size, AnyWindowHandle, AppContext, Bounds, Entity, Modifiers, MouseButton, Pixels,
    Role, VisualTestAppContext,
};
use kagi::ui::{e2e, editor_workspace::EditorWorkspaceView, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{MiddlePaneTab, RightPaneTab};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, unmount};
use crate::pr_viewed::{paint, rev};

const PATHS: [&str; 12] = [
    "src/a.rs",
    "tests/a.rs",
    "file-02.rs",
    "file-03.rs",
    "file-04.rs",
    "file-05.rs",
    "file-06.rs",
    "file-07.rs",
    "file-08.rs",
    "file-09.rs",
    "file-10.rs",
    "file-11.rs",
];

fn fixture_paths() -> Vec<String> {
    let mut paths: Vec<String> = PATHS.iter().map(|path| (*path).to_string()).collect();
    paths.extend((12..29).map(|index| format!("file-{index:02}.rs")));
    // More than 200 characters, but every UTF-8 filesystem component stays
    // below 255 bytes. Both EN and JA survive exact tooltip/AX comparison.
    let segment = format!(
        "editor-overflow-{}{}",
        "long-name-".repeat(6),
        "日本語".repeat(5)
    );
    let long = format!("{segment}/{segment}/{segment}.rs");
    assert!(long.chars().count() > 200);
    assert!(long.split('/').all(|component| component.len() < 255));
    paths.push(long);
    assert_eq!(paths.len(), 30);
    paths
}

fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let paths = fixture_paths();
    for path in &paths {
        let file = fixture.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "fn original() {}\n").unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(fixture.path(), &["commit", "-q", "-m", "editor tabs base"]);
    for path in &paths {
        std::fs::write(fixture.path().join(path), "fn revised() {}\n").unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(
        fixture.path(),
        &["commit", "-q", "-m", "editor tabs history"],
    );
    for path in &paths {
        std::fs::write(fixture.path().join(path), "fn work_in_progress() {}\n").unwrap();
    }
    fixture
}

fn wait_for(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx))) {
            // Reveal requests may be deferred until the first measured frame.
            paint(cx, window);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real Editor backend read did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn loaded(view: &EditorWorkspaceView) -> bool {
    !view.loading && !view.file_loading && view.content.is_some() && view.editor.is_some()
}

fn matrix(
    cx: &mut VisualTestAppContext,
    scenario: impl Fn(
        &mut VisualTestAppContext,
        &Entity<KagiApp>,
        &Entity<EditorWorkspaceView>,
        AnyWindowHandle,
        &Path,
    ),
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let old_theme = theme::theme().slug.to_string();
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    crate::gui_evidence::fixture(&repo);
    for dimensions in [(940., 660.), (1392., 883.)] {
        for slug in ["apple-light", "apple-dark"] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    let state = e2e::app_state(&repo).expect("real editor fixture state");
                    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
                    let output = captured.clone();
                    let window = crate::macos::open_offscreen(
                        cx,
                        size(px(dimensions.0), px(dimensions.1)),
                        move |window, cx| e2e::mount_root(state, window, cx, &output),
                    )
                    .into();
                    let app = captured.borrow().clone().unwrap();
                    drop(captured);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        app.update(cx, |app, cx| {
                            app.set_theme(slug, cx);
                            app.open_editor_workspace(cx);
                        });
                        let editor = cx
                            .read(|cx| app.read(cx).ui().editor_workspace.clone())
                            .unwrap();
                        editor.update(cx, |view, cx| {
                            // Clean new previews replace the previous tab. Install only
                            // the retained path list; all content loads and selections
                            // below go through the real production activation boundary.
                            view.open_tabs =
                                PATHS.iter().map(|path| PathBuf::from(*path)).collect();
                            view.activate_tab(PATHS[0].into(), cx);
                        });
                        wait_for(cx, &editor, window, loaded);
                        scenario(cx, &app, &editor, window, &repo);
                        drop(editor);
                    }));
                    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
                    // Retire child Input/task owners before unmount drains the dispatcher.
                    cx.update_window(window, |_, window, cx| {
                        window.replace_root(cx, |window, cx| {
                            let empty = cx.new(|_| gpui::Empty);
                            gpui_component::Root::new(empty, window, cx)
                        });
                    })
                    .ok();
                    let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        unmount(cx, app, window);
                    }));
                    // A panic inside update_window can already remove the window.
                    // Preserve the oracle's original failure rather than cleanup's.
                    if let Err(payload) = result {
                        std::panic::resume_unwind(payload);
                    }
                    if let Err(payload) = cleanup {
                        std::panic::resume_unwind(payload);
                    }
                }
            }
        }
    }
}

pub fn scenario_editor_tab_overflow(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, _app, editor, window, repo| {
        let pane_widths = cx.read(|cx| {
            let view = editor.read(cx);
            (view.tree_w, view.hunks_w)
        });
        let mut first_offset = None;
        for index in [0, 11, 0, 1, 6, 11] {
            editor.update(cx, |view, cx| view.activate_tab(PATHS[index].into(), cx));
            wait_for(cx, editor, window, loaded);
            cx.read(|cx| {
                let view = editor.read(cx);
                assert_eq!(view.open_path.as_deref(), Some(Path::new(PATHS[index])));
                assert_eq!(view.open_tabs.len(), PATHS.len());
                let strip = view.tab_strip_bounds_for_e2e().expect("rendered horizontal tab viewport");
                let probes = view.tab_probes_for_e2e();
                assert_eq!(probes.len(), PATHS.len(), "all tabs remain laid out, not hidden or shrunk away");
                assert!(strip.size.width > theme::scaled_px(120.));
                for (probe, path) in probes.iter().zip(PATHS) {
                    assert_eq!(probe.path, path);
                    let full = repo.join(path).to_string_lossy().into_owned();
                    assert_eq!(probe.full_path, full, "rendered tab AX name is the full absolute path");
                    assert_eq!(probe.tooltip, full, "exact Tooltip text retains full path");
                    assert_eq!(probe.role, Role::Tab, "editor file has tab AX role");
                    assert!(probe.close_bounds.size.width >= px(20.) && probe.close_bounds.size.height >= px(20.), "close hitbox below 20px: {path}");
                    assert!(probe.bounds.size.width >= theme::scaled_px(120.) - px(1.), "tab compressed below scaled minimum: {path}");
                    assert!(probe.bounds.size.width <= theme::scaled_px(180.) + px(1.), "tab exceeded scaled maximum: {path}");
                }
                assert_eq!(probes[0].label, "src/a.rs");
                assert_eq!(probes[1].label, "tests/a.rs");
                assert_ne!(probes[0].label, probes[1].label);
                let extent = probes.last().unwrap().bounds.right() - probes[0].bounds.left();
                assert!(extent > strip.size.width + px(1.), "12 nonshrinking tabs must overflow actual viewport");
                let active = probes[index].bounds;
                assert!(active.left() >= strip.left() - px(1.) && active.right() <= strip.right() + px(1.), "selected tab {index} not fully revealed: active={active:?} viewport={strip:?} offset={:?}", view.tab_scroll_offset_for_e2e());
                let offset = view.tab_scroll_offset_for_e2e();
                assert!(offset.y.abs() <= px(1.), "tab overflow is horizontal only");
                if index == 0 {
                    assert!(offset.x.abs() <= px(1.), "first tab selection must return to horizontal origin");
                    first_offset = Some(offset.x);
                } else if index == 11 {
                    assert!(offset.x < first_offset.unwrap() - px(1.), "last tab selection must move real ScrollHandle offset");
                }
            });
        }
        assert_live_resize_reveal(cx, editor, window);
        assert_extra_tab_counts(cx, editor, window, repo);
        cx.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(
                (view.tree_w, view.hunks_w),
                pane_widths,
                "viewport clamping must not overwrite stored pane preferences"
            );
        });
        assert_compact_divider_drags(cx, editor, window);
    });
}

fn assert_extra_tab_counts(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    repo: &Path,
) {
    let paths = fixture_paths();
    for count in [1, 8, 30] {
        let retained = &paths[..count];
        editor.update(cx, |view, cx| {
            view.open_tabs = retained
                .iter()
                .map(|path| PathBuf::from(path.as_str()))
                .collect();
            view.activate_tab(PathBuf::from(&retained[0]), cx);
        });
        for index in [0, count - 1, 0] {
            editor.update(cx, |view, cx| {
                view.activate_tab(PathBuf::from(&retained[index]), cx)
            });
            wait_for(cx, editor, window, loaded);
            cx.read(|cx| {
                    let view = editor.read(cx);
                    assert_eq!(view.open_tabs.len(), count);
                    assert_eq!(view.open_path.as_deref(), Some(Path::new(&retained[index])));
                    let strip = view.tab_strip_bounds_for_e2e().expect("actual tab viewport");
                    assert!(strip.size.width >= theme::scaled_px(180.) - px(1.), "Editor fitting must reserve its full maximum-width tab, including beside a compact rail: {strip:?}");
                    let probes = view.tab_probes_for_e2e();
                    assert_eq!(probes.len(), count);
                    for (probe, path) in probes.iter().zip(retained) {
                        assert_eq!(&probe.path, path);
                        let full = repo.join(path).to_string_lossy().into_owned();
                        assert_eq!(probe.role, Role::Tab);
                        assert_eq!(probe.full_path, full, "exact AX identity at {count} tabs");
                        assert_eq!(probe.tooltip, full, "exact long-path tooltip at {count} tabs");
                        assert!(probe.bounds.size.width >= theme::scaled_px(120.) - px(1.), "compressed tab at {count} files: {path}");
                        assert!(probe.bounds.size.width <= theme::scaled_px(180.) + px(1.));
                        assert!(probe.close_bounds.size.width >= px(20.) && probe.close_bounds.size.height >= px(20.), "close hitbox below20px: {path}");
                    }
                    if count > 1 {
                        assert_eq!(probes[0].label, "src/a.rs");
                        assert_eq!(probes[1].label, "tests/a.rs");
                    } else {
                        assert_eq!(probes[0].label, "a.rs", "unambiguous basename stays short");
                    }
                    let active = probes[index].bounds;
                    assert!(active.left() >= strip.left() - px(1.) && active.right() <= strip.right() + px(1.), "selected tab {index}/{count} not fully revealed: active={active:?} viewport={strip:?} offset={:?}", view.tab_scroll_offset_for_e2e());
                    if count == 30 {
                        let extent = probes.last().unwrap().bounds.right() - probes[0].bounds.left();
                        assert!(extent > strip.size.width, "30 tabs have real horizontal overflow");
                    }
                    if index == 0 {
                        assert!(view.tab_scroll_offset_for_e2e().x.abs() <= px(1.), "first tab returns to origin at {count} files");
                    }
                });
        }
    }
}

fn assert_live_resize_reveal(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    // The previous activation loop ends on the last tab, with its real load
    // settled. Do not reactivate it or invalidate the view after resizing.
    let original = cx
        .update_window(window, |_, window, _| window.viewport_size())
        .unwrap();
    let widths = cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.open_path.as_deref(), Some(Path::new(PATHS[11])));
        (view.tree_w, view.hunks_w)
    });
    for wanted in [size(px(1700.), original.height), original] {
        let previous = cx.read(|cx| editor.read(cx).tab_strip_bounds_for_e2e().unwrap());
        cx.update_window(window, |_, window, _| window.resize(wanted))
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            crate::macos::drain_native_events();
            cx.run_until_parked();
            // Hidden NSWindows have no display link. This enters GPUI's actual
            // native frame callback, not paint(): no forced draw or refresh.
            // Only the production next-frame callback can dirty the reveal frame.
            crate::macos::request_native_frame(cx, window);
            cx.run_until_parked();
            let viewport = cx
                .update_window(window, |_, window, _| window.viewport_size())
                .unwrap();
            let ready = cx.read(|cx| {
                let view = editor.read(cx);
                assert_eq!(view.open_path.as_deref(), Some(Path::new(PATHS[11])));
                assert_eq!(view.open_tabs.len(), PATHS.len());
                assert_eq!((view.tree_w, view.hunks_w), widths);
                let strip = view.tab_strip_bounds_for_e2e().unwrap();
                let probes = view.tab_probes_for_e2e();
                let active = probes[11].bounds;
                viewport == wanted
                    && (strip.size.width - previous.size.width).abs() > px(1.)
                    && active.left() >= strip.left() - px(1.)
                    && active.right() <= strip.right() + px(1.)
            });
            if ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "native resize to {wanted:?} did not fully reveal the unchanged last tab"
            );
        }
    }
}

fn assert_compact_divider_drags(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let window_width = cx
        .update_window(window, |_, window, _| window.viewport_size().width)
        .unwrap();
    if window_width != px(940.) || (theme::zoom() - 1.67).abs() > 0.001 {
        return;
    }
    let left =
        e2e::control_bounds(window.window_id(), "compact-rail").map_or(px(0.), |rail| rail.right());
    let available = window_width - left;
    let widths = cx.read(|cx| {
        let view = editor.read(cx);
        assert!(view.show_tree);
        assert!(
            view.pane_width_fraction(available) < 1.,
            "compact panes really clamp"
        );
        (view.tree_w, view.hunks_w)
    });
    for tree in [true, false] {
        assert_one_pixel_divider_drag(cx, editor, window, left, available, tree);
        editor.update(cx, |view, cx| {
            (view.tree_w, view.hunks_w) = widths;
            cx.notify();
        });
        paint(cx, window);
    }
}

fn assert_one_pixel_divider_drag(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    left: Pixels,
    available: Pixels,
    tree: bool,
) {
    let (bounds, before, fraction) = cx.read(|cx| {
        let view = editor.read(cx);
        (
            view.divider_bounds_for_e2e(tree)
                .expect("actual native Editor divider"),
            (view.tree_w, view.hunks_w),
            f32::from(available - theme::scaled_px(180. + 8.))
                / theme::scaled(view.tree_w + view.hunks_w),
        )
    });
    let start = bounds.center();
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    // Cross GPUI's drag threshold vertically, so activation itself must not
    // convert the clamped presentation width into a smaller saved preference.
    let armed = point(start.x, start.y + px(20.));
    cx.simulate_mouse_move(window, armed, MouseButton::Left, Modifiers::none());
    assert!(
        cx.read(|cx| cx.has_active_drag()),
        "real divider drag activated"
    );
    paint(cx, window);
    cx.simulate_mouse_move(window, armed, MouseButton::Left, Modifiers::none());
    cx.read(|cx| {
        let view = editor.read(cx);
        assert!(
            (view.tree_w - before.0).abs() < 0.05,
            "zero-x drag shrank saved tree width"
        );
        assert!(
            (view.hunks_w - before.1).abs() < 0.05,
            "zero-x drag shrank saved hunks width"
        );
    });
    let end = point(armed.x + px(1.), armed.y);
    cx.simulate_mouse_move(window, end, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
    assert!(
        !cx.read(|cx| cx.has_active_drag()),
        "native mouse-up ends divider drag"
    );
    // Convert the actual cursor independently, including the measured rail.
    // Native divider bounds can round to half pixels: adding an idealized
    // delta to the preference would ignore that real press-coordinate rounding.
    let scale = theme::scaled(1.);
    let requested = if tree {
        f32::from(end.x - left) - 2. * scale
    } else {
        f32::from(left + available - end.x) - 2. * scale
    } / (scale * fraction);
    cx.read(|cx| {
        let view = editor.read(cx);
        let expected = if tree {
            (requested, before.1)
        } else {
            (before.0, requested)
        };
        assert!(
            if tree { view.tree_w > before.0 } else { view.hunks_w < before.1 },
            "the real one-pixel pointer move must change its pane preference"
        );
        assert!(
            (view.tree_w - expected.0).abs() < 0.05
                && (view.hunks_w - expected.1).abs() < 0.05,
            "1px Editor {} drag must change only its inverse-fraction preference: before={before:?}, actual={:?}, expected={expected:?}, fraction={fraction}",
            if tree { "Tree" } else { "Hunks" },
            (view.tree_w, view.hunks_w)
        );
    });
}

fn selector_bounds(window: AnyWindowHandle, middle: bool) -> [Bounds<Pixels>; 2] {
    let ids = if middle {
        ["editor-middle-selector-0", "editor-middle-selector-1"]
    } else {
        ["editor-right-selector-0", "editor-right-selector-1"]
    };
    ids.map(|id| {
        let bounds = e2e::control_bounds(window.window_id(), id)
            .unwrap_or_else(|| panic!("actual selector cell {id} is drawn"));
        assert!(
            bounds.size.height >= px(20.),
            "selector cell {id} has at least20px hit height"
        );
        bounds
    })
}

fn assert_selector_geometry(window: AnyWindowHandle, middle: bool, initial: &[Bounds<Pixels>; 2]) {
    for (actual, expected) in selector_bounds(window, middle).iter().zip(initial) {
        assert!(
            (actual.origin.x - expected.origin.x).abs() <= px(1.)
                && (actual.origin.y - expected.origin.y).abs() <= px(1.)
                && (actual.size.width - expected.size.width).abs() <= px(1.)
                && (actual.size.height - expected.size.height).abs() <= px(1.),
            "selector focus/selection ring must not reflow: actual={actual:?} initial={expected:?}"
        );
    }
}

fn assert_selector(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    middle: bool,
    selected: usize,
    commit: &str,
    initial: &[Bounds<Pixels>; 2],
) {
    paint(cx, window);
    assert_selector_geometry(window, middle, initial);
    let names = if middle {
        [
            Msg::EditorMiddleTabDiff.t(),
            Msg::EditorMiddleTabSnapshot.t(),
        ]
    } else {
        [Msg::EditorRightTabDiff.t(), Msg::EditorRightTabHistory.t()]
    };
    cx.update_window(window, |_, window, cx| {
        let view = editor.read(cx);
        let probes = view.selector_probes_for_e2e(middle);
        assert_eq!(probes.len(), 2);
        assert_eq!(
            probes.iter().filter(|probe| probe.2).count(),
            1,
            "one selected selector cell"
        );
        assert_eq!(
            probes.iter().filter(|probe| probe.3).count(),
            1,
            "one Tab-entry stop for the whole selector"
        );
        for (index, (role, name, active, tab_stop)) in probes.iter().enumerate() {
            assert_eq!(*role, Role::Tab, "native TabList child exposes tab AX role");
            assert_eq!(name.as_str(), names[index]);
            assert_eq!(*active, index == selected);
            assert_eq!(*tab_stop, index == selected);
        }
        assert_eq!(
            view.selector_focused_for_e2e(middle, window),
            Some(selected),
            "activation must keep focus inside selector, not root/editor Input"
        );
        assert_eq!(
            view.selected_history_commit.as_deref(),
            Some(commit),
            "pane switching must preserve chosen history commit"
        );
        if middle {
            assert_eq!(
                view.middle_tab,
                if selected == 0 {
                    MiddlePaneTab::Diff
                } else {
                    MiddlePaneTab::Snapshot
                }
            );
        } else {
            assert_eq!(
                view.right_tab,
                if selected == 0 {
                    RightPaneTab::Diff
                } else {
                    RightPaneTab::History
                }
            );
        }
    })
    .unwrap();
}

pub fn scenario_editor_toggle_keys(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, editor, window, repo| {
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |view, cx| {
                view.set_right_tab(RightPaneTab::History, window, cx)
            });
        })
        .unwrap();
        wait_for(cx, editor, window, |view| {
            view.history.is_some() && !view.history_loading()
        });
        let commit = rev(repo, "HEAD");
        cx.read(|cx| {
            let view = editor.read(cx);
            assert!(
                view.history
                    .as_ref()
                    .unwrap()
                    .entry_by_hash(&commit)
                    .is_some(),
                "real file-history read contains fixture commit"
            );
        });
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |view, cx| {
                view.select_history_commit(commit.clone(), window, cx)
            });
        })
        .unwrap();
        wait_for(cx, editor, window, |view| view.history_diff.is_some());
        for middle in [false, true] {
            exercise_selector(cx, app, editor, window, middle, &commit);
        }
        wait_for(cx, editor, window, |view| {
            view.history_diff.is_some() && view.snapshot.is_some()
        });
    });
}

fn exercise_selector(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    middle: bool,
    commit: &str,
) {
    if middle {
        editor.update(cx, |view, cx| view.set_middle_tab(MiddlePaneTab::Diff, cx));
    }
    paint(cx, window);
    let initial = selector_bounds(window, middle);
    let reached = cx
        .update_window(window, |_, window, cx| {
            let root = app
                .read(cx)
                .root_focus
                .clone()
                .expect("actual app focus root");
            root.focus(window, cx);
            window.draw(cx).clear();
            (0..128).find_map(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                editor.read(cx).selector_focused_for_e2e(middle, window)
            })
        })
        .unwrap();
    let selected = usize::from(!middle);
    assert_eq!(
        reached,
        Some(selected),
        "rendered Tab-stop traversal reaches only selected selector cell"
    );
    assert_selector(cx, editor, window, middle, selected, commit, &initial);
    for (key, selected) in [
        ("home", 0),
        ("right", 1),
        ("left", 0),
        ("end", 1),
        ("home", 0),
    ] {
        keys(cx, window, key);
        assert_selector(cx, editor, window, middle, selected, commit, &initial);
    }
    for (key, target) in [("space", 1), ("enter", 0)] {
        // Focus alone must not select; the real keyboard click activates.
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |view, cx| {
                view.focus_selector_for_e2e(middle, target, window, cx)
            });
        })
        .unwrap();
        paint(cx, window);
        assert_selector_geometry(window, middle, &initial);
        cx.read(|cx| assert!(!editor.read(cx).selector_probes_for_e2e(middle)[target].2));
        keys(cx, window, key);
        assert_selector(cx, editor, window, middle, target, commit, &initial);
    }
    if !middle {
        // Middle controls are rendered only with History active.
        keys(cx, window, "end");
        assert_selector(cx, editor, window, false, 1, commit, &initial);
    }
}
