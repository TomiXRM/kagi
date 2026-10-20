//! Real Editor entry paths and hayro pixels; no source Input or write admission.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_viewed::{paint, rev};
use crate::{keyboard_nav::keys, native_ax};
use gpui::{AnyWindowHandle, Entity, Focusable as _, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::editor_workspace::EditorWorkspaceView;
use kagi_git::{CommitId, OperationKind};
use kagi_ui_core::{
    file_tree::TreeRow,
    i18n::{self, Lang, Msg},
};
use kagi_ui_editor::{EditorWorkspaceEvent, MiddlePaneTab, PdfProbe, RightPaneTab, TreeSource};
use std::{cell::Cell, path::Path, rc::Rc, sync::Arc};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

const PDF_PATH: &str = "b-document.PDF";
const NOTE: &str = "a-note.txt";
const SHORT_PDF_PATH: &str = "short-pages.pdf";
const PDF: &[u8] = include_bytes!("../fixtures/pdf/document.pdf");
const REPLACEMENT: &[u8] = include_bytes!("../fixtures/pdf/japanese-fallback.pdf");

fn wait_for(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView, &gpui::App) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.advance_clock(std::time::Duration::from_millis(16));
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx), cx)) {
            paint(cx, window);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real PDF Editor work did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn wait_pdf(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) -> PdfProbe {
    wait_for(cx, editor, window, |view, cx| {
        !view.file_loading && view.pdf_probe(cx).is_some_and(|p| p.settled)
    });
    cx.read(|cx| editor.read(cx).pdf_probe(cx).unwrap())
}
fn click_file(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    path: &str,
) {
    paint(cx, window);
    let bounds = cx.read(|cx| {
        let view = editor.read(cx);
        let file = view
            .files
            .iter()
            .position(|file| file.path == Path::new(path))
            .unwrap();
        let row = view
            .tree
            .iter()
            .position(|row| matches!(row, TreeRow::File { file_index, .. } if *file_index == file))
            .unwrap();
        view.tree_row_probe_for_e2e(row)
            .unwrap()
            .row
            .unwrap()
            .bounds
    });
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.simulate_mouse_down(
        window,
        bounds.center(),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        window,
        bounds.center(),
        MouseButton::Left,
        Modifiers::none(),
    );
}
fn assert_pixels(image: &gpui::RenderImage) {
    let bytes = image.as_bytes(0).unwrap();
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|p| p[0] < 80 && p[1] < 80 && p[2] < 80)
            .count()
            > 200,
        "real document glyph/graphic pixels, not merely Some(image)"
    );
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|p| p[0].abs_diff(p[2]) > 40)
            .count()
            > 100,
        "actual colored page content must reach the BGRA raster"
    );
}

fn short_pages_pdf() -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Count 24 /Kids [{}] >>",
            (0..24)
                .map(|index| format!("{} 0 R", index * 2 + 3))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    for index in 0..24 {
        let stream = format!(
            "{:.3} 0.35 0.75 rg 0 0 160 8 re f\n",
            (index % 8 + 1) as f32 / 10.
        );
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 8] /Resources << >> /Contents {} 0 R >>", index * 2 + 4));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}endstream",
            stream.len()
        ));
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

