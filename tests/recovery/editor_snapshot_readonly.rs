//! #1264: actual History loads remain immutable; current buffers stay editable.
use crate::macos::{build_fixture, git, unmount};
use crate::pr_viewed::paint;
use gpui::{point, px, size, Entity, Modifiers, Role, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_git::{CommitId, OperationKind};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{EditorWorkspaceView, MiddlePaneTab, RightPaneTab};
use std::{cell::RefCell, rc::Rc};

const FILE: &str = "snapshot.rs";
const CURRENT: &str = "let current = \"editable working tree\";\n";
const DIRTY: &str = "w";

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn dirty_live(
    cx: &mut VisualTestAppContext,
    live: &Entity<gpui_component::input::InputState>,
    window: gpui::AnyWindowHandle,
) {
    use gpui::Focusable;
    cx.update_window(window, |_, window, cx| {
        window.focus(&live.read(cx).focus_handle(cx), cx)
    })
    .unwrap();
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_keystrokes(window, DIRTY);
    paint(cx, window);
    assert_eq!(cx.read(|cx| live.read(cx).value().to_string()), DIRTY);
}

fn seed_undo(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, repo: &std::path::Path) {
    let before = git_fixture::git_output(repo, &["rev-parse", "HEAD~1"]);
    let after = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    app.update(cx, |app, _| {
        app.record_history(
            OperationKind::Commit,
            "main",
            CommitId(before),
            CommitId(after),
            "historical snapshot fixture",
        );
        app.ui_mut().unwrap().history_seed_attempted = true;
        assert!(app.ui().operation_history.can_undo());
    });
}

fn assert_shortcuts(cx: &mut VisualTestAppContext, mounted: &Mounted) {
    for key in ["cmd-s", "cmd-z", "cmd-shift-z"] {
        cx.simulate_keystrokes(mounted.window, key);
        paint(cx, mounted.window);
        cx.read(|cx| {
            let app = mounted.app.read(cx);
            assert!(
                app.active_modal.is_none(),
                "Historical shortcut opened a modal: {key}"
            );
            assert!(
                app.ui().operation_history.can_undo(),
                "real undo entry remains available"
            );
            let view = mounted.editor.read(cx);
            assert!(
                view.dirty,
                "Historical Save cleared live dirty state: {key}"
            );
            assert_eq!(mounted.live.read(cx).value().as_ref(), DIRTY);
            assert_eq!(
                std::fs::read_to_string(view.repo_path.join(FILE)).unwrap(),
                CURRENT,
                "Historical Save wrote the hidden buffer: {key}"
            );
        });
    }
}

#[derive(Debug)]
struct Case {
    preset: &'static str,
    lang: Lang,
    zoom: f32,
    dimensions: (f32, f32),
}
struct Mounted {
    app: Entity<KagiApp>,
    editor: Entity<EditorWorkspaceView>,
    live: Entity<gpui_component::input::InputState>,
    window: gpui::AnyWindowHandle,
    hash: String,
}

fn wait_view(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: gpui::AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx))) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "production Editor read completed"
        );
        crate::macos::drain_native_events();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn copy(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) -> String {
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("COPY_NOT_DELIVERED".into()))
    });
    cx.simulate_keystrokes(window, "cmd-a");
    cx.simulate_keystrokes(window, "cmd-c");
    cx.read(|cx| {
        cx.read_from_clipboard()
            .expect("selected snapshot copied")
            .text()
            .unwrap()
    })
}

fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    case: &Case,
) -> (Entity<KagiApp>, gpui::AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        size(px(case.dimensions.0), px(case.dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let window: gpui::AnyWindowHandle = window.into();
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |app, cx| app.set_theme(case.preset, cx));
    i18n::set_lang(case.lang);
    theme::set_zoom(case.zoom);
    cx.update(theme::sync_gpui_component_theme);
    (app, window)
}

fn select_snapshot(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: gpui::AnyWindowHandle,
) -> String {
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |view, cx| {
            view.set_right_tab(RightPaneTab::History, window, cx)
        })
    })
    .unwrap();
    wait_view(cx, editor, window, |view| {
        view.history.is_some() && !view.history_loading()
    });
    let hash = cx.read(|cx| {
        editor.read(cx).history.as_ref().unwrap().entries[0]
            .commit
            .as_ref()
            .unwrap()
            .full_hash
            .clone()
    });
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |view, cx| {
            view.set_middle_tab(MiddlePaneTab::Snapshot, cx);
            view.select_history_commit(hash.clone(), window, cx);
        })
    })
    .unwrap();
    wait_view(cx, editor, window, |view| {
        view.snapshot.is_some() && !view.snapshot_loading() && view.snapshot_bounds.is_some()
    });
    hash
}

fn setup(cx: &mut VisualTestAppContext, repo: &std::path::Path, case: &Case) -> Mounted {
    let (app, window) = mount_at(cx, repo, case);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab(FILE.into(), cx));
    wait_view(cx, &editor, window, |view| {
        !view.file_loading && view.editor.is_some() && view.content.is_some()
    });
    let live = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
    assert_eq!(cx.read(|cx| live.read(cx).value().to_string()), CURRENT);
    dirty_live(cx, &live, window);
    seed_undo(cx, &app, repo);
    let hash = select_snapshot(cx, &editor, window);
    // PM-approved compact leg: Toggle Sidebar only while #1262's shared
    // Editor width clamp is absent. After integration the natural layout is
    // checked instead. In either case the History pane stays visible.
    if case.zoom > 1.
        && case.dimensions.0 == 940.
        && cx.read(|cx| editor.read(cx).snapshot_bounds.unwrap().size.width < px(60.))
    {
        cx.simulate_keystrokes(window, "cmd-b");
        paint(cx, window);
        assert!(cx.read(|cx| !editor.read(cx).show_tree));
        assert_eq!(
            cx.read(|cx| editor.read(cx).right_tab),
            RightPaneTab::History
        );
    }
    paint(cx, window);
    assert_eq!(theme::theme().slug.as_ref(), case.preset);
    assert_eq!(i18n::lang(), case.lang);
    assert!((theme::zoom() - case.zoom).abs() < 0.001);
    Mounted {
        app,
        editor,
        live,
        window,
        hash,
    }
}

fn assert_header(cx: &mut VisualTestAppContext, mounted: &Mounted) -> gpui::Bounds<gpui::Pixels> {
    let (bounds, indicator, current) = cx.read(|cx| {
        let view = mounted.editor.read(cx);
        (
            view.snapshot_bounds.unwrap(),
            view.snapshot_indicator.clone().unwrap(),
            view.snapshot_current.clone().unwrap(),
        )
    });
    assert_eq!(indicator.role, Role::Label);
    assert!(indicator.name.contains(Msg::EditorSnapshotReadOnly.t()));
    assert!(indicator.name.contains(FILE) && indicator.name.contains(&mounted.hash));
    assert_eq!(indicator.tooltip.as_ref(), Some(&indicator.name));
    cx.simulate_mouse_move(
        mounted.window,
        indicator.bounds.center(),
        None,
        Modifiers::none(),
    );
    paint(cx, mounted.window);
    cx.advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();
    paint(cx, mounted.window);
    wait_view(cx, &mounted.editor, mounted.window, |view| {
        view.snapshot_tooltip_shown.is_some()
    });
    assert_eq!(
        cx.read(|cx| mounted.editor.read(cx).snapshot_tooltip_shown.clone()),
        Some(indicator.name.clone()),
        "the real tooltip builder exposes complete identity"
    );
    cx.simulate_mouse_move(
        mounted.window,
        bounds.origin + point(px(2.), px(2.)),
        None,
        Modifiers::none(),
    );
    paint(cx, mounted.window);
    assert_eq!(current.role, Role::Button);
    assert_eq!(current.name.as_ref(), Msg::EditorOpenCurrent.t());
    assert!(indicator.bounds.size.height > px(0.) && current.bounds.size.height > px(0.));
    assert!(current.bounds.size.width >= px(20.));
    assert!(
        bounds.size.width >= px(60.) && bounds.size.height >= px(60.),
        "readable snapshot viewport: {bounds:?}; indicator {:?}; Open current {:?}",
        indicator.bounds,
        current.bounds
    );
    bounds
}

