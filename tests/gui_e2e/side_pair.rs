//! #1742: real prepared media → side adoption → Save → independent Git oracle.
use crate::app_conflict::{click_control, wait_idle};
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{conflict_view::ConflictView, e2e};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
struct Blob {
    oid: git2::Oid,
    mode: u32,
    bytes: Vec<u8>,
}

fn index_blob(repo: &Path, path: &str, stage: i32) -> Blob {
    // Reopen independently: never derive expected content from the UI preview.
    let repository = git2::Repository::open(repo).unwrap();
    let index = repository.index().unwrap();
    let entry = index.get_path(Path::new(path), stage).unwrap();
    let bytes = repository.find_blob(entry.id).unwrap().content().to_vec();
    Blob {
        oid: entry.id,
        mode: entry.mode,
        bytes,
    }
}

fn fixture(path: &str, base: &[u8], current: &[u8], incoming: &[u8]) -> tempfile::TempDir {
    let fixture = build_fixture();
    let repo = fixture.path();
    // PDFs can be ASCII; Git must treat the genuine document as a raw conflict.
    std::fs::write(repo.join(".gitattributes"), format!("{path} binary\n")).unwrap();
    std::fs::write(repo.join(path), base).unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "media base"]);
    git(repo, &["checkout", "-qb", "media-incoming"]);
    std::fs::write(repo.join(path), incoming).unwrap();
    git(repo, &["commit", "-qam", "media incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join(path), current).unwrap();
    std::fs::set_permissions(repo.join(path), std::fs::Permissions::from_mode(0o755)).unwrap();
    git(repo, &["add", path]);
    git(repo, &["update-index", "--chmod=+x", path]);
    git(repo, &["commit", "-qm", "media current"]);
    let repository = git2::Repository::open(repo).unwrap();
    let commit = repository
        .revparse_single("media-incoming")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let annotated = repository.find_annotated_commit(commit.id()).unwrap();
    repository.merge(&[&annotated], None, None).unwrap();
    assert!(repository.index().unwrap().has_conflicts());
    assert_eq!(index_blob(repo, path, 2).mode, 0o100755);
    assert_eq!(index_blob(repo, path, 3).mode, 0o100644);
    assert_ne!(index_blob(repo, path, 2).oid, index_blob(repo, path, 3).oid);
    drop(annotated);
    drop(commit);
    drop(repository);
    fixture
}

fn wait_preview(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
) {
    crate::recovery_diff_presentation::resize_image_frame(cx, window, (1392., 883.));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        crate::recovery_layout::draw(cx, window, (1392., 883.));
        if conflict.read_with(cx, |view, _| view.raw_preview_state_for_e2e().0) == "ready" {
            // Paint the accepted pair, rather than retaining loading-frame bounds.
            crate::recovery_layout::draw(cx, window, (1392., 883.));
            return;
        }
        assert!(
            Instant::now() < deadline,
            "raw media preparation did not settle"
        );
        cx.advance_clock(Duration::from_millis(16));
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_presentation(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    conflict: &Entity<ConflictView>,
    pdf: bool,
) {
    wait_preview(cx, window, conflict);
    for (old, side) in [(true, "current"), (false, "incoming")] {
        if pdf {
            let viewer = cx
                .read(|cx| conflict.read(cx).pdf_viewer_for_e2e(old, cx))
                .expect("prepared PDF side owns the shared production viewer");
            let probe = crate::commit_pdf::wait_pdf(cx, &viewer, window);
            assert!(
                probe.error.is_none(),
                "PDF parse/raster error: {:?}",
                probe.error
            );
            assert!(probe.image.is_some(), "genuine PDF page was rasterized");
            assert_eq!(probe.page_count, if old { 3 } else { 5 });
            assert_eq!(probe.page_sizes.len(), probe.page_count);
            drop(viewer);
        } else {
            let id = format!("conflict-image-{side}");
            let bounds = e2e::control_bounds(window.window_id(), &id)
                .expect("prepared image is rendered by the shared side pair");
            assert!(bounds.size.width > gpui::px(0.));
            assert!(bounds.size.height > gpui::px(0.));
            let caption = e2e::diff_presentation::image_caption(&id)
                .expect("decoded image metadata is presented");
            assert!(caption.contains("2 × 2 px"), "{caption}");
            assert!(caption.contains(kagi_ui_core::i18n::Msg::ImageDimensions.t()));
            assert!(caption.contains(kagi_ui_core::i18n::Msg::ImageFileSize.t()));
        }
        assert!(
            e2e::control_bounds(window.window_id(), &format!("conflict-raw-{side}-use")).is_some()
        );
    }
}

fn choose_and_save(
    cx: &mut VisualTestAppContext,
    fixture: &tempfile::TempDir,
    path: &str,
    side: &str,
    pdf: bool,
) {
    let repo = fixture.path().canonicalize().unwrap();
    let worktree_bytes = std::fs::read(repo.join(path)).unwrap();
    let worktree_mode = std::fs::metadata(repo.join(path))
        .unwrap()
        .permissions()
        .mode();
    let ancestor = index_blob(&repo, path, 1);
    let current = index_blob(&repo, path, 2);
    let incoming = index_blob(&repo, path, 3);
    let expected = if side == "current" {
        &current
    } else {
        &incoming
    };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_select_file(0);
        cx.notify();
    });
    assert_presentation(cx, window, &conflict, pdf);
    click_control(cx, window, &format!("conflict-raw-{side}-use"));
    let chosen = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .raw_resolution(Path::new(path))
            .expect("Use adopts a raw resolution in the authoritative buffer")
    });
    assert_eq!(chosen.oid, expected.oid);
    assert_eq!(chosen.mode, expected.mode);
    // Adoption only changes the resolution buffer; Save owns the repository write.
    assert_eq!(index_blob(&repo, path, 2), current);
    assert_eq!(index_blob(&repo, path, 3), incoming);
    assert!(git2::Repository::open(&repo)
        .unwrap()
        .index()
        .unwrap()
        .get_path(Path::new(path), 0)
        .is_none());
    if !pdf {
        click_control(cx, window, &format!("conflict-raw-{side}-change"));
        cx.read(|cx| {
            let mode = conflict.read(cx).mode.as_ref().unwrap();
            assert!(mode.buffer.raw_resolution(Path::new(path)).is_none());
            assert!(!mode.buffer.has_resolution(Path::new(path)));
        });
        assert_eq!(index_blob(&repo, path, 1), ancestor);
        assert_eq!(index_blob(&repo, path, 2), current);
        assert_eq!(index_blob(&repo, path, 3), incoming);
        click_control(cx, window, &format!("conflict-raw-{side}-use"));
        let chosen = cx.read(|cx| {
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .raw_resolution(Path::new(path))
                .expect("Use after Change re-adopts the chosen source")
        });
        assert_eq!(chosen.oid, expected.oid);
        assert_eq!(chosen.mode, expected.mode);
    }
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    assert_eq!(
        &index_blob(&repo, path, 0),
        expected,
        "{path}/{side}: exact chosen blob, mode and bytes"
    );
    // Raw Save stages the frozen blob, deliberately never rewriting the worktree.
    assert_eq!(std::fs::read(repo.join(path)).unwrap(), worktree_bytes);
    assert_eq!(
        std::fs::metadata(repo.join(path))
            .unwrap()
            .permissions()
            .mode(),
        worktree_mode
    );
    let repository = git2::Repository::open(&repo).unwrap();
    let index = repository.index().unwrap();
    for stage in [1, 2, 3] {
        assert!(index.get_path(Path::new(path), stage).is_none());
    }
    assert!(!index.has_conflicts());
    let saves: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect();
    assert_eq!(saves.len(), 1, "one accepted Save receipt");
    assert!(matches!(saves[0].outcome, OpOutcome::Success { .. }));
    drop(index);
    drop(repository);
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_side_pair_image(cx: &mut VisualTestAppContext) {
    let base = crate::diff_highlight::png_bytes([0, 255, 0, 255]);
    let current = crate::diff_highlight::png_bytes([255, 0, 0, 255]);
    let incoming = crate::diff_highlight::png_bytes([0, 0, 255, 255]);
    for side in ["current", "incoming"] {
        let fixture = fixture("pair.png", &base, &current, &incoming);
        choose_and_save(cx, &fixture, "pair.png", side, false);
    }
}

