//! #1751: merged passthrough is not a stage reconstruction; delimiters cannot collide.
use crate::app_conflict::{click_control, wait_idle};
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::VisualTestAppContext;
use std::path::Path;

fn fixture(base: &str, current: &str, incoming: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("file.txt"), base).unwrap();
    git(repo, &["add", "file.txt"]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "incoming"]);
    std::fs::write(repo.join("file.txt"), incoming).unwrap();
    git(repo, &["commit", "-qam", "incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("file.txt"), current).unwrap();
    git(repo, &["commit", "-qam", "current"]);
    let merge = std::process::Command::new("git")
        .args(["merge", "incoming"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(!merge.status.success());
    assert!(String::from_utf8_lossy(&merge.stdout).contains("CONFLICT"));
    tmp
}

struct Case<'a> {
    base: &'a str,
    current: &'a str,
    incoming: &'a str,
    sides: &'a [&'a str],
    expected: &'a str,
    width: usize,
}

fn save_case(cx: &mut VisualTestAppContext, case: Case<'_>) {
    let tmp = fixture(case.base, case.current, case.incoming);
    let repo = tmp.path();
    let before = repo_fingerprint(repo);
    let backend = kagi_git::Backend::open(repo).unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert_eq!(
        backend
            .materialized_markers(&buffer, Path::new("file.txt"))
            .unwrap()
            .marker_width,
        case.width
    );
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    cx.read(|cx| {
        assert_eq!(
            conflict
                .read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .hunk_count(Path::new("file.txt")),
            case.sides.len()
        );
    });
    for (hunk, side) in case.sides.iter().enumerate() {
        click_control(cx, window, &format!("conflict-hunk-{hunk}-{side}-use"));
    }
    cx.read(|cx| {
        let view = conflict.read(cx);
        let buffer = &view.mode.as_ref().unwrap().buffer;
        let path = Path::new("file.txt");
        assert_eq!(buffer.resolved_text(path).as_deref(), Some(case.expected));
        assert!(buffer.has_resolution(path) && !buffer.cached_marker_residue(path));
        assert!(!buffer.has_marker_residue(path));
    });
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "choices are drafts, not writes"
    );
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.run_until_parked();
    cx.read(|cx| {
        assert_eq!(
            conflict.read(cx).selected_saved.as_deref(),
            Some(Path::new("file.txt"))
        );
    });
    assert_eq!(
        std::fs::read(repo.join("file.txt")).unwrap(),
        case.expected.as_bytes()
    );
    let repository = git2::Repository::open(repo).unwrap();
    let index = repository.index().unwrap();
    assert!(!index.has_conflicts());
    let entry = index.get_path(Path::new("file.txt"), 0).unwrap();
    assert_eq!(
        repository.find_blob(entry.id).unwrap().content(),
        case.expected.as_bytes()
    );
    unmount(cx, app, window);
}

pub fn scenario_conflict_merged_passthrough(cx: &mut VisualTestAppContext) {
    save_case(
        cx,
        Case {
            base: "a\nX\nb\n",
            current: "a\nY\nb\n",
            incoming: "a\nZ\nB\n",
            sides: &["current"],
            expected: "a\nY\nb\n",
            width: 7,
        },
    );
    save_case(
        cx,
        Case {
            base: "left-base\nc1\nc2\nc3\nX\nm1\nm2\nm3\nQ\nr1\nr2\nr3\nright-base\n",
            current: "left-current\nc1\nc2\nc3\nY\nm1\nm2\nm3\nR\nr1\nr2\nr3\nright-base\n",
            incoming: "left-base\nc1\nc2\nc3\nZ\nm1\nm2\nm3\nS\nr1\nr2\nr3\nright-incoming\n",
            sides: &["current", "incoming"],
            expected: "left-current\nc1\nc2\nc3\nY\nm1\nm2\nm3\nS\nr1\nr2\nr3\nright-incoming\n",
            width: 7,
        },
    );
    eprintln!("[gui-e2e] PASS conflict merged passthrough: advisory and mixed-hunk clean changes");
}

pub fn scenario_conflict_marker_width(cx: &mut VisualTestAppContext) {
    for (current, incoming, side, expected, width) in [
        ("b\n=======\nc\n", "d\n", "current", "b\n=======\nc\n", 8),
        ("b\n", "<<<<<<< x\n", "incoming", "<<<<<<< x\n", 8),
        (
            "b\n=========\nc\n",
            "d\n",
            "current",
            "b\n=========\nc\n",
            10,
        ),
    ] {
        save_case(
            cx,
            Case {
                base: "a\n",
                current,
                incoming,
                sides: &[side],
                expected,
                width,
            },
        );
    }
    eprintln!(
        "[gui-e2e] PASS conflict marker widths: literal separator/opener and width 10 save exactly"
    );
}

pub fn scenario_conflict_width_draft_recovery(cx: &mut VisualTestAppContext) {
    let tmp = fixture("a\n", "b\n=======\nc\n", "d\n");
    let backend = kagi_git::Backend::open(tmp.path()).unwrap();
    let mut legacy = backend.resolution_buffer_from_repo().unwrap();
    let path = Path::new("file.txt");
    let old_markers =
        "<<<<<<< Current\nb\n=======\nc\n||||||| Base\na\n=======\nd\n>>>>>>> Incoming\n";
    assert!(legacy.ensure_hunks(path, old_markers, 7));
    let body = "LEGACY WIDTH DRAFT 日本語\n";
    legacy.set_manual_text(path, body).unwrap();
    legacy.autosave().unwrap();
    let before = repo_fingerprint(tmp.path());
    let (app, window) = mount(cx, tmp.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert!(!mode.buffer.has_resolution(path));
        assert_eq!(mode.buffer.recovery_drafts().count(), 1);
        assert_eq!(
            mode.session.files[0].status,
            kagi_git::ConflictStatus::Unresolved
        );
    });
    crate::conflict_draft_inspection::show_log(cx, &app, window);
    let inspection = crate::conflict_draft_inspection::open_drafts(cx, &app, window, body);
    assert!(inspection.contains("file.txt"));
    assert_eq!(
        repo_fingerprint(tmp.path()),
        before,
        "inspection must not apply the old body"
    );
    cx.read(|cx| {
        assert!(!conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .has_resolution(path));
    });
    click_control(cx, window, "oplog-preserved-drafts-back");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict width draft recovery: exact legacy body inspected without adoption");
}