fn assert_many_visible_pages_and_hidden_job_ownership(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    editor.update(cx, |view, cx| view.open_tab(SHORT_PDF_PATH.into(), cx));
    wait_for(cx, editor, window, |view, cx| {
        !view.file_loading && view.pdf_probe(cx).is_some()
    });
    editor.update(cx, |view, cx| view.change_pdf_for_e2e(0, 0.25, false, cx));
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let hidden = cx.read(|cx| editor.read(cx).pdf_viewer_for_e2e().unwrap());
    let before = cx.read(|cx| hidden.read(cx).pdf_probe());
    assert!(before.visible_pages.len() > 6 && !before.settled);
    // NOTE is the retained dirty tab, so this genuinely caches the short PDF
    // rather than replacing/dropping its entity.
    editor.update(cx, |view, cx| view.activate_tab(NOTE.into(), cx));
    editor.update(cx, |view, cx| view.open_tab(PDF_PATH.into(), cx));
    let foreground = wait_pdf(cx, editor, window);
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: foreground.scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    assert_eq!(wait_pdf(cx, editor, window).page, 2);
    let stopped = cx.read(|cx| hidden.read(cx).pdf_probe());
    assert!(!stopped.in_flight);
    assert_eq!(
        stopped.raster_requests_started, before.raster_requests_started,
        "hidden cached PDF must finish at most its existing work, not acquire more jobs"
    );
    assert!(stopped.cached_pages.len() <= before.cached_pages.len() + 1);
    editor.update(cx, |view, cx| view.activate_tab(SHORT_PDF_PATH.into(), cx));
    let visible = wait_pdf(cx, editor, window);
    assert!(visible.visible_pages.len() > 6);
    assert!(
        visible
            .visible_pages
            .clone()
            .all(|page| visible.rendered_pages.contains(&page)),
        "every simultaneously visible short page must get real raster output"
    );
    assert!(
        visible.cached_pages.len() <= visible.cache_entry_limit
            && visible.cached_bytes <= 64_000_000
    );
    assert!(visible.raster_jobs <= 2);
    editor.update(cx, |view, cx| view.activate_tab(PDF_PATH.into(), cx));
    let restored = wait_pdf(cx, editor, window);
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: restored.scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    assert_eq!(wait_pdf(cx, editor, window).page, 0);
}

fn assert_continuous_scrolling(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    first: &PdfProbe,
) {
    for (delta, expected_page) in [(-900., 1), (-100_000., 2), (100_000., 0)] {
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: first.scroll.bounds().center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        let current = wait_pdf(cx, editor, window);
        assert_eq!(
            current.page, expected_page,
            "ordinary wheel must reach first/middle/last pages"
        );
        assert_eq!(
            (current.source_id, current.request, current.zoom),
            (first.source_id, first.request, first.zoom)
        );
        assert!(current.mounted_pages.contains(&expected_page));
        assert!(current.cached_pages.contains(&expected_page) && current.image.is_some());
        assert!(
            current.cached_pages.len() <= current.cache_entry_limit
                && current.cached_bytes <= 64_000_000
        );
        assert!(current.raster_jobs <= 2);
        if expected_page != 0 {
            assert_ne!(
                current.image.as_ref().unwrap().as_bytes(0),
                first.image.as_ref().unwrap().as_bytes(0)
            );
        }
    }
    // A short last page may already fit above the bottom edge. Toolbar targets
    // must remain reachable even when two pages share a clamped scroll offset.
    editor.update(cx, |view, cx| view.change_pdf_for_e2e(0, 0.25, false, cx));
    wait_pdf(cx, editor, window);
    native_ax::activate(cx);
    paint(cx, window);
    for (action, page) in [
        (Msg::EditorPdfNext, 1),
        (Msg::EditorPdfNext, 2),
        (Msg::EditorPdfPrevious, 1),
    ] {
        assert!(native_ax::press(cx, window, action.t()));
        assert_eq!(wait_pdf(cx, editor, window).page, page);
    }
    assert!(native_ax::press(cx, window, Msg::EditorPdfResetZoom.t()));
    let restored = wait_pdf(cx, editor, window);
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: restored.scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    assert_eq!((wait_pdf(cx, editor, window).page, restored.zoom), (0, 1.));
}

