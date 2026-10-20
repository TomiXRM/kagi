//! Real immutable Git blobs and retained shared PDF viewers in the Graph diff.
use crate::diff_highlight::{commit_image_index, main_view, settle};
use crate::evidence_support::deferred;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::native_ax;
use crate::pr_viewed::{paint, rev};
use gpui::{point, px, AnyWindowHandle, Entity, Focusable as _, Modifiers, VisualTestAppContext};
use kagi::ui::{commit_panel::CommitPanelFileRef, e2e, main_diff_pane::MainDiffPane, KagiApp};
use kagi_git::{Backend, CommitId};
use kagi_ui_core::i18n::{Lang, Msg};
use kagi_ui_editor::{PdfProbe, PdfViewer};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const DUAL: &str = "dual.pdf";
const SIGNATURE: &str = "signature.note";
const NOTE: &str = "note.txt";

// Font-free, public, all-ASCII PDF with differing page geometries. A long stable
// preamble keeps the unchanged signature out of the color-only modification hunk.
fn document(pages: usize, width: usize, height: usize, color: &str) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Count {pages} /Kids [{}] >>",
            (0..pages)
                .map(|page| format!("{} 0 R", 3 + page * 2))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    for page in 0..pages {
        let (width, height) = (width + page * 11, height + page * 29);
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << >> /Contents {} 0 R >>", 4 + page * 2));
        let marker = [
            "1 0 0", "0 1 0", "0 0 1", "0 1 1", "1 0 1", "1 1 0", "0 0 0",
        ][page % 7];
        let stream = format!(
            "{color} rg 0 0 {width} {height} re f\n{marker} rg {} {} {} {} re f\n",
            width / 4,
            height / 4,
            width / 2,
            height / 2
        );
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}endstream",
            stream.len()
        ));
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for line in 0..24 {
        bytes.extend_from_slice(format!("% stable preamble line {line:02}\n").as_bytes());
    }
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.into_iter().skip(1) {
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

pub(crate) fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    for (path, bytes) in [
        (DUAL, document(3, 240, 1500, "0.8 0.2 0.2")),
        (SIGNATURE, document(3, 240, 480, "0.8 0.2 0.2")),
        (
            "deleted.pdf",
            include_bytes!("../fixtures/pdf/document.pdf").to_vec(),
        ),
        (
            "rename-before.pdf",
            include_bytes!("../fixtures/pdf/rotated.pdf").to_vec(),
        ),
        ("repaired.pdf", b"not a valid PDF".to_vec()),
        (NOTE, b"old ordinary text\n".to_vec()),
    ] {
        std::fs::write(repo.join(path), bytes).unwrap();
    }
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "PDF previous versions"]);
    for (path, bytes) in [
        (DUAL, document(5, 360, 1800, "0.2 0.2 0.8")),
        (SIGNATURE, document(3, 240, 480, "0.2 0.2 0.8")),
        ("added.pdf", document(2, 300, 600, "0.2 0.8 0.2")),
        ("repaired.pdf", document(1, 320, 500, "0.2 0.8 0.2")),
        ("malformed.pdf", b"%PDF-invalid".to_vec()),
        (
            "password.pdf",
            include_bytes!("../fixtures/pdf/password.pdf").to_vec(),
        ),
        (NOTE, b"new ordinary text\n".to_vec()),
    ] {
        std::fs::write(repo.join(path), bytes).unwrap();
    }
    std::fs::remove_file(repo.join("deleted.pdf")).unwrap();
    git(repo, &["mv", "rename-before.pdf", "rename-after.pdf"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "PDF current versions"]);
    // The historical target must not silently substitute the live file.
    std::fs::write(repo.join(DUAL), document(7, 480, 600, "0.2 0.8 0.2")).unwrap();
    fixture
}

