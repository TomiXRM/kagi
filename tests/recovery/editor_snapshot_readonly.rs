//! #1264: actual History loads remain immutable; current buffers stay editable.
use crate::macos::{build_fixture, git, unmount};
use crate::pr_viewed::paint;
use gpui::{point, px, size, Entity, Modifiers, Role, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{EditorWorkspaceView, MiddlePaneTab, RightPaneTab};
use std::{cell::RefCell, rc::Rc};

const FILE: &str = "snapshot.rs";
const CURRENT: &str = "let current = \"editable working tree\";\n";

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
            assert!(!view.dirty);
            assert_eq!(
                view.editor.as_ref().unwrap().entity_id(),
                mounted.live.entity_id()
            );
            assert_eq!(mounted.live.read(cx).value().as_ref(), CURRENT);
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
            view.editor.as_ref().unwrap().entity_id(),
            mounted.live.entity_id()
        );
    });
    cx.simulate_keystrokes(mounted.window, "x");
    assert_ne!(
        cx.read(|cx| mounted.live.read(cx).value().to_string()),
        CURRENT,
        "Open current restores an editable buffer"
    );
    assert!(cx.read(|cx| mounted.editor.read(cx).dirty));
}

fn exercise(cx: &mut VisualTestAppContext, repo: &std::path::Path, original: &str, case: &Case) {
    eprintln!("[gui-e2e] snapshot case: {case:?}");
    let mounted = setup(cx, repo, case);
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
    eprintln!("[gui-e2e] PASS editor_snapshot_readonly EN/JA × 100/167% × Apple Light/Dark × 940×660/1392×883; actual History loads, selection/copy, mutation refusal, Open current");
}
