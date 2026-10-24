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

pub fn scenario_conflict_width_recovery_survives_other_file_save(cx: &mut VisualTestAppContext) {
    use crate::conflict_draft_revision::detect;
    use kagi_git::resolution::lines_to_text;

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let a = Path::new("a.txt");
    let b = Path::new("b.txt");
    git(repo, &["init", "-q", "-b", "main"]);
    for (name, body) in [("a.txt", "base A\n"), ("b.txt", "a\n")] {
        std::fs::write(repo.join(name), body).unwrap();
    }
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "incoming"]);
    std::fs::write(repo.join(a), "incoming A\n").unwrap();
    std::fs::write(repo.join(b), "d\n").unwrap();
    git(repo, &["commit", "-qam", "incoming"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join(a), "current A\n").unwrap();
    std::fs::write(repo.join(b), "b\n=======\nc\n").unwrap();
    git(repo, &["commit", "-qam", "current"]);
    let merge = std::process::Command::new("git")
        .args(["merge", "incoming"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(!merge.status.success());
    assert!(String::from_utf8_lossy(&merge.stdout).contains("CONFLICT"));

    let backend = kagi_git::Backend::open(repo).unwrap();
    let mut legacy = backend.resolution_buffer_from_repo().unwrap();
    assert_eq!(
        backend
            .materialized_markers(&legacy, b)
            .unwrap()
            .marker_width,
        8
    );
    let old_markers =
        "<<<<<<< Current\nb\n=======\nc\n||||||| Base\na\n=======\nd\n>>>>>>> Incoming\n";
    assert!(legacy.ensure_hunks(b, old_markers, 7));
    let body = "UNSELECTED B LEGACY WIDTH DRAFT 日本語\nkeep this exact body\n";
    legacy.set_manual_text(b, body).unwrap();
    legacy.autosave().unwrap();
    let b_bytes = std::fs::read(repo.join(b)).unwrap();
    let stages = || {
        let repository = git2::Repository::open(repo).unwrap();
        let index = repository.index().unwrap();
        assert!(index.get_path(b, 0).is_none());
        (1..=3)
            .map(|stage| {
                let entry = index.get_path(b, stage).unwrap();
                (entry.id, entry.mode, entry.path)
            })
            .collect::<Vec<_>>()
    };
    let b_stages = stages();
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    detect(cx, &app, window);
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    let assert_recovery = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().unwrap();
            assert!(!mode.buffer.has_resolution(b));
            assert_eq!(mode.buffer.resolved_text(b), None);
            let drafts: Vec<_> = mode
                .buffer
                .recovery_drafts()
                .filter(|draft| draft.path == b)
                .collect();
            assert_eq!(drafts.len(), 1);
            assert_eq!(drafts[0].result.map(lines_to_text).as_deref(), Some(body));
            assert!(drafts[0].raw_result.is_none());
            assert_eq!(
                mode.session
                    .files
                    .iter()
                    .find(|file| file.path == b)
                    .unwrap()
                    .status,
                kagi_git::ConflictStatus::Unresolved
            );
            assert!(!mode.can_continue());
            assert!(mode.continue_blocker().is_some());
        });
        assert_eq!(std::fs::read(repo.join(b)).unwrap(), b_bytes);
        assert_eq!(stages(), b_stages);
    };
    assert_recovery(cx);
    let revision = cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(mode.session.files.len(), 2);
        assert_eq!(mode.session.files[mode.selected_file.unwrap()].path, a);
        assert_eq!(view.editing.as_deref(), Some(a));
        mode.revision.clone()
    });
    click_control(cx, window, "conflict-hunk-0-current-use");
    cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(mode.session.files[mode.selected_file.unwrap()].path, a);
        assert_eq!(view.editing.as_deref(), Some(a));
        assert_eq!(mode.buffer.resolved_text(a).as_deref(), Some("current A\n"));
    });
    assert_recovery(cx);
    assert_eq!(repo_fingerprint(repo), before);
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.run_until_parked();
    assert_eq!(std::fs::read(repo.join(a)).unwrap(), b"current A\n");
    {
        let repository = git2::Repository::open(repo).unwrap();
        let index = repository.index().unwrap();
        let entry = index.get_path(a, 0).unwrap();
        assert_eq!(
            repository.find_blob(entry.id).unwrap().content(),
            b"current A\n"
        );
        for stage in 1..=3 {
            assert!(index.get_path(a, stage).is_none());
        }
    }
    // Check before selecting B: opening it would revalidate its hunks and hide
    // the retain_drafts regression in changed-observation detection.
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            conflict.entity_id()
        );
        assert!(state.ui().conflict_detected);
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_ne!(
            mode.revision, revision,
            "Save A must change the observation"
        );
        assert_eq!(
            &mode.revision,
            state.view().operation.as_ref().unwrap().revision()
        );
        assert_eq!(view.selected_saved.as_deref(), Some(a));
    });
    assert_recovery(cx);
    detect(cx, &app, window);
    assert_recovery(cx);

    click_control(cx, window, "conflict-next-file");
    cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().unwrap();
        assert_eq!(mode.session.files[mode.selected_file.unwrap()].path, b);
        assert_eq!(view.editing.as_deref(), Some(b));
    });
    assert_recovery(cx);
    let before_disabled_save = repo_fingerprint(repo);
    let receipts_before: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    // As in conflict_refusal, click the laid-out disabled Save and prove it
    // cannot dispatch a mutation (including a refused backend operation).
    click_control(cx, window, "conflict-save");
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(repo_fingerprint(repo), before_disabled_save);
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        receipts_before
    );
    assert_recovery(cx);
    crate::conflict_draft_inspection::show_log(cx, &app, window);
    let inspection = crate::conflict_draft_inspection::open_drafts(cx, &app, window, body);
    assert!(inspection.contains("b.txt"));
    click_control(cx, window, "oplog-preserved-drafts-back");
    assert_recovery(cx);
    assert_eq!(repo_fingerprint(repo), before_disabled_save);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict width recovery survives another file Save and re-detection");
}