fn assert_mutations(cx: &mut VisualTestAppContext, mounted: &Mounted, original: &str) -> String {
    // The pinned Root trims outer whitespace when copying selectable text.
    let before = copy(cx, mounted.window);
    assert_eq!(
        before,
        original.trim(),
        "literal JA/code/fences remain selectable"
    );
    for key in [
        "x",
        "cmd-v",
        "cmd-x",
        "backspace",
        "delete",
        "cmd-z",
        "cmd-shift-z",
        "cmd-s",
        "あ",
    ] {
        cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("PASTE".into())));
        cx.simulate_keystrokes(mounted.window, key);
        paint(cx, mounted.window);
        assert!(
            cx.read(|cx| mounted.app.read(cx).active_modal.is_none()),
            "Snapshot shortcut opened a modal: {key}"
        );
        assert_eq!(
            copy(cx, mounted.window),
            before,
            "Snapshot accepted a discarded edit: {key}"
        );
        cx.read(|cx| {
            let view = mounted.editor.read(cx);
            assert_eq!(
                view.snapshot.as_ref().unwrap().content.as_deref(),
                Some(original)
            );
            assert!(view.dirty);
            assert_eq!(
                view.editor.as_ref().unwrap().entity_id(),
                mounted.live.entity_id()
            );
            assert_eq!(mounted.live.read(cx).value().as_ref(), DIRTY);
            assert_eq!(
                std::fs::read_to_string(view.repo_path.join(FILE)).unwrap(),
                CURRENT
            );
        });
    }
    before
}

fn return_current(cx: &mut VisualTestAppContext, mounted: &Mounted) {
    let current = cx.read(|cx| mounted.editor.read(cx).snapshot_current.clone().unwrap());
    cx.simulate_click(mounted.window, current.bounds.center(), Modifiers::none());
    paint(cx, mounted.window);
    cx.read(|cx| {
        let view = mounted.editor.read(cx);
        assert_eq!(
            view.right_tab,
            RightPaneTab::Diff,
            "Open current hit {:?}",
            current.bounds
        );
        assert!(view.selected_history_commit.is_none());
        assert_eq!(
            mounted.live.read(cx).value().as_ref(),
            DIRTY,
            "Open current preserves unsaved text"
        );
        assert_eq!(
            view.editor.as_ref().unwrap().entity_id(),
            mounted.live.entity_id()
        );
    });
    cx.simulate_keystrokes(mounted.window, "x");
    assert_ne!(
        cx.read(|cx| mounted.live.read(cx).value().to_string()),
        DIRTY,
        "Open current restores an editable buffer"
    );
    assert!(cx.read(|cx| mounted.editor.read(cx).dirty));
}

