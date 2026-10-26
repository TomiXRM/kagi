//! Consumer-level Fit Width coverage: real PDF geometry, pixels and shared Buttons.
use crate::diff_highlight::{commit_image_index, settle};
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::native_ax;
use crate::pr_viewed::paint;
use gpui::{px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, editor_workspace::EditorWorkspaceView, KagiApp};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use kagi_ui_editor::{PdfProbe, PdfViewer, TreeSource};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

const FILE: &str = "a-fit.pdf";
const FIT: &str = "ews-pdf-fit-width";

fn geometry(widest: usize) -> [(usize, usize); 3] {
    [(1200, 12000), (widest, widest * 9 / 10), (1600, 14000)]
}

// Same minimal font-free PDF/xref pattern as commit_pdf, with deliberately mixed
// portrait/landscape pages. No fitting calculations are used to create the input.
fn document(widest: usize, blue: bool) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Count 3 /Kids [3 0 R 5 0 R 7 0 R] >>".to_owned(),
    ];
    for (page, (width, height)) in geometry(widest).into_iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << >> /Contents {} 0 R >>", 4 + page * 2));
        let color = if blue { "0 0 1" } else { "1 0 0" };
        let stream = format!("{color} rg 0 0 {width} {height} re f\n");
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

fn wait_pdf(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
) -> PdfProbe {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.advance_clock(Duration::from_millis(16));
        // Exercise ordinary dirty-view drawing. A forced window.refresh()
        // masks ancestor layout invalidation when only PdfViewer changes.
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        let pdf = cx.read(|cx| viewer.read(cx).pdf_probe());
        if pdf.settled && (pdf.image.is_some() || pdf.error.is_some()) {
            assert!(
                pdf.error.is_none(),
                "real PDF raster failed: {:?}",
                pdf.error
            );
            assert!(pdf.image.is_some());
            assert!(pdf
                .visible_pages
                .clone()
                .all(|page| pdf.rendered_pages.contains(&page)));
            assert!(pdf.cached_pages.len() <= pdf.cache_entry_limit);
            assert!(pdf.cached_bytes <= 64_000_000 && pdf.raster_jobs <= 2);
            return pdf;
        }
        assert!(Instant::now() < deadline, "Fit Width PDF did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_document(pdf: &PdfProbe, widest: usize, blue: bool) {
    assert_eq!(pdf.page_count, 3);
    assert_eq!(
        pdf.page_sizes,
        geometry(widest).map(|(w, h)| (w as f32, h as f32))
    );
    let image = pdf
        .image
        .as_ref()
        .expect("actual raster, not just accepted geometry");
    let raster = image.size(0);
    let (width, height) = (raster.width.0 as usize, raster.height.0 as usize);
    assert!(width > 1 && height > 1);
    let (page_width, page_height) = geometry(widest)[pdf.page];
    assert!(
        (width as f32 - height as f32 * page_width as f32 / page_height as f32).abs() <= 2.,
        "raster aspect ratio must match this real page's independent MediaBox"
    );
    let center = ((height / 2) * width + width / 2) * 4;
    let expected = if blue {
        [255, 0, 0, 255]
    } else {
        [0, 0, 255, 255]
    };
    assert_eq!(
        &image.as_bytes(0).unwrap()[center..center + 4],
        &expected,
        "historical/live page color must reach the actual BGRA raster"
    );
}

fn assert_fit(pdf: &PdfProbe, widest: usize, blue: bool) {
    assert_document(pdf, widest, blue);
    assert!(
        pdf.zoom.is_finite() && pdf.zoom > 0. && pdf.zoom < 0.25,
        "fixture must really fit below the manual 25% floor: {}",
        pdf.zoom
    );
    let viewport = pdf.scroll.bounds();
    let content = pdf
        .scroll
        .bounds_for_item(0)
        .expect("measured document child");
    // Measure the actual body inset, not a copied .p_4/rem/backing-scale formula.
    let padding = content.left() - viewport.left();
    assert!(padding > px(0.) && padding < viewport.size.width / 3.);
    let available = viewport.size.width - padding * 2.;
    assert!(
        (content.size.width - available).abs() <= px(1.5),
        "widest page must fill the currently padded logical viewport: {content:?}, {viewport:?}"
    );
    assert!(
        (f32::from(content.size.width) - widest as f32 * pdf.zoom).abs() <= 1.5,
        "actual document layout must use the fitted scale and widest landscape page"
    );
    assert!(
        pdf.scroll.max_offset().x <= px(1.5),
        "Fit must remove horizontal clipping"
    );
}

fn tab_to_fit(cx: &mut VisualTestAppContext, window: AnyWindowHandle, pdf: &PdfProbe) {
    // Start from this side's actual body, so duplicate toolbar ids on historical
    // old/new viewers cannot select the other side. Traverse, never focus a proxy.
    cx.simulate_click(window, pdf.scroll.bounds().center(), Modifiers::none());
    paint(cx, window);
    let identity = format!("{:?}", gpui::ElementId::from(FIT));
    for _ in 0..16 {
        keys(cx, window, "shift-tab");
        paint(cx, window);
        let tree = native_ax::tree(cx, window);
        if let Some(id) = tree["gpui_focus"].as_str() {
            let node = &tree["nodes"][id];
            if node["element_id"] == identity {
                assert_eq!(node["aria"]["role"], "Button");
                assert_eq!(node["aria"]["label"], Msg::EditorPdfFitWidth.t());
                // Also exercise ordinary forward Tab into the same real stop.
                keys(cx, window, "shift-tab");
                keys(cx, window, "tab");
                paint(cx, window);
                let focused = native_ax::focused(cx, window);
                assert_eq!(focused["element_id"], identity);
                return;
            }
        }
    }
    panic!("real Shift-Tab must reach this PDF's Fit Width Button");
}

fn fit_key(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
    key: &str,
) -> PdfProbe {
    let before = wait_pdf(cx, viewer, window);
    tab_to_fit(cx, window, &before);
    keys(cx, window, key);
    let fitted = wait_pdf(cx, viewer, window);
    assert_eq!(
        fitted.source_id, before.source_id,
        "Fit must not reparse/reload the source"
    );
    fitted
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
            paint(cx, window);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native Fit fixture resize did not settle"
        );
    }
}