fn assert_zoom_reading_context(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    editor.update(cx, |view, cx| view.change_pdf_for_e2e(0, 2., false, cx));
    let initial = wait_pdf(cx, editor, window);
    let maximum = initial.scroll.max_offset();
    initial
        .scroll
        .set_offset(gpui::point(-maximum.x / 2., -maximum.y / 2.));
    paint(cx, window);
    for action in [
        Msg::EditorPdfZoomIn,
        Msg::EditorPdfZoomOut,
        Msg::EditorPdfResetZoom,
    ] {
        let before = cx.read(|cx| editor.read(cx).pdf_probe(cx).unwrap());
        let page_origin = before.scroll.bounds_for_item(0).unwrap().origin;
        let center = before.scroll.bounds().center() - page_origin - before.scroll.offset();
        let anchor = before
            .page_tops
            .partition_point(|top| *top <= f32::from(center.y))
            .saturating_sub(1);
        let within = (f32::from(center.y) - before.page_tops[anchor]) / before.zoom;
        let x = center.x / before.zoom;
        assert!(native_ax::press(cx, window, action.t()));
        let after = wait_pdf(cx, editor, window);
        let page_origin = after.scroll.bounds_for_item(0).unwrap().origin;
        let document_point = gpui::point(
            x * after.zoom,
            gpui::px(after.page_tops[anchor] + within * after.zoom),
        );
        let desired = after.scroll.bounds().center() - page_origin - document_point;
        let maximum = after.scroll.max_offset();
        let expected = gpui::point(
            desired.x.clamp(-maximum.x, gpui::px(0.)),
            desired.y.clamp(-maximum.y, gpui::px(0.)),
        );
        let actual = after.scroll.offset();
        assert!(
            (actual.x - expected.x).abs() < gpui::px(1.)
                && (actual.y - expected.y).abs() < gpui::px(1.),
            "same-page {} lost the viewport's document point: {actual:?}, expected {expected:?}",
            action.t()
        );
    }
}

