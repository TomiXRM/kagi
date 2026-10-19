//! #1682: native pointer drags must reach both ends of the real PDF viewport.
//! Uses only baseline PdfProbe geometry: no scroll setters or new test seams.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::native_ax;
use crate::pr_viewed::{paint, rev};
use crate::surface_scrollbars::{resize, PresentationRestore};
use gpui::{
    point, px, AnyWindowHandle, Entity, Modifiers, MouseButton, Pixels, Point, VisualTestAppContext,
};
use gpui_component::scroll::ScrollbarShow;
use kagi::ui::editor_workspace::EditorWorkspaceView;
use kagi_ui_core::{
    file_tree::TreeRow,
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{PdfProbe, TreeSource};
use std::{path::Path, sync::Arc, time::Duration};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

const PDF_PATH: &str = "a-reading.pdf";
const NEXT: &str = "b-next.txt";
const PDF: &[u8] = include_bytes!("../fixtures/pdf/document.pdf");
const NOTE: &[u8] = b"scrollbar must not select or edit this file\n";

fn wait_for(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    ready: impl Fn(&EditorWorkspaceView) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        paint(cx, window);
        if cx.read(|cx| ready(editor.read(cx))) {
            paint(cx, window);
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PDF scrollbar fixture did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_pdf(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) -> PdfProbe {
    wait_for(cx, editor, window, |view| {
        !view.file_loading && view.pdf_probe().is_some_and(|pdf| pdf.settled)
    });
    cx.read(|cx| editor.read(cx).pdf_probe().unwrap())
}

fn open_pdf(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    editor.update(cx, |view, cx| view.set_source(TreeSource::All, cx));
    wait_for(cx, editor, window, |view| {
        !view.loading
            && view
                .files
                .iter()
                .any(|file| file.path == Path::new(PDF_PATH))
    });
    let bounds = cx.read(|cx| {
        let view = editor.read(cx);
        let file = view
            .files
            .iter()
            .position(|file| file.path == Path::new(PDF_PATH))
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
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    wait_pdf(cx, editor, window);
    // Ordinary toolbar activation prepares useful overflow; it is not the
    // scrolling oracle and never changes a handle offset directly.
    native_ax::activate(cx);
    native_ax::focus_named(cx, window, "Button", Msg::EditorPdfZoomIn.t());
    for _ in 0..4 {
        keys(cx, window, "space");
        wait_pdf(cx, editor, window);
    }
}

fn assert_identity(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    initial: &PdfProbe,
) -> Point<Pixels> {
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.open_path.as_deref(), Some(Path::new(PDF_PATH)));
        assert_eq!(
            view.selected.map(|i| view.files[i].path.as_path()),
            Some(Path::new(PDF_PATH)),
            "scrollbar pointer input must not leak into File Tree selection"
        );
        assert!(matches!(view.source, TreeSource::All));
        assert!(
            view.editor.is_none() && view.content.is_none() && !view.dirty,
            "PDF scrollbar interaction must retain the read-only presentation"
        );
        let current = view.pdf_probe().expect("drag discarded the PDF");
        assert_eq!(
            (
                current.page,
                current.page_count,
                current.zoom,
                current.request
            ),
            (
                initial.page,
                initial.page_count,
                initial.zoom,
                initial.request
            ),
            "scrollbar drag must not change document/page/zoom/raster request"
        );
        assert!(current.settled && !current.in_flight && current.error.is_none());
        assert!(
            Arc::ptr_eq(
                current.image.as_ref().unwrap(),
                initial.image.as_ref().unwrap()
            ),
            "scrollbar drag must preserve the accepted page bitmap"
        );
        let offset = current.scroll.offset();
        let maximum = current.scroll.max_offset();
        assert!(
            offset.x >= -maximum.x
                && offset.x <= px(0.)
                && offset.y >= -maximum.y
                && offset.y <= px(0.),
            "scrollbar escaped actual viewport extent: {offset:?}, max {maximum:?}"
        );
        offset
    })
}

fn assert_close(actual: Pixels, expected: Pixels, why: &str) {
    assert!(
        (actual - expected).abs() < px(1.),
        "{why}: {actual:?}, expected {expected:?}"
    );
}

// Coordinates follow the pinned shared control, not theme-size assertions:
// 48px minimum thumb, 16px bar/corner lane. The centre is inside both idle
// and hovered thumb hitboxes, whose insets cancel around the centre point.
fn thumb_center(pdf: &PdfProbe, vertical: bool) -> Point<Pixels> {
    let bounds = pdf.scroll.bounds();
    let maximum = pdf.scroll.max_offset();
    let offset = pdf.scroll.offset();
    let (length, extent, position, corner) = if vertical {
        (bounds.size.height, maximum.y, offset.y, px(0.))
    } else {
        (bounds.size.width, maximum.x, offset.x, px(16.))
    };
    let thumb = (length / (length + extent) * length).max(px(48.));
    let start = -position / extent * (length - corner - thumb);
    if vertical {
        point(bounds.right() - px(8.), bounds.top() + start + thumb / 2.)
    } else {
        point(bounds.left() + start + thumb / 2., bounds.bottom() - px(8.))
    }
}

fn drag_endpoint(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    initial: &PdfProbe,
    vertical: bool,
    end: bool,
) {
    let axis = if vertical { "vertical" } else { "horizontal" };
    let bounds = initial.scroll.bounds();
    let from = thumb_center(initial, vertical);
    let before = assert_identity(cx, editor, initial);
    cx.simulate_mouse_move(window, from, None, Modifiers::none());
    paint(cx, window);
    cx.simulate_mouse_down(window, from, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        initial.scroll.offset(),
        before,
        "{axis} mouse down must grab the thumb, not jump on the track"
    );
    let to = if vertical {
        point(
            from.x,
            if end {
                bounds.bottom() + px(48.)
            } else {
                bounds.top() - px(48.)
            },
        )
    } else {
        point(
            if end {
                bounds.right() + px(48.)
            } else {
                bounds.left() - px(48.)
            },
            from.y,
        )
    };
    let maximum = initial.scroll.max_offset();
    let expected = if end {
        if vertical {
            -maximum.y
        } else {
            -maximum.x
        }
    } else {
        px(0.)
    };
    // Kit throttles using wall-clock Instant, not the test dispatcher clock.
    // A pause above its 120fps interval makes each real pointer move eligible.
    for n in 1..=5 {
        std::thread::sleep(Duration::from_millis(20));
        cx.simulate_mouse_move(
            window,
            from + (to - from) * (n as f32 / 5.),
            MouseButton::Left,
            Modifiers::none(),
        );
        paint(cx, window);
        let offset = assert_identity(cx, editor, initial);
        assert_eq!(
            if vertical { offset.x } else { offset.y },
            if vertical { before.x } else { before.y },
            "{axis} thumb drag must preserve the orthogonal axis"
        );
    }
    let reached = assert_identity(cx, editor, initial);
    // Expected baseline failure: overflow exists, but there is no scrollbar
    // thumb, so the native drag leaves the actual viewport at its old offset.
    assert_close(
        if vertical { reached.y } else { reached.x },
        expected,
        &format!("{axis} PDF thumb drag must reach the requested endpoint"),
    );
    let beyond = to + (to - from) * 0.25;
    std::thread::sleep(Duration::from_millis(20));
    cx.simulate_mouse_move(window, beyond, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        assert_identity(cx, editor, initial),
        reached,
        "held drag clamps beyond endpoint"
    );
    cx.simulate_mouse_up(window, beyond, MouseButton::Left, Modifiers::none());
    paint(cx, window);
    assert_eq!(
        assert_identity(cx, editor, initial),
        reached,
        "release and redraw retain endpoint"
    );
}

pub fn scenario_editor_pdf_scrollbar_drag(cx: &mut VisualTestAppContext) {
    let _restore = PresentationRestore::capture(cx);
    // Always tests the resting affordance; Hover starts by entering its lane;
    // Scrolling becomes interactive after a real wheel, never an offset setter.
    for (policy, preset, lang, zoom) in [
        (ScrollbarShow::Always, "apple-light", Lang::En, 1.),
        (ScrollbarShow::Hover, "apple-dark", Lang::Ja, 1.67),
        (ScrollbarShow::Scrolling, "apple-light", Lang::En, 1.),
    ] {
        assert!(theme::set_active(preset));
        theme::set_zoom(zoom);
        i18n::set_lang(lang);
        cx.update(theme::sync_gpui_component_theme);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::write(repo.join(PDF_PATH), PDF).unwrap();
        std::fs::write(repo.join(NEXT), NOTE).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "PDF scrollbar fixture"]);
        let head = rev(&repo, "HEAD");
        let staged = git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]);
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        let readme = std::fs::read(repo.join("README.md")).unwrap();
        let (app, window) = mount(cx, &repo);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resize(cx, window);
            cx.update(|cx| gpui_component::Theme::global_mut(cx).scrollbar_show = policy);
            if zoom > 1. {
                keys(cx, window, "cmd-j");
            }
            app.update(cx, |app, cx| app.open_editor_workspace(cx));
            let editor = cx
                .read(|cx| app.read(cx).ui().editor_workspace.clone())
                .unwrap();
            open_pdf(cx, &editor, window);
            let initial = wait_pdf(cx, &editor, window);
            assert_eq!((initial.page, initial.page_count), (0, 3));
            assert!(
                initial.scroll.max_offset().x > px(100.)
                    && initial.scroll.max_offset().y > initial.scroll.bounds().size.height,
                "fixture must overflow both axes by a useful reading distance"
            );
            if policy == ScrollbarShow::Scrolling {
                let before = initial.scroll.offset();
                cx.simulate_event(
                    window,
                    gpui::ScrollWheelEvent {
                        position: initial.scroll.bounds().center(),
                        delta: gpui::ScrollDelta::Pixels(point(px(-16.), px(-32.))),
                        touch_phase: gpui::TouchPhase::Moved,
                        ..Default::default()
                    },
                );
                paint(cx, window);
                let offset = assert_identity(cx, &editor, &initial);
                assert!(
                    offset.y < before.y,
                    "a native wheel must open the auto-hide visibility interval"
                );
            }
            // The second and third drags preserve a nonzero orthogonal offset.
            for (vertical, end) in [(true, true), (false, true), (true, false), (false, false)] {
                drag_endpoint(cx, &editor, window, &initial, vertical, end);
            }
            assert_eq!(rev(&repo, "HEAD"), head, "viewing must preserve HEAD");
            assert_eq!(
                git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]),
                staged
            );
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(std::fs::read(repo.join(PDF_PATH)).unwrap(), PDF);
            assert_eq!(std::fs::read(repo.join(NEXT)).unwrap(), NOTE);
            assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), readme);
            drop(editor);
        }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