fn pane(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Entity<MainDiffPane> {
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .main_diff
            .clone()
            .expect("accepted main diff")
    })
}
pub(crate) fn viewer(
    cx: &mut VisualTestAppContext,
    pane: &Entity<MainDiffPane>,
    old: bool,
) -> Option<Entity<PdfViewer>> {
    cx.read(|cx| pane.read(cx).pdf_viewer_for_e2e(old, cx))
}
pub(crate) fn probe(cx: &mut VisualTestAppContext, viewer: &Entity<PdfViewer>) -> PdfProbe {
    cx.read(|cx| viewer.read(cx).pdf_probe())
}
pub(crate) fn wait_pdf(
    cx: &mut VisualTestAppContext,
    viewer: &Entity<PdfViewer>,
    window: AnyWindowHandle,
) -> PdfProbe {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.advance_clock(Duration::from_millis(16));
        paint(cx, window);
        let pdf = probe(cx, viewer);
        if pdf.settled && (pdf.error.is_some() || pdf.image.is_some()) {
            assert!(pdf.cached_pages.len() <= pdf.cache_entry_limit);
            assert!(pdf.cached_bytes <= 64_000_000);
            assert!(pdf.raster_jobs <= 2);
            return pdf;
        }
        assert!(
            Instant::now() < deadline,
            "historical PDF viewer did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(crate) fn open(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    path: &str,
) -> Entity<MainDiffPane> {
    let index = commit_image_index(cx, app, path);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    paint(cx, window);
    let pane = pane(cx, app);
    cx.read(|cx| {
        let view = &pane.read(cx).view;
        assert_eq!(view.title.as_ref(), path);
        assert!(
            view.rows.is_empty(),
            "PDF preview must not synthesize text diff rows"
        );
        assert!(view.images.is_none() && view.lang.is_none());
    });
    pane
}
fn assert_color(pdf: &PdfProbe, red: bool) {
    let bytes = pdf.image.as_ref().unwrap().as_bytes(0).unwrap();
    let matching = bytes
        .chunks_exact(4)
        .filter(|pixel| {
            if red {
                pixel[2].saturating_sub(pixel[0]) > 60
            } else {
                pixel[0].saturating_sub(pixel[2]) > 60
            }
        })
        .count();
    assert!(
        matching > 1000,
        "real historical document pixels, not only a parsed source"
    );
}

fn assert_center_pixel(pdf: &PdfProbe, expected_bgra: [u8; 4]) {
    let image = pdf
        .image
        .as_ref()
        .expect("the visible page must contain actual raster output");
    let size = image.size(0);
    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    assert!(width > 0 && height > 0);
    let bytes = image.as_bytes(0).unwrap();
    let center = ((height / 2) * width + width / 2) * 4;
    for (actual, expected) in bytes[center..center + 4].iter().zip(expected_bgra) {
        assert!(actual.abs_diff(expected) <= 3,
            "page-specific center marker must match independently expected BGRA {expected_bgra:?}, actual {:?}",
            &bytes[center..center + 4]);
    }
}
fn assert_sides(
    cx: &mut VisualTestAppContext,
    pane: &Entity<MainDiffPane>,
    window: AnyWindowHandle,
    old: Option<usize>,
    new: Option<usize>,
) {
    for (old_side, expected) in [(true, old), (false, new)] {
        let actual = viewer(cx, pane, old_side);
        assert_eq!(
            actual.is_some(),
            expected.is_some(),
            "old side = {old_side}"
        );
        if let (Some(viewer), Some(count)) = (actual, expected) {
            let pdf = wait_pdf(cx, &viewer, window);
            assert_eq!(pdf.page_count, count);
            assert!(pdf.error.is_none());
        }
    }
}

pub fn scenario_commit_pdf_sources(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let backend = Backend::open(&repo).unwrap();
    let head = CommitId(rev(&repo, "HEAD"));
    let signature = backend
        .commit_file_diff(&head, Path::new(SIGNATURE))
        .unwrap();
    assert!(
        !signature.is_binary,
        "ASCII signature-only PDF exercises text-like Git bytes"
    );
    assert!(
        signature
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .all(|line| !line.content.contains("%PDF-")),
        "changed hunks must omit the PDF signature"
    );
    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        let dual = open(cx, &app, window, DUAL);
        assert_sides(cx, &dual, window, Some(3), Some(5));
        let old_viewer = viewer(cx, &dual, true).unwrap();
        let new_viewer = viewer(cx, &dual, false).unwrap();
        let old = wait_pdf(cx, &old_viewer, window);
        let new = wait_pdf(cx, &new_viewer, window);
        assert_eq!(old.page_sizes[0], (240., 1500.));
        assert_eq!(new.page_sizes[0], (360., 1800.));
        assert_color(&old, true);
        assert_color(&new, false);
        for (id, label) in [
            ("main-diff-pdf-old", Msg::DiffPdfPrevious),
            ("main-diff-pdf-new", Msg::DiffPdfCurrent),
        ] {
            let node = native_ax::node(cx, window, id.into());
            assert_eq!(node["aria"]["role"], "Group");
            assert_eq!(node["aria"]["label"], label.t());
        }
        let added = open(cx, &app, window, "added.pdf");
        assert_sides(cx, &added, window, None, Some(2));
        assert_eq!(
            native_ax::node(cx, window, "main-diff-pdf-old".into())["aria"]["description"],
            Msg::DiffPdfNoPrevious.t()
        );
        let deleted = open(cx, &app, window, "deleted.pdf");
        assert_sides(cx, &deleted, window, Some(3), None);
        assert_eq!(
            native_ax::node(cx, window, "main-diff-pdf-new".into())["aria"]["description"],
            Msg::DiffPdfNoCurrent.t()
        );
        let renamed = open(cx, &app, window, "rename-after.pdf");
        assert_sides(cx, &renamed, window, Some(1), Some(1));
        let old = viewer(cx, &renamed, true).unwrap();
        let new = viewer(cx, &renamed, false).unwrap();
        assert_eq!(
            probe(cx, &old).image.unwrap().as_bytes(0),
            probe(cx, &new).image.unwrap().as_bytes(0)
        );
        let repaired = open(cx, &app, window, "repaired.pdf");
        let old = viewer(cx, &repaired, true).expect("malformed old side remains present");
        assert_eq!(
            wait_pdf(cx, &old, window).error,
            Some(Msg::EditorPdfInvalid.t())
        );
        let new = viewer(cx, &repaired, false).unwrap();
        assert_eq!(wait_pdf(cx, &new, window).page_count, 1);
        for (path, message) in [
            ("malformed.pdf", Msg::EditorPdfInvalid),
            ("password.pdf", Msg::EditorPdfPassword),
        ] {
            let pane = open(cx, &app, window, path);
            assert!(viewer(cx, &pane, true).is_none());
            let new = viewer(cx, &pane, false).expect("parse failure is a visible current version");
            assert_eq!(wait_pdf(cx, &new, window).error, Some(message.t()));
        }
        for _ in 0..2 {
            let signature = open(cx, &app, window, SIGNATURE);
            assert_sides(cx, &signature, window, Some(3), Some(3));
            let old = viewer(cx, &signature, true).unwrap();
            let new = viewer(cx, &signature, false).unwrap();
            assert_color(&wait_pdf(cx, &old, window), true);
            assert_color(&wait_pdf(cx, &new, window), false);
            app.update(cx, |app, _| app.close_main_diff());
        }
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_pdf_sources");
}

pub fn scenario_commit_pdf_staged_rename(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let previous = document(3, 240, 480, "0.2 0.2 0.8");
    let staged = document(4, 240, 480, "0.2 0.2 0.8");
    let working = document(6, 240, 480, "0.2 0.2 0.8");
    std::fs::write(repo.join("old.pdf"), &previous).unwrap();
    git(&repo, &["add", "old.pdf"]);
    git(&repo, &["commit", "-q", "-m", "staged PDF rename base"]);
    git(&repo, &["mv", "old.pdf", "new.pdf"]);
    std::fs::write(repo.join("new.pdf"), &staged).unwrap();
    git(&repo, &["add", "new.pdf"]);
    std::fs::write(repo.join("new.pdf"), &working).unwrap();
    let before = repo_fingerprint(&repo);
    let index_before = rev(&repo, ":new.pdf");
    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    settle(cx, window);
    let index = cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.as_ref().unwrap().read(cx);
        let index = panel
            .state
            .staged
            .iter()
            .position(|file| file.path == Path::new("new.pdf"))
            .expect("staged rename is listed in the real Commit Panel");
        assert_eq!(
            panel.state.staged[index].change,
            kagi_git::ChangeKind::Renamed {
                from: "old.pdf".into()
            },
            "fixture must exercise a detected rename, not an added PDF"
        );
        index
    });
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Staged { index }, cx)
    });
    paint(cx, window);
    let staged_pane = pane(cx, &app);
    // Three distinct documents make either a missing old side or a worktree
    // substitution fail against the actual parsed/rasterized viewer output.
    assert_sides(cx, &staged_pane, window, Some(3), Some(4));
    cx.read(|cx| {
        let view = &staged_pane.read(cx).view;
        assert!(view.rows.is_empty() && view.images.is_none() && view.lang.is_none());
    });
    assert_eq!(repo_fingerprint(&repo), before);
    assert_eq!(rev(&repo, ":new.pdf"), index_before);
    assert!(!repo.join("old.pdf").exists());
    assert_eq!(std::fs::read(repo.join("new.pdf")).unwrap(), working);

    // The same open pane must discard rename identity when a refresh changes
    // the new path into an addition by restoring the original HEAD path.
    std::fs::write(repo.join("old.pdf"), &previous).unwrap();
    git(&repo, &["add", "old.pdf"]);
    let refreshed_before = repo_fingerprint(&repo);
    app.update(cx, |app, cx| app.reload(cx));
    settle(cx, window);
    let refreshed = pane(cx, &app);
    assert_sides(cx, &refreshed, window, None, Some(4));

    let index = cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.as_ref().unwrap().read(cx);
        panel
            .state
            .unstaged
            .iter()
            .position(|file| file.path == Path::new("new.pdf"))
            .expect("worktree edit remains on the unstaged side")
    });
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(CommitPanelFileRef::Unstaged { index }, cx)
    });
    paint(cx, window);
    let unstaged_pane = pane(cx, &app);
    assert_sides(cx, &unstaged_pane, window, Some(4), Some(6));
    assert_eq!(repo_fingerprint(&repo), refreshed_before);
    assert_eq!(rev(&repo, ":new.pdf"), index_before);
    assert_eq!(std::fs::read(repo.join("old.pdf")).unwrap(), previous);
    assert_eq!(std::fs::read(repo.join("new.pdf")).unwrap(), working);
    drop((staged_pane, refreshed, unstaged_pane));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_pdf_staged_rename");
}