pub fn scenario_conflict_side_pair_pdf(cx: &mut VisualTestAppContext) {
    // Reuse the existing font-free valid PDF generator, not magic-byte stand-ins.
    let generated = crate::commit_pdf::fixture();
    let repository = git2::Repository::open(generated.path()).unwrap();
    let document = |spec: &str| {
        repository
            .revparse_single(spec)
            .unwrap()
            .peel_to_blob()
            .unwrap()
            .content()
            .to_vec()
    };
    let current = document("HEAD^:dual.pdf");
    let incoming = document("HEAD:dual.pdf");
    let base = std::fs::read(generated.path().join("dual.pdf")).unwrap();
    for side in ["current", "incoming"] {
        let fixture = fixture("pair.pdf", &base, &current, &incoming);
        choose_and_save(cx, &fixture, "pair.pdf", side, true);
    }
}

pub fn scenario_conflict_side_pair_modify_delete(cx: &mut VisualTestAppContext) {
    use kagi_ui_core::i18n::{self, Lang, Msg};
    let _settings = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    let base = crate::diff_highlight::png_bytes([0, 255, 0, 255]);
    let current = crate::diff_highlight::png_bytes([255, 0, 0, 255]);
    let incoming = crate::diff_highlight::png_bytes([0, 0, 255, 255]);
    for lang in [Lang::En, Lang::Ja] {
        for (absent_stage, absent_side) in [(2, "current"), (3, "incoming")] {
            let fixture = fixture("pair.png", &base, &current, &incoming);
            let repo = fixture.path().canonicalize().unwrap();
            let repository = git2::Repository::open(&repo).unwrap();
            let head = repository.head().unwrap().target().unwrap();
            let mut index = repository.index().unwrap();
            index.remove(Path::new("pair.png"), absent_stage).unwrap();
            index.write().unwrap();
            drop(index);
            let surviving_stage = if absent_stage == 2 { 3 } else { 2 };
            let survivor = index_blob(&repo, "pair.png", surviving_stage);
            std::fs::write(repo.join("pair.png"), &survivor.bytes).unwrap();
            let before = std::fs::read(repo.join(".git/index")).unwrap();
            drop(repository);
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(lang, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
            conflict.update(cx, |view, cx| {
                view.conflict_select_file(0);
                cx.notify();
            });
            wait_preview(cx, window, &conflict);
            let action = format!("conflict-raw-{absent_side}-use");
            let (role, name, label) = kagi::ui::conflict_editor_a11y::control(&action).unwrap();
            assert_eq!(role, gpui::Role::Button);
            assert_eq!(name, Msg::ConflictUseDeletedSide.t());
            assert_eq!(label, Msg::ConflictUseDeletedSide.t());
            click_control(cx, window, &action);
            cx.read(|cx| {
                let app = app.read(cx);
                let modal = app
                    .conflict_resolution_modal()
                    .expect("frozen deletion sheet");
                assert_eq!(modal.path, Path::new("pair.png"));
                assert_eq!(
                    modal.changes.remove_index,
                    vec![Path::new("pair.png").to_path_buf()]
                );
                assert_eq!(
                    modal.changes.remove_worktree,
                    vec![Path::new("pair.png").to_path_buf()]
                );
            });
            cx.simulate_keystrokes(window, "enter");
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_some()));
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), before);
            assert_eq!(
                std::fs::read(repo.join("pair.png")).unwrap(),
                survivor.bytes
            );
            cx.simulate_keystrokes(window, "escape");
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_none()));
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), before);
            assert_eq!(
                std::fs::read(repo.join("pair.png")).unwrap(),
                survivor.bytes
            );
            assert!(read_oplog_tail_for_repo(&repo, 100).is_empty());
            click_control(cx, window, &action);
            click_control(cx, window, "conflict-resolution-confirm");
            wait_idle(cx, &app);
            crate::busy_dismiss::ready(cx, &app);
            let repository = git2::Repository::open(&repo).unwrap();
            let index = repository.index().unwrap();
            for stage in 0..=3 {
                assert!(index.get_path(Path::new("pair.png"), stage).is_none());
            }
            assert!(!index.has_conflicts());
            assert!(!repo.join("pair.png").exists());
            assert_eq!(repository.head().unwrap().target(), Some(head));
            assert!(repo.join(".git/MERGE_HEAD").exists());
            let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "conflict-save:merge")
                .collect();
            assert_eq!(receipts.len(), 1);
            assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
            drop(index);
            drop(repository);
            drop(conflict);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original);
}