fn exercise(cx: &mut VisualTestAppContext, repo: &std::path::Path, original: &str, case: &Case) {
    eprintln!("[gui-e2e] snapshot case: {case:?}");
    let mounted = setup(cx, repo, case);
    assert_shortcuts(cx, &mounted); // commit selection left root focus; no body click yet.
    let bounds = assert_header(cx, &mounted);
    cx.simulate_click(
        mounted.window,
        bounds.origin
            + point(
                px((24. * case.zoom).min(f32::from(bounds.size.width) / 2.)),
                px(24. * case.zoom),
            ),
        Modifiers::none(),
    );
    assert_shortcuts(cx, &mounted); // focused reader must protect the same dirty owner.
    let before = assert_mutations(cx, &mounted, original);
    cx.simulate_event(
        mounted.window,
        gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-600.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    paint(cx, mounted.window);
    assert_eq!(
        copy(cx, mounted.window),
        before,
        "scroll retains selectable snapshot"
    );
    return_current(cx, &mounted);
    let Mounted {
        app,
        editor,
        live,
        window,
        ..
    } = mounted;
    drop(live);
    drop(editor);
    unmount(cx, app, window);
}

fn warm_draw(cx: &mut VisualTestAppContext, mounted: &Mounted) -> std::time::Duration {
    let state = cx.read(|cx| mounted.editor.read(cx).snapshot_text_state().unwrap());
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        state.update(cx, |_, cx| cx.notify());
        let started = std::time::Instant::now();
        cx.update_window(mounted.window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    samples[3]
}

fn measure_snapshot(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    original: &str,
) -> std::time::Duration {
    let case = Case {
        preset: "apple-light",
        lang: Lang::En,
        zoom: 1.,
        dimensions: (1392., 883.),
    };
    let mounted = setup(cx, repo, &case);
    let bounds = cx.read(|cx| mounted.editor.read(cx).snapshot_bounds.unwrap());
    let position = bounds.origin + point(px(24.), px(24.));
    cx.simulate_click(mounted.window, position, Modifiers::none());
    let copied = copy(cx, mounted.window);
    assert!(
        copied == original.trim(),
        "chunked whole-document selection must preserve source: copied {} bytes, expected {}",
        copied.len(),
        original.trim().len()
    );
    cx.simulate_click(mounted.window, position, Modifiers::none());
    paint(cx, mounted.window);
    // Initial parse/measure and selection are settled before forced warm draws.
    let elapsed = warm_draw(cx, &mounted);
    let Mounted {
        app,
        editor,
        live,
        window,
        ..
    } = mounted;
    drop(live);
    drop(editor);
    unmount(cx, app, window);
    elapsed
}

fn large_snapshot(cx: &mut VisualTestAppContext, repo: &std::path::Path, small: &str) {
    let small_time = measure_snapshot(cx, repo, small);
    let mut large = String::with_capacity(900_000);
    for line in 0..20_000 {
        large.push_str(&format!("let historical_line_{line} = {line}; // 日本語\n"));
    }
    std::fs::write(repo.join(FILE), &large).unwrap();
    git(repo, &["add", FILE]);
    git(
        repo,
        &["commit", "-q", "-m", "20,000-line historical snapshot"],
    );
    std::fs::write(repo.join(FILE), CURRENT).unwrap();
    let large_time = measure_snapshot(cx, repo, &large);
    let limit = small_time * 8 + std::time::Duration::from_millis(15);
    assert!(large_time <= limit, "196× source growth must not make warm redraw linear: small={small_time:?}, 20k={large_time:?}, limit={limit:?}");
    eprintln!("[gui-e2e] snapshot warm redraw median/7 forced TextView draws: 102 lines={small_time:?}, 20,000 lines={large_time:?}, limit={limit:?}; full-document copy preserved");
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let mut original = "let greeting = \"こんにちは <&>\";\n// ````` literal fence\n".to_string();
    for line in 0..100 {
        original.push_str(&format!("let line_{line} = {line}; // 日本語\n"));
    }
    std::fs::write(fixture.path().join(FILE), &original).unwrap();
    git(fixture.path(), &["add", FILE]);
    git(
        fixture.path(),
        &["commit", "-q", "-m", "historical snapshot"],
    );
    std::fs::write(fixture.path().join(FILE), CURRENT).unwrap();
    for preset in ["apple-light", "apple-dark"] {
        for lang in [Lang::En, Lang::Ja] {
            for zoom in [1., 1.67] {
                for dimensions in [(940., 660.), (1392., 883.)] {
                    exercise(
                        cx,
                        fixture.path(),
                        &original,
                        &Case {
                            preset,
                            lang,
                            zoom,
                            dimensions,
                        },
                    );
                }
            }
        }
    }
    large_snapshot(cx, fixture.path(), &original);
    eprintln!("[gui-e2e] PASS editor_snapshot_readonly EN/JA × 100/167% × Apple Light/Dark × 940×660/1392×883; actual History loads, selection/copy, mutation refusal, Open current");
}