pub fn scenario_commit_pdf_owners(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let index = commit_image_index(cx, &app, DUAL);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        app.select(1);
        app.select(0);
    });
    release.send(());
    paint(cx, window);
    assert!(
        main_view(cx, &app).is_none(),
        "selection ABA must not revive a PDF request"
    );
    assert!(!cx.read(|cx| app
        .read(cx)
        .ui()
        .diff_caches
        .file_content
        .contains_key(&(0, index))));

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    let added = open(cx, &app, window, "added.pdf");
    let accepted = viewer(cx, &added, false).unwrap();
    release.send(());
    paint(cx, window);
    let current = pane(cx, &app);
    assert_eq!(
        viewer(cx, &current, false).unwrap().entity_id(),
        accepted.entity_id()
    );
    assert_sides(cx, &current, window, None, Some(2));
    drop(current);
    drop(accepted);

    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        app.select(1);
        cx.notify();
    });
    settle(cx, window);
    let previous_commit = open(cx, &app, window, DUAL);
    assert_sides(cx, &previous_commit, window, None, Some(3));
    let accepted = viewer(cx, &previous_commit, false).unwrap();
    release.send(());
    paint(cx, window);
    let current = pane(cx, &app);
    assert_eq!(
        viewer(cx, &current, false).unwrap().entity_id(),
        accepted.entity_id()
    );
    assert_sides(cx, &current, window, None, Some(3));
    drop(current);

    let other = build_fixture();
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.open_main_diff_commit(index, cx);
        assert!(app.open_repository(other.path().canonicalize().unwrap(), cx));
        app.switch_repo(0, cx);
    });
    settle(cx, window);
    settle(cx, window);
    let revisited = open(cx, &app, window, "added.pdf");
    let revisit_viewer = viewer(cx, &revisited, false).unwrap();
    release.send(());
    paint(cx, window);
    let current = pane(cx, &app);
    assert_eq!(
        viewer(cx, &current, false).unwrap().entity_id(),
        revisit_viewer.entity_id(),
        "an earlier visit cannot replace the returned session's accepted viewer"
    );
    assert_sides(cx, &current, window, None, Some(2));
    drop(current);
    drop(revisit_viewer);
    drop(revisited);

    // Unknown classification from a synchronous/headless cache entry is resolved
    // once. Known ordinary text retains its existing immediate cached opening.
    settle(cx, window);
    let text_index = commit_image_index(cx, &app, NOTE);
    let backend = Backend::open(&repo).unwrap();
    let text = Arc::new(
        backend
            .commit_file_diff(&CommitId(rev(&repo, "HEAD")), Path::new(NOTE))
            .unwrap(),
    );
    app.update(cx, |app, _| {
        app.close_main_diff();
        app.ui_mut()
            .unwrap()
            .diff_caches
            .file_content
            .insert((0, text_index), text.into());
    });
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| app.open_main_diff_commit(text_index, cx));
    assert!(
        main_view(cx, &app).is_none(),
        "unknown classification must inspect original bytes on worker"
    );
    release.send(());
    paint(cx, window);
    assert_eq!(main_view(cx, &app).unwrap().title.as_ref(), NOTE);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    app.update(cx, |app, cx| {
        app.close_main_diff();
        app.open_main_diff_commit(text_index, cx);
        assert!(
            app.ui().main_diff.is_some(),
            "known text must still publish synchronously"
        );
        app.open_main_diff_commit(index, cx);
    });
    paint(cx, window);
    assert_eq!(
        main_view(cx, &app).unwrap().title.as_ref(),
        NOTE,
        "cached text must not consume the next worker read"
    );
    release.send(());
    paint(cx, window);
    let current = pane(cx, &app);
    assert_sides(cx, &current, window, Some(3), Some(5));
    drop(current);
    drop(accepted);
    drop(previous_commit);
    drop(added);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_pdf_owners");
}

