//! #1684: consumer-visible editor details disclosure, independent of Graph.
//! Uses baseline APIs; the missing measured header control is the pre-fix oracle.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::native_ax;
use crate::pr_viewed::paint;
use crate::surface_scrollbars::PresentationRestore;
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Focusable, Modifiers, MouseButton, Pixels,
    VisualTestAppContext,
};
use kagi::ui::e2e::system_accessibility::SystemDisplayInjection;
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    motion, theme,
};
use kagi_ui_editor::{EditorWorkspaceView, MiddlePaneTab, RightPaneTab};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const TOGGLE: &str = "editor-details-toggle";
const PANE: &str = "editor-details-pane";
const DIVIDER: &str = "editor-details-divider";
const TEXT: &str = "details.txt";
const PDF: &str = "reading.pdf";

struct MotionRestore {
    enabled: bool,
    _saved: crate::gui_isolation::SavedKeys,
}
impl MotionRestore {
    fn capture() -> Self {
        Self {
            enabled: theme::app_reduce_motion(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]),
        }
    }
}
impl Drop for MotionRestore {
    fn drop(&mut self) {
        motion::set_clock_for_e2e(None);
        theme::set_reduce_motion(self.enabled);
    }
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for id in [TOGGLE, PANE, DIVIDER] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("actual visible editor control missing: {id}"))
}

fn wait(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView, &gpui::App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx), cx)) {
            draw(cx, window);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "editor document read did not settle"
        );
        crate::macos::drain_native_events();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn command(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(id, window, cx));
    })
    .unwrap();
    draw(cx, window);
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.simulate_click(window, bounds(window, TOGGLE).center(), Modifiers::none());
    draw(cx, window);
}

fn center_width(cx: &VisualTestAppContext, editor: &Entity<EditorWorkspaceView>) -> Pixels {
    cx.read(|cx| {
        editor
            .read(cx)
            .tab_strip_bounds_for_e2e()
            .unwrap()
            .size
            .width
    })
}

fn close(actual: Pixels, expected: Pixels, reason: &str) {
    assert!(
        (actual - expected).abs() <= px(1.5),
        "{reason}: {actual:?} != {expected:?}"
    );
}

fn hidden(window: AnyWindowHandle) {
    bounds(window, TOGGLE);
    for id in [PANE, DIVIDER] {
        assert!(
            e2e::control_bounds(window.window_id(), id).is_none_or(|b| b.size.width <= px(0.5)),
            "settled hidden details must remove {id}"
        );
    }
}

fn focus_toggle(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    // Enter navigation from the workspace, not from the code editor, where Tab indents.
    cx.update_window(window, |_, window, cx| {
        let root = editor.read(cx).root_focus.clone();
        window.focus(&root, cx);
    })
    .unwrap();
    // Obtain the actual component's semantic label, not a duplicate localization oracle.
    let node = native_ax::node(cx, window, TOGGLE.into());
    assert_eq!(node["aria"]["role"], "Button");
    let label = node["aria"]["label"]
        .as_str()
        .expect("named header button")
        .to_owned();
    for _ in 0..160 {
        keys(cx, window, "tab");
        draw(cx, window);
        let tree = native_ax::tree(cx, window);
        if let Some(id) = tree["gpui_focus"].as_str() {
            if tree["nodes"][id]["aria"]["label"] == label {
                assert_eq!(tree["nodes"][id]["aria"]["role"], "Button");
                return;
            }
        }
    }
    panic!("real Tab never reaches the visible details button");
}

fn assert_no_right_focus(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        assert_eq!(
            editor.read(cx).selector_focused_for_e2e(false, window),
            None,
            "hidden selector cannot retain native focus"
        );
    })
    .unwrap();
}

fn resize(cx: &mut VisualTestAppContext, window: AnyWindowHandle, width: f32, height: f32) {
    let wanted = size(px(width), px(height));
    cx.update_window(window, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        crate::macos::drain_native_events();
        paint(cx, window);
        if cx
            .update_window(window, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native viewport resize timed out"
        );
    }
}

