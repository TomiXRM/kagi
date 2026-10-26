//! Real main-diff consumers of bounded PDF reads across immutable/mutable sources.
use crate::diff_highlight::{commit_image_index, settle};
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::native_ax;
use crate::pr_viewed::{paint, rev};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{commit_panel::CommitPanelFileRef, e2e, main_diff_pane::MainDiffPane, KagiApp};
use kagi_git::{Backend, CommitId};
use kagi_ui_core::i18n::Msg;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const LIMIT: usize = 10 * 1024 * 1024;
const SIGNATURE: &str = "signature.note";

// All ASCII, with a stable preamble so changing geometry omits the signature
// from Git's text hunks. Padding is a PDF comment before the objects/xref, not
// trailing garbage which a parser might elect to ignore.
fn document(width: usize, size: Option<usize>) -> Vec<u8> {
    let build = |padding: usize| {
        let stream = "0 0 1 rg 0 0 50 50 re f\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_owned(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} 200] /Resources << >> /Contents 4 0 R >>"),
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        for _ in 0..24 {
            bytes.extend_from_slice(b"% stable preamble\n");
        }
        bytes.push(b'%');
        bytes.resize(bytes.len() + padding, b' ');
        bytes.push(b'\n');
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        bytes
    };
    let Some(size) = size else {
        return build(0);
    };
    let mut padding = size - build(0).len();
    loop {
        let bytes = build(padding);
        if bytes.len() == size {
            return bytes;
        }
        padding = if bytes.len() > size {
            padding - (bytes.len() - size)
        } else {
            padding + (size - bytes.len())
        };
    }
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

fn dimensions(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    old: bool,
) -> (f32, f32) {
    paint(cx, window);
    let pane = pane(cx, app);
    let viewer = cx
        .read(|cx| pane.read(cx).pdf_viewer_for_e2e(old, cx))
        .expect("present PDF side must own the shared viewer");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        paint(cx, window);
        let pdf = cx.read(|cx| viewer.read(cx).pdf_probe());
        if pdf.settled && (pdf.image.is_some() || pdf.error.is_some()) {
            assert!(
                pdf.error.is_none(),
                "PDF parse/raster failure: {:?}",
                pdf.error
            );
            assert_eq!(pdf.page_count, 1);
            let image = pdf.image.as_ref().expect("actual shared raster output");
            let size = image.size(0);
            let width = size.width.0 as usize;
            let height = size.height.0 as usize;
            let pixel = ((height - 2) * width + 2) * 4;
            assert_eq!(
                &image.as_bytes(0).unwrap()[pixel..pixel + 4],
                &[255, 0, 0, 255],
                "the document's blue corner must be rasterized, not only parsed"
            );
            return pdf.page_sizes[0];
        }
        assert!(Instant::now() < deadline, "PDF media did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_pair(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    old_width: usize,
    new_width: usize,
) {
    assert_eq!(dimensions(cx, app, window, true), (old_width as f32, 200.));
    assert_eq!(dimensions(cx, app, window, false), (new_width as f32, 200.));
    let pane = pane(cx, app);
    cx.read(|cx| {
        let view = &pane.read(cx).view;
        assert!(view.rows.is_empty() && view.images.is_none() && view.lang.is_none());
    });
}

fn compare_open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    settle(cx, window);
    let index = cx.read(|cx| {
        app.read(cx)
            .ui()
            .compare_view
            .as_ref()
            .unwrap()
            .read(cx)
            .view()
            .files
            .iter()
            .position(|file| file.path == Path::new(SIGNATURE))
            .unwrap()
    });
    app.update(cx, |app, cx| app.open_main_diff_compare(index, cx));
    paint(cx, window);
}

fn wip_open(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    path: &str,
    staged: bool,
) {
    settle(cx, window);
    let file = cx.read(|cx| {
        let state = &app
            .read(cx)
            .ui()
            .commit_panel
            .as_ref()
            .unwrap()
            .read(cx)
            .state;
        let files = if staged {
            &state.staged
        } else {
            &state.unstaged
        };
        let index = files
            .iter()
            .position(|file| file.path == Path::new(path))
            .unwrap();
        if staged {
            CommitPanelFileRef::Staged { index }
        } else {
            CommitPanelFileRef::Unstaged { index }
        }
    });
    app.update(cx, |app, cx| app.open_main_diff_wip(file, cx));
    paint(cx, window);
}