fn mount_compact(
    cx: &mut VisualTestAppContext,
    repo: &Path,
) -> (Entity<kagi::ui::KagiApp>, AnyWindowHandle) {
    let state = kagi::ui::e2e::app_state(repo).unwrap();
    let captured = Rc::new(std::cell::RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        gpui::size(gpui::px(940.), gpui::px(660.)),
        move |window, cx| kagi::ui::e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().unwrap();
    (app, *window)
}

pub fn scenario_editor_pdf_readonly(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    oversized_pdf_keeps_readonly_shortcuts(cx);
    for cached in [false, true] {
        renamed_dirty_text_keeps_keyboard_save(cx, cached);
    }
    dirty_watcher_read_keeps_keyboard_save(cx);
    for (lang, zoom) in [(Lang::En, 1.), (Lang::Ja, 1.), (Lang::Ja, 1.67)] {
        kagi_ui_core::theme::set_zoom(zoom);
        i18n::set_lang(lang);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::write(repo.join(PDF_PATH), PDF).unwrap();
        std::fs::write(repo.join(SHORT_PDF_PATH), short_pages_pdf()).unwrap();
        std::fs::write(repo.join("signature-only"), PDF).unwrap();
        std::fs::write(repo.join(NOTE), "original text\n").unwrap();
        std::fs::write(repo.join("c-broken.PDF"), b"valid UTF-8 but invalid PDF").unwrap();
        std::fs::write(
            repo.join("d-password.pdf"),
            include_bytes!("../fixtures/pdf/password.pdf"),
        )
        .unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "PDF viewer fixture"]);
        let head = rev(&repo, "HEAD");
        let staged = git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]);
        let (app, window) = if zoom > 1. {
            let mounted = mount_compact(cx, &repo);
            // Close the initially open bottom panel to inspect the PDF viewport.
            keys(cx, mounted.1, "cmd-j");
            mounted
        } else {
            mount(cx, &repo)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            app.update(cx, |app, cx| {
                app.open_editor_workspace(cx);
                app.record_history(
                    OperationKind::Commit,
                    "main",
                    CommitId(rev(&repo, "HEAD~1")),
                    CommitId(head.clone()),
                    "PDF must not undo this Git operation",
                );
                app.ui_mut().unwrap().history_seed_attempted = true;
            });
            let editor = cx
                .read(|cx| app.read(cx).ui().editor_workspace.clone())
                .unwrap();
            editor.update(cx, |view, cx| view.set_source(TreeSource::All, cx));
            wait_for(cx, &editor, window, |view, _| {
                !view.loading
                    && view
                        .files
                        .iter()
                        .any(|file| file.path == Path::new(PDF_PATH))
            });
            click_file(cx, &editor, window, NOTE);
            wait_for(cx, &editor, window, |view, _| {
                !view.file_loading && view.editor.is_some()
            });
            let input = cx.read(|cx| editor.read(cx).editor.clone()).unwrap();
            cx.update_window(window, |_, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx)
            })
            .unwrap();
            keys(cx, window, "cmd-a");
            cx.simulate_keystrokes(window, "unsaved");
            keys(cx, window, "space");
            cx.simulate_keystrokes(window, "text");
            paint(cx, window);
            cx.read(|cx| assert!(editor.read(cx).dirty));
            // Root/tree focus is deliberately outside the PDF canvas. Do not
            // focus PDF in this test: opening it must protect the very next key.
            cx.update_window(window, |_, window, cx| {
                editor.read(cx).root_focus.clone().focus(window, cx)
            })
            .unwrap();
            let saves = Rc::new(Cell::new(0));
            let diffs = Rc::new(Cell::new(0));
            let (save_count, diff_count) = (saves.clone(), diffs.clone());
            let subscription = cx.update(|cx| {
                cx.subscribe(&editor, move |_, event, _| match event {
                    EditorWorkspaceEvent::SaveRequested(_) => save_count.set(save_count.get() + 1),
                    EditorWorkspaceEvent::DiffRequested { path, .. }
                        if path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) =>
                    {
                        diff_count.set(diff_count.get() + 1)
                    }
                    EditorWorkspaceEvent::HistoryDiffRequested { .. } => {
                        diff_count.set(diff_count.get() + 1)
                    }
                    _ => {}
                })
            });
            // Hold the real signature classification at its publication boundary:
            // extensionless PDFs must be protected before their type is known.
            let (hold, release) = crate::evidence_support::deferred::<()>(cx);
            EditorWorkspaceView::hold_next_file_classification_for_e2e(hold);
            editor.update(cx, |view, cx| view.open_tab("signature-only".into(), cx));
            paint(cx, window);
            cx.read(|cx| {
                let view = editor.read(cx);
                assert!(view.file_loading);
                assert!(view.editor.is_none());
                assert!(view.pdf_probe(cx).is_none());
            });
            for key in ["cmd-z", "cmd-shift-z", "cmd-s"] {
                keys(cx, window, key);
                paint(cx, window);
                cx.read(|cx| {
                    assert!(
                        app.read(cx).active_modal.is_none(),
                        "unclassified document opened a Git operation: {key}"
                    );
                    assert!(app.read(cx).ui().operation_history.can_undo());
                    assert!(editor.read(cx).file_loading);
                });
                assert_eq!(saves.get(), 0);
            }
            release.send(());
            assert_eq!(wait_pdf(cx, &editor, window).page_count, 3);
            click_file(cx, &editor, window, PDF_PATH);
            for key in ["cmd-z", "cmd-shift-z", "cmd-s"] {
                // Draw the key dispatch tree after the real tree click, without
                // clicking or transferring focus to the PDF canvas.
                paint(cx, window);
                keys(cx, window, key);
                paint(cx, window);
                cx.read(|cx| {
                    assert!(
                        app.read(cx).active_modal.is_none(),
                        "PDF entry accidentally opened Git undo/redo: {key}"
                    );
                    assert!(app.read(cx).ui().operation_history.can_undo());
                    assert!(editor.read(cx).editor.is_none());
                    assert!(!editor.read(cx).dirty);
                });
            }
            let first = wait_pdf(cx, &editor, window);
            assert_eq!((first.page, first.page_count, first.zoom), (0, 3, 1.));
            assert_pixels(first.image.as_ref().unwrap());
            assert_continuous_scrolling(cx, &editor, window, &first);
            if lang == Lang::En {
                assert_many_visible_pages_and_hidden_job_ownership(cx, &editor, window);
            }
            if zoom > 1. {
                let viewport = first.scroll.bounds();
                assert!(
                    viewport.size.height >= gpui::px(200.)
                        && viewport.bottom() <= gpui::px(661.),
                    "large-text PDF controls must leave a usable, contained document viewport: {viewport:?}"
                );
            }
            let backing_scale = cx
                .update_window(window, |_, window, _| window.scale_factor())
                .unwrap();
            let raster = first.image.as_ref().unwrap().size(0);
            assert_eq!(
                (raster.width.0 as f32, raster.height.0 as f32),
                (
                    (612. * backing_scale).floor(),
                    (792. * backing_scale).floor()
                ),
                "PDF raster must use the real display density without changing logical zoom"
            );
            cx.read(|cx| {
                let view = editor.read(cx);
                assert!(
                    view.content.is_none() && view.diff.is_none() && view.history_diff.is_none()
                );
            });
            native_ax::activate(cx);
            paint(cx, window);
            for (id, msg) in [
                ("ews-pdf-previous", Msg::EditorPdfPrevious),
                ("ews-pdf-next", Msg::EditorPdfNext),
                ("ews-pdf-zoom-in", Msg::EditorPdfZoomIn),
                ("ews-pdf-zoom-out", Msg::EditorPdfZoomOut),
                ("ews-pdf-reset", Msg::EditorPdfResetZoom),
            ] {
                let node = native_ax::node(cx, window, id.into());
                assert_eq!(node["aria"]["role"], "Button");
                assert_eq!(node["aria"]["label"], msg.t());
            }
            let _ = native_ax::press(cx, window, Msg::EditorPdfPrevious.t());
            assert_eq!(
                wait_pdf(cx, &editor, window).page,
                0,
                "first-page boundary is inert"
            );
            // Traverse actual Button focus and use Space, not a direct callback.
            let reached = cx
                .update_window(window, |_, window, cx| {
                    for _ in 0..160 {
                        window.focus_next(cx);
                        window.draw(cx).clear();
                        let tree: serde_json::Value =
                            serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
                        if let Some(focus) = tree["gpui_focus"].as_str() {
                            if tree["nodes"][focus]["element_id"]
                                == format!("{:?}", gpui::ElementId::from("ews-pdf-next"))
                            {
                                return true;
                            }
                        }
                    }
                    false
                })
                .unwrap();
            assert!(reached, "native Tab order must reach Next PDF page");
            keys(cx, window, "space");
            let second = wait_pdf(cx, &editor, window);
            assert_eq!(second.page, 1);
            assert_ne!(
                first.image.as_ref().unwrap().as_bytes(0),
                second.image.as_ref().unwrap().as_bytes(0)
            );
            assert!(native_ax::press(cx, window, Msg::EditorPdfNext.t()));
            assert_eq!(wait_pdf(cx, &editor, window).page, 2);
            let _ = native_ax::press(cx, window, Msg::EditorPdfNext.t());
            assert_eq!(
                wait_pdf(cx, &editor, window).page,
                2,
                "last-page boundary is inert"
            );
            assert!(native_ax::press(cx, window, Msg::EditorPdfPrevious.t()));
            assert_eq!(wait_pdf(cx, &editor, window).page, 1);
            let unzoomed_scroll_height = second.scroll.max_offset().y;
            assert_zoom_reading_context(cx, &editor, window);
            assert!(native_ax::press(cx, window, Msg::EditorPdfZoomIn.t()));
            let zoomed = wait_pdf(cx, &editor, window);
            assert_eq!(zoomed.zoom, 1.25);
            assert!(
                zoomed.image.as_ref().unwrap().size(0).height
                    > second.image.as_ref().unwrap().size(0).height
            );
            assert!(
                zoomed.scroll.max_offset().y > unzoomed_scroll_height,
                "zoom must enlarge scrollable page rather than contain-fit it back"
            );
            let image = zoomed.image.clone().unwrap();
            editor.update(cx, |view, cx| view.on_worktree_changed(cx));
            let unchanged = wait_pdf(cx, &editor, window);
            assert!(
                Arc::ptr_eq(&image, unchanged.image.as_ref().unwrap()),
                "unchanged watcher read reuses current raster"
            );
            editor.update(cx, |view, cx| view.activate_tab(NOTE.into(), cx));
            paint(cx, window);
            cx.read(|cx| {
                assert!(editor.read(cx).dirty);
                assert_eq!(editor.read(cx).editor.as_ref(), Some(&input));
                assert_eq!(input.read(cx).value().as_ref(), "unsaved text");
            });
            editor.update(cx, |view, cx| view.activate_tab(PDF_PATH.into(), cx));
            let restored = wait_pdf(cx, &editor, window);
            assert_eq!((restored.page, restored.zoom), (1, 1.25));
            assert!(Arc::ptr_eq(&image, restored.image.as_ref().unwrap()));
            editor.update(cx, |view, cx| {
                let before = view.pdf_probe(cx).unwrap().request;
                view.change_pdf_for_e2e(1, 1., false, cx);
                view.change_pdf_for_e2e(-1, 1., false, cx);
                view.change_pdf_for_e2e(0, 1.25, false, cx);
                view.change_pdf_for_e2e(0, 0.8, false, cx);
                assert!(view.pdf_probe(cx).unwrap().request > before);
            });
            let latest = wait_pdf(cx, &editor, window);
            assert_eq!((latest.page, latest.zoom), (1, 1.25));
            assert_eq!(
                latest.image.unwrap().as_bytes(0),
                image.as_bytes(0),
                "coalesced ABA result must be the newest desired page"
            );
            // Replacement clamps the page and invalidates old-document pixels.
            std::fs::write(repo.join(PDF_PATH), REPLACEMENT).unwrap();
            editor.update(cx, |view, cx| view.on_worktree_changed(cx));
            let replaced = wait_pdf(cx, &editor, window);
            assert_eq!(
                (replaced.page, replaced.page_count, replaced.zoom),
                (0, 1, 1.25)
            );
            assert_ne!(
                replaced.image.as_ref().unwrap().as_bytes(0),
                image.as_bytes(0)
            );
            // Close/reopen while an older request executes cannot resurrect its zoom.
            editor.update(cx, |view, cx| {
                view.change_pdf_for_e2e(0, 1.25, false, cx);
                view.close_tab_now(Path::new(PDF_PATH), cx);
                view.open_tab(PDF_PATH.into(), cx);
            });
            let reopened = wait_pdf(cx, &editor, window);
            assert_eq!((reopened.page, reopened.zoom), (0, 1.));
            assert!(native_ax::press(cx, window, Msg::EditorPdfZoomIn.t()));
            wait_pdf(cx, &editor, window);
            assert!(native_ax::press(cx, window, Msg::EditorPdfResetZoom.t()));
            assert_eq!(wait_pdf(cx, &editor, window).zoom, 1.);
            // Historical PDFs keep the existing binary snapshot placeholder;
            // neither selected-commit nor WIP PDF diffs are admitted.
            cx.update_window(window, |_, window, cx| {
                editor.update(cx, |view, cx| {
                    view.set_right_tab(RightPaneTab::History, window, cx)
                });
            })
            .unwrap();
            wait_for(cx, &editor, window, |view, _| view.history.is_some());
            cx.update_window(window, |_, window, cx| {
                editor.update(cx, |view, cx| {
                    view.set_middle_tab(MiddlePaneTab::Diff, cx);
                    view.select_history_commit(head.clone(), window, cx);
                });
            })
            .unwrap();
            paint(cx, window);
            cx.read(|cx| {
                let view = editor.read(cx);
                assert!(
                    !view.pdf_surface_visible(),
                    "current raster is not historical content"
                );
                assert!(view.history_diff.is_none());
            });
            editor.update(cx, |view, cx| {
                view.set_middle_tab(MiddlePaneTab::Snapshot, cx)
            });
            wait_for(cx, &editor, window, |view, _| view.snapshot.is_some());
            cx.read(|cx| assert!(editor.read(cx).snapshot.as_ref().unwrap().is_binary));
            cx.update_window(window, |_, window, cx| {
                editor.update(cx, |view, cx| {
                    view.set_right_tab(RightPaneTab::Diff, window, cx)
                });
            })
            .unwrap();
            assert_eq!(wait_pdf(cx, &editor, window).zoom, 1.);
            for (path, expected) in [
                ("c-broken.PDF", Msg::EditorPdfInvalid),
                ("d-password.pdf", Msg::EditorPdfPassword),
            ] {
                editor.update(cx, |view, cx| view.open_tab(path.into(), cx));
                let error = wait_pdf(cx, &editor, window);
                assert_eq!(error.error, Some(expected.t()));
                assert!(error.image.is_none());
                editor.update(cx, |view, cx| {
                    view.save(cx);
                    view.save_overwrite(cx);
                });
                cx.read(|cx| assert!(editor.read(cx).editor.is_none() && !editor.read(cx).dirty));
            }
            assert_eq!(
                saves.get(),
                0,
                "PDF controls/save never request write admission"
            );
            assert_eq!(diffs.get(), 0, "recognized PDFs never request Git diffs");
            assert_eq!(std::fs::read(repo.join(PDF_PATH)).unwrap(), REPLACEMENT);
            assert_eq!(
                std::fs::read_to_string(repo.join(NOTE)).unwrap(),
                "original text\n"
            );
            assert_eq!(
                git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]),
                staged,
                "PDF viewing preserves staged paths, object IDs and modes; stat-cache refresh is allowed"
            );
            assert_eq!(rev(&repo, "HEAD"), head);
            drop(subscription);
            drop(input);
            drop(editor);
        }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