pub(crate) fn wheel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    pdf: &PdfProbe,
    delta: f32,
) {
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: pdf.scroll.bounds().center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
}

pub fn scenario_commit_pdf_continuous_sides(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let pane = open(cx, &app, window, DUAL);
    let old = viewer(cx, &pane, true).unwrap();
    let new = viewer(cx, &pane, false).unwrap();
    for (active, other, count, last_marker) in [
        (&old, &new, 3, [255, 0, 0, 255]), // Page 3 has a blue center marker.
        (&new, &old, 5, [255, 0, 255, 255]), // Page 5 has a magenta center marker.
    ] {
        let initial = wait_pdf(cx, active, window);
        assert_center_pixel(&initial, [0, 0, 255, 255]); // First page marker is red.
        let other_offset = wait_pdf(cx, other, window).scroll.offset();
        let mut seen = vec![false; count];
        let step = f32::from(initial.scroll.bounds().size.height) * 0.7;
        assert!(
            step > 0.,
            "each side must have a bounded nonempty reading viewport"
        );
        let steps = (f32::from(initial.scroll.max_offset().y) / step).ceil() as usize + count + 2;
        for _ in 0..steps {
            let pdf = wait_pdf(cx, active, window);
            assert_eq!(pdf.source_id, initial.source_id);
            assert_eq!(pdf.zoom, initial.zoom);
            for page in pdf.visible_pages.clone() {
                seen[page] = true;
            }
            assert_eq!(
                probe(cx, other).scroll.offset(),
                other_offset,
                "PDF sides scroll independently"
            );
            if pdf.visible_pages.contains(&(count - 1)) {
                break;
            }
            wheel(cx, window, &pdf, -step);
        }
        let last = wait_pdf(cx, active, window);
        assert!(
            seen.into_iter().all(|seen| seen),
            "ordinary wheel scrolling must reach every page"
        );
        assert!(last.visible_pages.contains(&(count - 1)));
        assert!(
            last.cached_pages.contains(&(count - 1)),
            "last page is retained in the viewport cache"
        );
        assert!(last.scroll.offset().y < px(0.));
        // These real 3/5-page documents fit the 64MB budget at final density.
        // Leaving the mount window must not evict useful reading history.
        assert!(
            last.cached_pages.contains(&0),
            "warm history survives unmounting"
        );
        if count > 3 {
            assert!(
                !last.mounted_pages.contains(&0),
                "the cache oracle must include an unmounted page"
            );
        }
        let warm_requests = last.raster_requests_started;
        cx.simulate_click(window, last.scroll.bounds().center(), Modifiers::none());
        assert!(
            cx.update_window(window, |_, window, cx| {
                active.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap(),
            "click must focus this side's actual shared PDF body"
        );
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("PDF copy sentinel".into()));
        keys(cx, window, "cmd-c");
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("PDF copy sentinel"),
            "PDF focus must not copy a covered Graph commit"
        );
        keys(cx, window, "home");
        let first = wait_pdf(cx, active, window);
        assert_eq!(first.scroll.offset().y, px(0.));
        assert_eq!(
            first.raster_requests_started, warm_requests,
            "warm backtracking must not admit another raster"
        );
        assert!(
            Arc::ptr_eq(
                initial.image.as_ref().unwrap(),
                first.image.as_ref().unwrap(),
            ),
            "warm backtracking must reuse the original final-density image"
        );
        keys(cx, window, "end");
        let last = wait_pdf(cx, active, window);
        assert_eq!(last.page, count - 1, "End must show the actual final page");
        assert_eq!(
            last.raster_requests_started, warm_requests,
            "returning to the settled last page must reuse its raster"
        );
        assert!(
            last.error.is_none(),
            "a cache entry containing an error is not a rendered page"
        );
        assert_center_pixel(&last, last_marker);
        assert_ne!(
            initial.image.as_ref().unwrap().as_bytes(0),
            last.image.as_ref().unwrap().as_bytes(0),
            "the final page must not repaint the first page's bitmap under another cache key"
        );
        assert!(last.visible_pages.contains(&(count - 1)));
        assert_eq!(probe(cx, other).scroll.offset(), other_offset);
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().selected),
            Some(0),
            "body keyboard must not select another commit"
        );
        assert_eq!(
            main_view(cx, &app).unwrap().title.as_ref(),
            DUAL,
            "body keyboard must not step another file"
        );
    }
    let old_offset = probe(cx, &old).scroll.offset();
    let new_offset = probe(cx, &new).scroll.offset();
    let retained = open(cx, &app, window, DUAL);
    assert_eq!(
        viewer(cx, &retained, true).unwrap().entity_id(),
        old.entity_id()
    );
    assert_eq!(
        viewer(cx, &retained, false).unwrap().entity_id(),
        new.entity_id()
    );
    assert_eq!(probe(cx, &old).scroll.offset(), old_offset);
    assert_eq!(probe(cx, &new).scroll.offset(), new_offset);
    drop(retained);
    drop(old);
    drop(new);
    drop(pane);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_pdf_continuous_sides");
}