pub fn scenario_pdf_media_sources(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join(SIGNATURE), document(100, None)).unwrap();
    git(&repo, &["add", SIGNATURE]);
    git(&repo, &["commit", "-q", "-m", "PDF bounded source base"]);
    let base = CommitId(rev(&repo, "HEAD"));
    std::fs::write(repo.join(SIGNATURE), document(200, None)).unwrap();
    git(&repo, &["add", SIGNATURE]);
    git(&repo, &["commit", "-q", "-m", "PDF bounded source head"]);
    let head = CommitId(rev(&repo, "HEAD"));
    std::fs::write(repo.join(SIGNATURE), document(300, None)).unwrap();
    git(&repo, &["add", SIGNATURE]);
    std::fs::write(repo.join(SIGNATURE), document(400, None)).unwrap();
    let diff = Backend::open(&repo)
        .unwrap()
        .commit_file_diff(&head, Path::new(SIGNATURE))
        .unwrap();
    assert!(!diff.is_binary);
    assert!(diff
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .all(|line| !line.content.contains("%PDF-")));

    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    let index = commit_image_index(cx, &app, SIGNATURE);
    app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
    assert_pair(cx, &app, window, 100, 200);

    app.update(cx, |app, cx| {
        app.open_compare_with_head(base.clone(), Some(cx))
    });
    compare_open(cx, &app, window);
    assert_pair(cx, &app, window, 100, 200);
    app.update(cx, |app, cx| {
        app.open_compare_with_working_tree(base, Some(cx))
    });
    compare_open(cx, &app, window);
    assert_pair(cx, &app, window, 100, 400);

    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    wip_open(cx, &app, window, SIGNATURE, true);
    assert_pair(cx, &app, window, 200, 300);
    wip_open(cx, &app, window, SIGNATURE, false);
    assert_pair(cx, &app, window, 300, 400);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pdf_media_sources");
}

fn assert_failed_side(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    old: bool,
) {
    paint(cx, window);
    let pane = pane(cx, app);
    assert!(cx
        .read(|cx| pane.read(cx).pdf_viewer_for_e2e(old, cx))
        .is_none());
    let id = if old {
        "main-diff-pdf-old"
    } else {
        "main-diff-pdf-new"
    };
    assert_eq!(
        native_ax::node(cx, window, id.into())["aria"]["description"],
        Msg::DiffPdfReadFailed.t(),
        "unreadable present side must be a read failure, not absence or another file's preview"
    );
}

pub fn scenario_pdf_media_symlink(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("outside.pdf");
    std::fs::write(&target, document(500, None)).unwrap();
    std::os::unix::fs::symlink(&target, repo.join("link.pdf")).unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    wip_open(cx, &app, window, "link.pdf", false);
    let pane = pane(cx, &app);
    cx.read(|cx| {
        let pane = pane.read(cx);
        assert!(pane.pdf_viewer_for_e2e(false, cx).is_none());
        let pair = pane
            .view
            .side_pair
            .as_ref()
            .expect("shared symlink presentation");
        assert!(
            matches!(&pair.right, kagi::ui::SideContent::Symlink { target: Some(link), meta }
            if link.as_ref() == target.to_string_lossy() && meta.error.is_none())
        );
    });
    assert_eq!(
        native_ax::node(cx, window, "main-diff-pdf-old".into())["aria"]["description"],
        Msg::DiffPdfNoPrevious.t()
    );
    assert_eq!(before, repo_fingerprint(&repo));
    assert_eq!(std::fs::read(&target).unwrap(), document(500, None));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pdf_media_symlink");
}

pub fn scenario_pdf_media_bounds(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for (path, width, size) in [
        ("old-over.pdf", 100, LIMIT + 1),
        ("new-over.pdf", 200, LIMIT),
    ] {
        std::fs::write(repo.join(path), document(width, Some(size))).unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "PDF size base"]);
    for (path, width, size) in [
        ("old-over.pdf", 300, LIMIT),
        ("new-over.pdf", 400, LIMIT + 1),
    ] {
        std::fs::write(repo.join(path), document(width, Some(size))).unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "PDF size target"]);
    // Signature-only oversized source still classifies as PDF without a full
    // workdir read, even though a normal text/binary heuristic is insufficient.
    std::fs::write(repo.join(SIGNATURE), document(500, None)).unwrap();
    git(&repo, &["add", SIGNATURE]);
    let huge = std::fs::OpenOptions::new()
        .write(true)
        .open(repo.join(SIGNATURE))
        .unwrap();
    huge.set_len(LIMIT as u64 + 1).unwrap();

    let (app, window) = mount(cx, &repo);
    native_ax::activate(cx);
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    settle(cx, window);
    for (path, failed_old, valid_width) in
        [("old-over.pdf", true, 300), ("new-over.pdf", false, 200)]
    {
        let index = commit_image_index(cx, &app, path);
        app.update(cx, |app, cx| app.open_main_diff_commit(index, cx));
        assert_failed_side(cx, &app, window, failed_old);
        assert_eq!(
            dimensions(cx, &app, window, !failed_old),
            (valid_width as f32, 200.)
        );
    }
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    wip_open(cx, &app, window, SIGNATURE, false);
    assert_failed_side(cx, &app, window, false);
    assert_eq!(dimensions(cx, &app, window, true), (500., 200.));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pdf_media_bounds");
}