fn renamed_dirty_text_keeps_keyboard_save(cx: &mut VisualTestAppContext, cached: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join(NOTE), "original text\n").unwrap();
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.open_editor_workspace(cx));
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .unwrap();
        editor.update(cx, |view, cx| {
            view.set_source(TreeSource::All, cx);
            view.open_tab(NOTE.into(), cx);
        });
        wait_for(cx, &editor, window, |view, _| {
            !view.file_loading && view.editor.is_some()
        });
        let input = cx.read(|cx| editor.read(cx).editor.clone()).unwrap();
        cx.update_window(window, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx);
        })
        .unwrap();
        keys(cx, window, "cmd-a");
        cx.simulate_keystrokes(window, "retained");
        keys(cx, window, "space");
        cx.simulate_keystrokes(window, "draft");
        paint(cx, window);
        cx.read(|cx| assert!(editor.read(cx).dirty));
        if cached {
            editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
            wait_for(cx, &editor, window, |view, _| !view.file_loading);
        }
        app.update(cx, |app, cx| {
            app.open_editor_fs_prompt(
                kagi::ui::EditorFsPromptKind::Rename,
                NOTE.into(),
                "renamed.pdf".into(),
                cx,
            );
            app.confirm_editor_fs_prompt(cx);
        });
        assert!(!repo.join(NOTE).exists());
        assert_eq!(
            std::fs::read_to_string(repo.join("renamed.pdf")).unwrap(),
            "original text\n",
            "rename does not write the unsaved draft"
        );
        if cached {
            editor.update(cx, |view, cx| view.open_tab("renamed.pdf".into(), cx));
        }
        paint(cx, window);
        cx.update_window(window, |_, window, cx| {
            input.read(cx).focus_handle(cx).focus(window, cx);
        })
        .unwrap();
        keys(cx, window, "cmd-s");
        wait_for(cx, &editor, window, |view, _| !view.dirty);
        assert_eq!(
            std::fs::read_to_string(repo.join("renamed.pdf")).unwrap(),
            "retained draft",
            "a retained text buffer must keep keyboard Save after rename (cached={cached})"
        );
        editor.update(cx, |view, cx| view.on_worktree_changed(cx));
        wait_pdf(cx, &editor, window);
        keys(cx, window, "cmd-shift-g");
        cx.read(|cx| {
            assert!(
                app.read(cx).ui().editor_workspace.is_none(),
                "reclassification must not strand keyboard focus on the removed Input"
            );
        });
        drop(input);
        drop(editor);
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn dirty_watcher_read_keeps_keyboard_save(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join(NOTE), "original text\n").unwrap();
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.open_editor_workspace(cx));
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .unwrap();
        editor.update(cx, |view, cx| {
            view.set_source(TreeSource::All, cx);
            view.open_tab(NOTE.into(), cx);
        });
        wait_for(cx, &editor, window, |view, _| {
            !view.file_loading && view.editor.is_some()
        });
        // Cover both a dirty result discarded before Save and Save while the
        // older clean-buffer read is still pending. Neither may trap the draft.
        for (save_while_pending, draft) in [(false, "first"), (true, "second")] {
            let (hold, release) = crate::evidence_support::deferred::<()>(cx);
            let mut release = Some(release);
            EditorWorkspaceView::hold_next_file_classification_for_e2e(hold);
            editor.update(cx, |view, cx| view.on_worktree_changed(cx));
            paint(cx, window);
            cx.read(|cx| assert!(editor.read(cx).file_loading));
            let input = cx.read(|cx| editor.read(cx).editor.clone()).unwrap();
            cx.update_window(window, |_, window, cx| {
                input.read(cx).focus_handle(cx).focus(window, cx);
            })
            .unwrap();
            keys(cx, window, "cmd-a");
            cx.simulate_keystrokes(window, draft);
            paint(cx, window);
            cx.read(|cx| assert!(editor.read(cx).dirty));
            if !save_while_pending {
                release.take().unwrap().send(());
                wait_for(cx, &editor, window, |view, _| !view.file_loading);
            }
            keys(cx, window, "cmd-s");
            wait_for(cx, &editor, window, |view, _| !view.dirty);
            assert_eq!(std::fs::read_to_string(repo.join(NOTE)).unwrap(), draft);
            if let Some(release) = release {
                release.send(());
            }
            wait_for(cx, &editor, window, |view, _| !view.file_loading);
            cx.read(|cx| assert_eq!(editor.read(cx).content.as_deref(), Some(draft)));
            drop(input);
        }
        drop(editor);
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn oversized_pdf_keeps_readonly_shortcuts(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let path = repo.join("oversized.pdf");
    std::fs::write(&path, PDF).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(10 * 1024 * 1024 + 1)
        .unwrap();
    let before = git_fixture::git_output(&repo, &["hash-object", "oversized.pdf"]);
    let head = rev(&repo, "HEAD");
    let parent = rev(&repo, "HEAD~1");
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| {
            app.open_editor_workspace(cx);
            for (kind, from, to) in [
                (OperationKind::Commit, parent.clone(), head.clone()),
                (OperationKind::UndoCommit, head.clone(), parent.clone()),
            ] {
                app.record_history(
                    kind,
                    "main",
                    CommitId(from),
                    CommitId(to),
                    "Oversized PDF must not invoke Git history",
                );
            }
            let ui = app.ui_mut().unwrap();
            ui.operation_history.undo().unwrap();
            ui.history_seed_attempted = true;
        });
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .unwrap();
        let saves = Rc::new(Cell::new(0));
        let save_count = saves.clone();
        let subscription = cx.update(|cx| {
            cx.subscribe(&editor, move |_, event, _| {
                if matches!(event, EditorWorkspaceEvent::SaveRequested(_)) {
                    save_count.set(save_count.get() + 1);
                }
            })
        });
        editor.update(cx, |view, cx| view.open_tab("oversized.pdf".into(), cx));
        wait_for(cx, &editor, window, |view, _| {
            !view.file_loading && view.content_too_large
        });
        cx.update_window(window, |_, window, cx| {
            editor.read(cx).root_focus.clone().focus(window, cx);
        })
        .unwrap();
        for key in ["cmd-z", "cmd-shift-z", "cmd-s"] {
            cx.read(|cx| {
                let history = &app.read(cx).ui().operation_history;
                assert!(history.can_undo() && history.can_redo());
                assert!(editor.read(cx).editor.is_none());
                assert!(editor.read(cx).pdf_probe(cx).is_none());
            });
            keys(cx, window, key);
            paint(cx, window);
            cx.read(|cx| {
                assert!(
                    app.read(cx).active_modal.is_none(),
                    "oversized read-only PDF opened a Git operation: {key}"
                );
                assert_eq!(app.read(cx).ui().operation_history.cursor(), 1);
            });
            assert_eq!(saves.get(), 0);
        }
        assert_eq!(rev(&repo, "HEAD"), head);
        assert_eq!(
            git_fixture::git_output(&repo, &["hash-object", "oversized.pdf"]),
            before
        );
        drop(subscription);
    }));
    unmount(cx, app, window);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