fn compact_mount(cx: &mut VisualTestAppContext, repo: &Path) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = crate::macos::open_offscreen(cx, size(px(940.), px(660.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    });
    crate::gui_evidence::fixture(repo);
    let app = captured.borrow().clone().unwrap();
    (app, *window)
}

fn open_editor(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Entity<EditorWorkspaceView>, Entity<PdfViewer>) {
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.set_source(TreeSource::All, cx));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        paint(cx, window);
        if cx.read(|cx| !editor.read(cx).loading) {
            break;
        }
        assert!(Instant::now() < deadline, "Editor tree did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    editor.update(cx, |view, cx| view.open_tab(FILE.into(), cx));
    loop {
        paint(cx, window);
        if let Some(viewer) = cx.read(|cx| editor.read(cx).pdf_viewer_for_e2e()) {
            wait_pdf(cx, &viewer, window);
            return (editor, viewer);
        }
        assert!(Instant::now() < deadline, "Editor PDF source did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn editor_manual_controls(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
    fitted: &PdfProbe,
) {
    for (action, page) in [(Msg::EditorPdfNext, 1), (Msg::EditorPdfPrevious, 0)] {
        assert!(native_ax::press(cx, window, action.t()));
        let now = wait_pdf(cx, viewer, window);
        assert_eq!(
            (now.page, now.zoom, now.source_id),
            (page, fitted.zoom, fitted.source_id),
            "page navigation must not clamp a sub25% fit back to manual MIN_ZOOM"
        );
        assert_fit(&now, 6000, false);
    }
    let _ = native_ax::press(cx, window, Msg::EditorPdfZoomOut.t());
    assert_eq!(
        wait_pdf(cx, viewer, window).zoom,
        fitted.zoom,
        "Zoom Out below the manual floor must remain unavailable"
    );
    let before = wait_pdf(cx, viewer, window);
    let center_y = (before.scroll.bounds().center().y
        - before.scroll.bounds_for_item(0).unwrap().top()
        - before.scroll.offset().y)
        / before.zoom;
    assert!(native_ax::press(cx, window, Msg::EditorPdfZoomIn.t()));
    let increased = wait_pdf(cx, viewer, window);
    assert!(
        increased.zoom > fitted.zoom && increased.zoom < 1.,
        "Zoom In must increase a fitted scale without resetting to 100%"
    );
    assert_eq!(increased.source_id, fitted.source_id);
    assert_eq!(increased.page, 0);
    let after_center_y = (increased.scroll.bounds().center().y
        - increased.scroll.bounds_for_item(0).unwrap().top()
        - increased.scroll.offset().y)
        / increased.zoom;
    assert!(
        ((after_center_y - center_y) * increased.zoom).abs() <= px(2.),
        "manual Zoom In must preserve the visible page-relative reading point"
    );
    assert!(native_ax::press(cx, window, Msg::EditorPdfResetZoom.t()));
    let reset = wait_pdf(cx, viewer, window);
    assert_eq!((reset.zoom, reset.source_id), (1., fitted.source_id));
    assert!(
        reset.scroll.max_offset().x > px(100.),
        "100% must restore genuine overflow"
    );
    let viewport = reset.scroll.bounds();
    let page_left = reset.scroll.bounds_for_item(0).unwrap().left() + reset.scroll.offset().x;
    let page_right = page_left + px(reset.page_sizes[reset.page].0 * reset.zoom);
    assert!(
        page_right > viewport.left() + px(1.) && page_left < viewport.right() - px(1.),
        "100% reset must keep the current narrower page on screen, not anchor blank canvas"
    );
}

fn editor_width_changes(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
    compact: bool,
) {
    let fitted = fit_key(cx, viewer, window, "enter");
    assert_fit(&fitted, 6000, false);
    let toggle = e2e::control_bounds(window.window_id(), "editor-details-toggle").unwrap();
    let body_right = cx
        .update_window(window, |_, window, cx| {
            editor.read(cx).pane_bounds(window).right()
        })
        .unwrap();
    assert!(
        (body_right - toggle.right()).abs() < theme::scaled_px(24.),
        "fitting displaced Details: body right {body_right:?}, control {toggle:?}"
    );
    let fitted_width = fitted.scroll.bounds().size.width;
    let (width, height) = if compact { (1440., 900.) } else { (940., 660.) };
    resize(cx, window, width, height);
    let resized = wait_pdf(cx, viewer, window);
    assert_eq!(
        (resized.zoom, resized.source_id),
        (fitted.zoom, fitted.source_id),
        "window resizing is not persistent auto-fit"
    );
    assert!((resized.scroll.bounds().size.width - fitted_width).abs() > px(10.));
    let reapplied = fit_key(cx, viewer, window, "space");
    assert_fit(&reapplied, 6000, false);
    assert!((reapplied.zoom - fitted.zoom).abs() > 0.001);
    let reapplied_width = reapplied.scroll.bounds().size.width;
    let bounds = e2e::control_bounds(window.window_id(), "editor-details-toggle").unwrap();
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        paint(cx, window);
        if cx.read(|cx| !editor.read(cx).pane_widths_animating()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "details disclosure did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let hidden = wait_pdf(cx, viewer, window);
    assert!(!cx.read(|cx| editor.read(cx).details_visible()));
    assert_eq!(
        (hidden.zoom, hidden.source_id),
        (reapplied.zoom, reapplied.source_id),
        "pane disclosure must preserve numeric zoom"
    );
    assert!(hidden.scroll.bounds().size.width > reapplied_width + px(10.));
    let refitted = fit_key(cx, viewer, window, "enter");
    assert_fit(&refitted, 6000, false);
    assert!(refitted.zoom > reapplied.zoom);
    assert!(native_ax::press(cx, window, Msg::EditorDetailsShow.t()));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        paint(cx, window);
        if cx.read(|cx| !editor.read(cx).pane_widths_animating()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "details redisclosure did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let shown = wait_pdf(cx, viewer, window);
    assert!(cx.read(|cx| editor.read(cx).details_visible()));
    assert_eq!(
        (shown.zoom, shown.source_id),
        (refitted.zoom, refitted.source_id),
        "showing details must not implicitly refit either"
    );
    assert!(
        shown.scroll.max_offset().x > px(10.),
        "the now-narrower pane must genuinely need an explicit new Fit"
    );
    assert_fit(&fit_key(cx, viewer, window, "space"), 6000, false);
}

fn replace_editor_source(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
    repo: &Path,
) {
    let before = wait_pdf(cx, viewer, window);
    std::fs::write(repo.join(FILE), document(8000, true)).unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    let deadline = Instant::now() + Duration::from_secs(20);
    let replacement = loop {
        cx.advance_clock(Duration::from_millis(16));
        paint(cx, window);
        let current = cx.read(|cx| editor.read(cx).pdf_probe(cx));
        if let Some(pdf) = current {
            if pdf.source_id != before.source_id && pdf.settled && pdf.image.is_some() {
                break pdf;
            }
        }
        assert!(
            Instant::now() < deadline,
            "watcher replacement did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(
        replacement.zoom, before.zoom,
        "watcher replacement is not an implicit refit"
    );
    assert_document(&replacement, 8000, true);
    assert!(
        replacement.scroll.max_offset().x > px(10.),
        "wider replacement must actually clip until Fit"
    );
    let retained = cx.read(|cx| editor.read(cx).pdf_viewer_for_e2e().unwrap());
    let fitted = fit_key(cx, &retained, window, "space");
    assert_fit(&fitted, 8000, true);
    assert!(fitted.zoom < before.zoom);
    // Restore fixture bytes through the same production watcher before checking
    // its read-only fingerprint. The only content writes here are test setup.
    std::fs::write(repo.join(FILE), document(6000, false)).unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.advance_clock(Duration::from_millis(16));
        paint(cx, window);
        if cx.read(|cx| {
            editor.read(cx).pdf_probe(cx).is_some_and(|pdf| {
                pdf.source_id != fitted.source_id && pdf.settled && pdf.image.is_some()
            })
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "restored fixture did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn scenario_editor_pdf_fit_width(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "sidebar_visible"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    for (lang, zoom) in [(Lang::En, 1.), (Lang::Ja, 1.67)] {
        i18n::set_lang(lang);
        theme::set_zoom(zoom);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::write(repo.join(FILE), document(6000, false)).unwrap();
        git(&repo, &["add", FILE]);
        git(
            &repo,
            &["commit", "-q", "-m", "mixed portrait landscape fit fixture"],
        );
        let before = repo_fingerprint(&repo);
        let staged = git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]);
        let (app, window) = if zoom > 1. {
            compact_mount(cx, &repo)
        } else {
            mount(cx, &repo)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if zoom > 1. {
                keys(cx, window, "cmd-j");
            }
            let (editor, viewer) = open_editor(cx, &app, window);
            native_ax::activate(cx);
            paint(cx, window);
            let initial = wait_pdf(cx, &viewer, window);
            assert_eq!(initial.zoom, 1.);
            assert_document(&initial, 6000, false);
            assert!(initial.scroll.max_offset().x > px(100.));
            let node = native_ax::node(cx, window, FIT.into());
            assert_eq!(node["aria"]["role"], "Button");
            assert_eq!(node["aria"]["label"], Msg::EditorPdfFitWidth.t());
            assert!(native_ax::press(cx, window, Msg::EditorPdfFitWidth.t()));
            let fitted = wait_pdf(cx, &viewer, window);
            assert_fit(&fitted, 6000, false);
            assert_eq!(fitted.source_id, initial.source_id);
            assert!(
                fitted.scroll.bounds().size.height >= px(200.),
                "Fit toolbar must retain useful PDF height"
            );
            editor_manual_controls(cx, &viewer, window, &fitted);
            editor_width_changes(cx, &editor, &viewer, window, zoom > 1.);
            let repeated = fit_key(cx, &viewer, window, "space");
            let repeat_zoom = repeated.zoom;
            keys(cx, window, "enter");
            assert_eq!(
                wait_pdf(cx, &viewer, window).zoom,
                repeat_zoom,
                "the still-focused Fit action must remain available when already fitted"
            );
            cx.simulate_click(window, repeated.scroll.bounds().center(), Modifiers::none());
            for key in ["cmd-s", "cmd-z", "cmd-shift-z"] {
                keys(cx, window, key);
            }
            paint(cx, window);
            assert!(cx.read(|cx| editor.read(cx).editor.is_none() && !editor.read(cx).dirty));
            assert_eq!(
                cx.read(|cx| editor.read(cx).open_path.clone()),
                Some(FILE.into())
            );
            assert_eq!(
                repo_fingerprint(&repo),
                before,
                "Fit/navigation/shortcuts are read-only"
            );
            replace_editor_source(cx, &editor, &viewer, window, &repo);
            assert_eq!(
                std::fs::read(repo.join(FILE)).unwrap(),
                document(6000, false)
            );
            assert_eq!(repo_fingerprint(&repo), before);
            assert_eq!(
                git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]),
                staged
            );
            drop(viewer);
            drop(editor);
        }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

fn historical_sides(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Entity<PdfViewer>, Entity<PdfViewer>) {
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let index = commit_image_index(cx, app, FILE);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    paint(cx, window);
    cx.read(|cx| {
        let pane = app.read(cx).ui().main_diff.as_ref().unwrap().read(cx);
        assert!(pane.view.rows.is_empty() && pane.view.images.is_none());
        (
            pane.pdf_viewer_for_e2e(true, cx).unwrap(),
            pane.pdf_viewer_for_e2e(false, cx).unwrap(),
        )
    })
}

fn reading_state(pdf: &PdfProbe) -> (f32, usize, usize, u64, gpui::Point<gpui::Pixels>) {
    // ScrollHandle is shared/live, so snapshot its offset before the other action.
    (
        pdf.zoom,
        pdf.page,
        pdf.source_id,
        pdf.request,
        pdf.scroll.offset(),
    )
}

pub fn scenario_commit_pdf_fit_width(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "sidebar_visible"]);
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    for (lang, zoom) in [(Lang::En, 1.), (Lang::Ja, 1.67)] {
        i18n::set_lang(lang);
        theme::set_zoom(zoom);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        for (widest, blue) in [(6000, false), (8000, true)] {
            std::fs::write(repo.join(FILE), document(widest, blue)).unwrap();
            git(&repo, &["add", FILE]);
            git(
                &repo,
                &["commit", "-q", "-m", "historical mixed PDF fit fixture"],
            );
        }
        // An unmistakably different live document must never replace either blob.
        std::fs::write(repo.join(FILE), document(10000, false)).unwrap();
        let before = repo_fingerprint(&repo);
        let staged = git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]);
        let (app, window) = if zoom > 1. {
            compact_mount(cx, &repo)
        } else {
            mount(cx, &repo)
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if zoom > 1. {
                keys(cx, window, "cmd-j");
            }
            let (old, new) = historical_sides(cx, &app, window);
            native_ax::activate(cx);
            let old_initial = wait_pdf(cx, &old, window);
            let new_initial = wait_pdf(cx, &new, window);
            assert_document(&old_initial, 6000, false);
            assert_document(&new_initial, 8000, true);
            assert_ne!(old_initial.source_id, new_initial.source_id);
            let new_state = reading_state(&new_initial);
            let old_fit = fit_key(cx, &old, window, "enter");
            assert_fit(&old_fit, 6000, false);
            assert_eq!(
                reading_state(&wait_pdf(cx, &new, window)),
                new_state,
                "old-side Fit must not change the new side"
            );
            let old_state = reading_state(&old_fit);
            let new_fit = fit_key(cx, &new, window, "space");
            assert_fit(&new_fit, 8000, true);
            assert_eq!(
                reading_state(&wait_pdf(cx, &old, window)),
                old_state,
                "new-side Fit must not change the old side"
            );
            assert_ne!(
                old_fit.zoom, new_fit.zoom,
                "different geometries must fit independently"
            );
            let old_width = old_fit.scroll.bounds().size.width;
            let new_width = new_fit.scroll.bounds().size.width;
            // Stay within each layout breakpoint so pane disclosure cannot
            // cancel the window-width change and leave both PDF widths equal.
            resize(
                cx,
                window,
                if zoom > 1. { 820. } else { 1200. },
                if zoom > 1. { 660. } else { 900. },
            );
            let old_resized = wait_pdf(cx, &old, window);
            let new_resized = wait_pdf(cx, &new, window);
            assert!(old_resized.scroll.bounds().size.width < old_width - px(10.));
            assert!(new_resized.scroll.bounds().size.width < new_width - px(10.));
            assert_eq!(old_resized.zoom, old_fit.zoom);
            assert_eq!(new_resized.zoom, new_fit.zoom);
            let new_state = reading_state(&new_resized);
            let old_refit = fit_key(cx, &old, window, "space");
            assert_fit(&old_refit, 6000, false);
            assert_eq!(reading_state(&wait_pdf(cx, &new, window)), new_state);
            let old_state = reading_state(&old_refit);
            let new_refit = fit_key(cx, &new, window, "enter");
            assert_fit(&new_refit, 8000, true);
            assert_eq!(reading_state(&wait_pdf(cx, &old, window)), old_state);
            let new_state = reading_state(&new_refit);
            // Body keyboard page movement after sub25% fitting must preserve both
            // immutable source identities and keep Graph selection/file ownership.
            cx.simulate_click(
                window,
                old_refit.scroll.bounds().center(),
                Modifiers::none(),
            );
            let (body_focused, focused, body_focus) = cx
                .update_window(window, |_, window, cx| {
                    use gpui::Focusable as _;
                    let focus = old.read(cx).focus_handle(cx);
                    (focus.is_focused(window), window.focused(cx), focus)
                })
                .unwrap();
            assert!(
                body_focused,
                "clicking the fitted PDF body must own keyboard focus; bounds={:?}, focused={focused:?}, body={body_focus:?}",
                old_refit.scroll.bounds(),
            );
            keys(cx, window, "end");
            let last = wait_pdf(cx, &old, window);
            assert_eq!(
                (last.page, last.zoom, last.source_id),
                (2, old_refit.zoom, old_initial.source_id)
            );
            assert_document(&last, 6000, false);
            assert_eq!(reading_state(&wait_pdf(cx, &new, window)), new_state);
            assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
            assert_eq!(repo_fingerprint(&repo), before);
            assert_eq!(
                git_fixture::git_output(&repo, &["ls-files", "--stage", "-z"]),
                staged
            );
            assert_eq!(
                std::fs::read(repo.join(FILE)).unwrap(),
                document(10000, false)
            );
            drop(old);
            drop(new);
        }));
        unmount(cx, app, window);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
