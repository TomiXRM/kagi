//! Focused C1 GUI adapters. Run each with its exact KAGI_GUI_E2E_ONLY filter.
use crate::macos::{git, mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi::app::LegacyBusy;
use kagi::ui::e2e;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn content_fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "feature"]);
    std::fs::write(repo.join("file.txt"), "feature\n").unwrap();
    git(repo, &["commit", "-qam", "feature"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("file.txt"), "main\n").unwrap();
    git(repo, &["commit", "-qam", "main"]);
    let status = std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .unwrap();
    assert!(!status.success());
    fixture
}

fn dir_file_fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("base"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "file-side"]);
    std::fs::write(repo.join("thing"), "file side\n").unwrap();
    git(repo, &["add", "thing"]);
    git(repo, &["commit", "-qm", "file"]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["checkout", "-qb", "dir-side"]);
    std::fs::create_dir(repo.join("thing")).unwrap();
    std::fs::write(repo.join("thing/child"), "directory side\n").unwrap();
    git(repo, &["add", "thing/child"]);
    git(repo, &["commit", "-qm", "directory"]);
    git(repo, &["checkout", "-q", "file-side"]);
    let repository = git2::Repository::open(repo).unwrap();
    let other = repository
        .revparse_single("dir-side")
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let annotated = repository.find_annotated_commit(other.id()).unwrap();
    repository.merge(&[&annotated], None, None).unwrap();
    drop(annotated);
    drop(other);
    drop(repository);
    fixture
}

fn click_control(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle, name: &'static str) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let bounds = e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} control was not laid out"));
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
}

fn wait_idle(cx: &mut VisualTestAppContext, app: &gpui::Entity<kagi::ui::KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).busy_op.is_none()) {
            break;
        }
        assert!(Instant::now() < deadline, "conflict adapter did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn scenario_conflict_save_boundary(cx: &mut VisualTestAppContext) {
    // A marker-bearing owned draft is a Backend-recorded refusal and preserves
    // the long-standing footer/klog contract byte-for-byte.
    {
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.detect_conflict_mode(cx));
        cx.run_until_parked();
        let conflict = cx.read(|cx| app.read(cx).conflict.clone()).unwrap();
        conflict.update(cx, |view, cx| {
            view.conflict_open_editor(Path::new("file.txt"));
            view.conflict_editor_reset_all(Path::new("file.txt"));
            cx.notify();
        });
        assert!(cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().expect("conflict mode");
            matches!(
                mode.buffer.conflict_draft(Path::new("file.txt")),
                Some(kagi_domain::conflict_family::ConflictDraft::Text(bytes))
                    if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
            )
        }));
        click_control(cx, window, "conflict-save");
        cx.run_until_parked();
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "conflict-save:merge")
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Refused { .. }));
        const EXPECTED_REFUSAL_FOOTER: &str = "conflict-save:merge: refused (1 blocker)";
        assert!(cx.read(|cx| matches!(
            &app.read(cx).status_footer,
            kagi::ui::FooterStatus::Failed(message)
                if message.as_ref() == EXPECTED_REFUSAL_FOOTER
        )));
        drop(conflict);
        unmount(cx, app, window);
    }

    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx
        .read(|cx| app.read(cx).conflict.clone())
        .expect("conflict view");
    let path = PathBuf::from("file.txt");
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(&path);
        view.conflict_editor_set_file_side(
            &path,
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        view.result_editing = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let input = cx
        .read(|cx| {
            conflict
                .read(cx)
                .editor_inputs
                .as_ref()
                .map(|inputs| inputs.result.clone())
        })
        .expect("result input");
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    let expected = cx.read(|cx| input.read(cx).value().to_string());
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    assert_eq!(std::fs::read_to_string(repo.join(&path)).unwrap(), expected);
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect();
    assert_eq!(entries.len(), 1, "Backend records accepted Save once");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    drop(input);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict Save keystroke → control → exact bytes/index/receipt");
}

pub fn scenario_conflict_dir_file_boundary(cx: &mut VisualTestAppContext) {
    let fixture = dir_file_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx
        .read(|cx| app.read(cx).conflict.clone())
        .expect("D/F conflict view");

    let guard = app.update(cx, |app, _| {
        app.app_sessions
            .write_lease(Path::new(&repo), LegacyBusy(false))
            .unwrap()
    });
    conflict.update(cx, |view, cx| {
        view.writer_busy = true;
        cx.notify();
    });
    click_control(cx, window, "conflict-keep-directory");
    cx.run_until_parked();
    assert!(read_oplog_tail_for_repo(&repo, 100)
        .iter()
        .all(|entry| !entry.op.starts_with("conflict-dir-file:")));
    guard.complete();
    conflict.update(cx, |view, cx| {
        view.writer_busy = false;
        cx.notify();
    });
    click_control(cx, window, "conflict-keep-directory");
    wait_idle(cx, &app);
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-dir-file:keep-directory")
        .collect();
    assert_eq!(entries.len(), 1, "Backend records accepted D/F once");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    let index = git2::Repository::open(&repo).unwrap().index().unwrap();
    assert!(index.get_path(Path::new("thing"), 0).is_none());
    assert!(index.get_path(Path::new("thing/child"), 0).is_some());
    drop(index);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict D/F disabled while leased → control → exact index/receipt");
}