fn open(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    path: &str,
) {
    editor.update(cx, |view, cx| view.open_tab(path.into(), cx));
    wait(cx, editor, window, |view, _| {
        !view.file_loading && view.open_path.as_deref() == Some(Path::new(path))
    });
}

fn editor(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Entity<EditorWorkspaceView> {
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    wait(cx, &editor, window, |view, _| !view.loading);
    editor
}

fn drag_details(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let start = bounds(window, DIVIDER).center();
    let before = cx.read(|cx| (editor.read(cx).tree_w, editor.read(cx).hunks_w));
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    let armed = start + point(px(0.), px(20.));
    cx.simulate_mouse_move(window, armed, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    let end = armed + point(px(-5.), px(0.));
    cx.simulate_mouse_move(window, end, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
    draw(cx, window);
    cx.read(|cx| {
        let view = editor.read(cx);
        assert!(
            view.hunks_w > before.1,
            "real right divider drag updates retained width"
        );
        assert_eq!(
            view.tree_w, before.0,
            "right divider cannot mutate tree preference"
        );
    });
}

pub fn scenario_editor_details_controls(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let _motion = MotionRestore::capture();
    theme::set_reduce_motion(true);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["sidebar_visible"]);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join(TEXT), "original details text\n").unwrap();
    // Pairwise presentation legs cover both dimensions, languages, scales and themes.
    for (preset, lang, zoom, width, height) in [
        ("apple-light", Lang::En, 1., 1392., 883.),
        ("apple-dark", Lang::Ja, 1.67, 940., 660.),
        ("apple-light", Lang::Ja, 1., 940., 660.),
        ("apple-dark", Lang::En, 1.67, 1392., 883.),
    ] {
        assert!(theme::set_active(preset));
        i18n::set_lang(lang);
        theme::set_zoom(zoom);
        cx.update(theme::sync_gpui_component_theme);
        let (app, window) = mount(cx, fixture.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                resize(cx, window, width, height);
                let editor = editor(cx, &app, window);
                // The recovery control must also exist before a file is selected.
                bounds(window, TOGGLE);
                open(cx, &editor, window, TEXT);
                native_ax::activate(cx);
                let graph = cx.read(|cx| app.read(cx).inspector_visible);
                for without_tree in [false, true] {
                    if without_tree {
                        command(cx, &app, window, "view.toggleSidebar");
                    }
                    draw(cx, window);
                    drag_details(cx, &editor, window);
                    let before = center_width(cx, &editor);
                    let before_right =
                        cx.read(|cx| editor.read(cx).tab_strip_bounds_for_e2e().unwrap().right());
                    let pane = bounds(window, PANE);
                    let divider = bounds(window, DIVIDER);
                    let cached = cx.read(|cx| editor.read(cx).hunks_w);
                    assert!(pane.size.width > px(1.) && divider.size.width > px(0.));
                    click(cx, window);
                    hidden(window);
                    let after_right =
                        cx.read(|cx| editor.read(cx).tab_strip_bounds_for_e2e().unwrap().right());
                    close(
                        after_right - before_right,
                        pane.size.width + divider.size.width,
                        "hiding moves the center's trailing edge into the removed details space",
                    );
                    assert!(center_width(cx, &editor) > before,
                    "details disclosure increases document width even when the tree decompresses");
                    assert_eq!(cx.read(|cx| editor.read(cx).hunks_w), cached);
                    assert_eq!(cx.read(|cx| app.read(cx).inspector_visible), graph);
                    focus_toggle(cx, &editor, window);
                    keys(cx, window, "enter");
                    draw(cx, window);
                    close(
                        bounds(window, PANE).size.width,
                        pane.size.width,
                        "Enter restores saved details width",
                    );
                    close(
                        center_width(cx, &editor),
                        before,
                        "restoration returns center geometry",
                    );
                    focus_toggle(cx, &editor, window);
                    keys(cx, window, "space");
                    draw(cx, window);
                    hidden(window);
                    // Inspect the complete focus tree, including stops after the code editor,
                    // without inserting indentation into that editor with repeated Tab keys.
                    for _ in 0..40 {
                        cx.update_window(window, |_, window, cx| window.focus_next(cx))
                            .unwrap();
                        draw(cx, window);
                        assert_no_right_focus(cx, &editor, window);
                    }
                    command(cx, &app, window, "view.toggleEditorDetails");
                    close(
                        bounds(window, PANE).size.width,
                        pane.size.width,
                        "host command restores details",
                    );
                    drag_details(cx, &editor, window);
                }
                // Existing Graph command may change only Graph state, never Editor geometry.
                let shown = bounds(window, PANE);
                command(cx, &app, window, "view.toggleCommitDetails");
                assert_ne!(cx.read(|cx| app.read(cx).inspector_visible), graph);
                close(
                    bounds(window, PANE).size.width,
                    shown.size.width,
                    "Graph details is independent",
                );
                command(cx, &app, window, "view.toggleCommitDetails");
                if !cx.read(|cx| editor.read(cx).show_tree) {
                    command(cx, &app, window, "view.toggleSidebar");
                }
                drop(editor);
            }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

pub fn scenario_editor_details_documents(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let _motion = MotionRestore::capture();
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join(TEXT), "historical details text\n").unwrap();
    std::fs::write(
        repo.join("preview.md"),
        "# Retained preview\n\nBody text.\n",
    )
    .unwrap();
    std::fs::write(
        repo.join(PDF),
        include_bytes!("../fixtures/pdf/document.pdf"),
    )
    .unwrap();
    std::fs::write(
        repo.join("picture.png"),
        crate::diff_highlight::png_bytes([40, 120, 180, 255]),
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "details documents"]);
    std::fs::write(repo.join(TEXT), "current details text\n").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let (app, window) = mount(cx, repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let editor = editor(cx, &app, window);
        open(cx, &editor, window, TEXT);
        let live = cx.read(|cx| editor.read(cx).editor.clone().unwrap());
        cx.update_window(window, |_, window, cx| {
            window.focus(&live.read(cx).focus_handle(cx), cx)
        })
        .unwrap();
        keys(cx, window, "cmd-a");
        cx.simulate_keystrokes(window, "x");
        paint(cx, window);
        assert!(cx.read(|cx| editor.read(cx).dirty));
        let value = cx.read(|cx| live.read(cx).value().to_string());
        // Use the real default shortcut while the document owns native focus.
        keys(cx, window, "cmd-alt-e");
        draw(cx, window);
        hidden(window);
        cx.update_window(window, |_, window, cx| {
            assert!(
                live.read(cx).focus_handle(cx).is_focused(window),
                "hiding another pane preserves center focus"
            );
        })
        .unwrap();
        keys(cx, window, "cmd-alt-e");
        draw(cx, window);
        bounds(window, PANE);
        assert_eq!(cx.read(|cx| live.read(cx).value().to_string()), value);
        assert_eq!(
            cx.read(|cx| editor.read(cx).editor.as_ref().unwrap().entity_id()),
            live.entity_id()
        );
        // Select actual loaded History and retain its center Snapshot on disclosure.
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |view, cx| {
                view.set_right_tab(RightPaneTab::History, window, cx)
            })
        })
        .unwrap();
        wait(cx, &editor, window, |view, _| {
            view.history.is_some() && !view.history_loading()
        });
        let history = cx.read(|cx| editor.read(cx).history.clone().unwrap());
        let hash = history.entries[0]
            .commit
            .as_ref()
            .unwrap()
            .full_hash
            .clone();
        cx.update_window(window, |_, window, cx| {
            editor.update(cx, |view, cx| {
                view.select_history_commit(hash.clone(), window, cx);
                view.set_middle_tab(MiddlePaneTab::Snapshot, cx);
            })
        })
        .unwrap();
        wait(cx, &editor, window, |view, _| {
            view.snapshot.is_some()
                && !view.snapshot_loading()
                && view.snapshot_text_state().is_some()
        });
        let snapshot = cx.read(|cx| editor.read(cx).snapshot_text_state().unwrap());
        let initial = cx.read(|cx| editor.read(cx).snapshot_bounds.unwrap());
        // Real Tab reaches a rendered right selector; the disclosure command must release it.
        let label = cx.read(|cx| editor.read(cx).selector_probes_for_e2e(false)[1].1.clone());
        native_ax::activate(cx);
        native_ax::focus_named(cx, window, "Tab", &label);
        command(cx, &app, window, "view.toggleEditorDetails");
        hidden(window);
        assert_no_right_focus(cx, &editor, window);
        cx.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.right_tab, RightPaneTab::History);
            assert_eq!(view.middle_tab, MiddlePaneTab::Snapshot);
            assert_eq!(view.selected_history_commit.as_deref(), Some(hash.as_str()));
            assert!(view.showing_history_commit());
            assert!(Arc::ptr_eq(view.history.as_ref().unwrap(), &history));
            assert_eq!(
                view.snapshot_text_state().unwrap().entity_id(),
                snapshot.entity_id()
            );
            assert!(view.snapshot_bounds.unwrap().size.width > initial.size.width + px(1.));
            assert!(view.dirty);
            assert_eq!(live.read(cx).value().to_string(), value);
        });
        command(cx, &app, window, "view.toggleEditorDetails");
        // Diff is a separate middle presentation, not a synonym for Snapshot.
        editor.update(cx, |view, cx| view.set_middle_tab(MiddlePaneTab::Diff, cx));
        wait(cx, &editor, window, |view, _| {
            view.history_diff.is_some() && !view.history_diff_loading()
        });
        click(cx, window);
        assert!(cx
            .read(|cx| editor.read(cx).showing_history_commit()
                && editor.read(cx).history_diff.is_some()));
        assert_eq!(
            cx.read(|cx| editor.read(cx).selected_history_commit.clone()),
            Some(hash)
        );
        // Visibility belongs to the Editor, not a file/tab presentation.
        open(cx, &editor, window, "preview.md");
        editor.update(cx, |view, cx| view.set_markdown_preview(true, cx));
        draw(cx, window);
        hidden(window);
        assert!(cx.read(|cx| editor.read(cx).markdown_preview_visible()));
        let markdown = cx.read(|cx| editor.read(cx).content.clone());
        click(cx, window);
        assert!(cx.read(|cx| editor.read(cx).markdown_preview_visible()));
        assert_eq!(cx.read(|cx| editor.read(cx).content.clone()), markdown);
        open(cx, &editor, window, "picture.png");
        wait(cx, &editor, window, |view, _| view.content_image.is_some());
        let image = cx.read(|cx| editor.read(cx).content_image.clone().unwrap());
        click(cx, window);
        click(cx, window);
        assert!(cx.read(|cx| Arc::ptr_eq(editor.read(cx).content_image.as_ref().unwrap(), &image)));
        open(cx, &editor, window, PDF);
        wait(cx, &editor, window, |view, cx| {
            view.pdf_probe(cx).is_some_and(|pdf| pdf.settled)
        });
        native_ax::focus_named(cx, window, "Button", Msg::EditorPdfNext.t());
        keys(cx, window, "space");
        wait(cx, &editor, window, |view, cx| {
            view.pdf_probe(cx)
                .is_some_and(|pdf| pdf.settled && pdf.page == 1)
        });
        native_ax::focus_named(cx, window, "Button", Msg::EditorPdfZoomIn.t());
        for _ in 0..4 {
            keys(cx, window, "space");
            wait(cx, &editor, window, |view, cx| {
                view.pdf_probe(cx).is_some_and(|pdf| pdf.settled)
            });
        }
        let pdf = cx.read(|cx| editor.read(cx).pdf_probe(cx).unwrap());
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: pdf.scroll.bounds().center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-80.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        paint(cx, window);
        let offset = pdf.scroll.offset();
        assert!(offset.y < px(0.), "PDF fixture has a real reading position");
        for _ in 0..2 {
            click(cx, window);
            let now = cx.read(|cx| editor.read(cx).pdf_probe(cx).unwrap());
            assert_eq!(
                (now.page, now.zoom, now.request),
                (pdf.page, pdf.zoom, pdf.request)
            );
            assert!(Arc::ptr_eq(
                now.image.as_ref().unwrap(),
                pdf.image.as_ref().unwrap()
            ));
            close(
                now.scroll.offset().y,
                offset.y,
                "details disclosure preserves PDF read position",
            );
            assert!(now.settled && !now.in_flight);
        }
        open(cx, &editor, window, TEXT);
        assert!(cx.read(|cx| editor.read(cx).dirty));
        assert_eq!(
            cx.read(|cx| editor.read(cx).editor.as_ref().unwrap().entity_id()),
            live.entity_id()
        );
        assert_eq!(cx.read(|cx| live.read(cx).value().to_string()), value);
        assert_eq!(
            std::fs::read_to_string(repo.join(TEXT)).unwrap(),
            "current details text\n"
        );
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        editor.update(cx, |view, cx| view.request_close_tab(TEXT.into(), cx));
        assert!(
            cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()),
            "retained unsaved text still participates in the ordinary close guard"
        );
        drop(snapshot);
        drop(live);
        drop(editor);
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn frame(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    at: Instant,
) {
    motion::set_clock_for_e2e(Some(at));
    editor.update(cx, |_, cx| cx.notify());
    draw(cx, window);
}

pub fn scenario_editor_details_motion(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    let _motion = MotionRestore::capture();
    let _system = SystemDisplayInjection::new(false, false);
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join(TEXT), "motion retained document\n").unwrap();
    let (app, window) = mount(cx, fixture.path());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let editor = editor(cx, &app, window);
        open(cx, &editor, window, TEXT);
        let t = Instant::now();
        frame(cx, &editor, window, t);
        let full = bounds(window, PANE).size.width;
        let center = center_width(cx, &editor);
        let divider = bounds(window, DIVIDER).center();
        let cached = cx.read(|cx| editor.read(cx).hunks_w);
        command(cx, &app, window, "view.toggleEditorDetails");
        frame(cx, &editor, window, t + Duration::from_millis(60));
        let middle = bounds(window, PANE).size.width;
        assert!(
            middle > px(0.) && middle < full,
            "native closing clip has an intermediate width"
        );
        assert!(
            center_width(cx, &editor) > center,
            "center grows during close"
        );
        // A pointer at the old divider cannot mutate cached width during close.
        cx.simulate_mouse_down(window, divider, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            window,
            divider + point(px(-45.), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            window,
            divider + point(px(-45.), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert_eq!(cx.read(|cx| editor.read(cx).hunks_w), cached);
        command(cx, &app, window, "view.toggleEditorDetails");
        frame(cx, &editor, window, t + Duration::from_millis(60));
        close(
            bounds(window, PANE).size.width,
            middle,
            "reversal starts continuously at current clip",
        );
        frame(cx, &editor, window, t + Duration::from_millis(100));
        let opening = bounds(window, PANE).size.width;
        assert!(
            opening > middle && opening <= full,
            "interrupted close reverses toward shown"
        );
        frame(cx, &editor, window, t + Duration::from_secs(1));
        close(
            bounds(window, PANE).size.width,
            full,
            "motion restores settled width",
        );
        theme::set_reduce_motion(true);
        command(cx, &app, window, "view.toggleEditorDetails");
        hidden(window);
        command(cx, &app, window, "view.toggleEditorDetails");
        close(
            bounds(window, PANE).size.width,
            full,
            "reduced motion settles on the first frame",
        );
        drop(editor);
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
