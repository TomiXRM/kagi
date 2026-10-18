//! #1662: compact Changes accepts its live WIP read, never a retired consumer's.
use super::*;
use crate::diff_highlight::main_view;
use crate::evidence_support::{deferred, Reply};
use kagi::ui::commit_panel::CommitPanelFileRef;
use kagi::ui::diff_view::{CompareTarget, DiffRow, MainDiffSource, MainDiffView};
use kagi::ui::BottomTab;

const ALPHA: &str = "alpha.txt";
const BETA: &str = "beta.txt";
const NAVIGATION: &str = "navigation.txt";
const LOCAL_TEXT: &str = "pending compact local alpha";
const STAGED_TEXT: &str = "newer compact staged beta";
const NAVIGATION_TEXT: &str = "authoritative committed navigation";
const SESSION_TEXT: &str = "authoritative second session";
const FOREIGN_TEXT: &str = "authoritative replacement worktree";

fn read_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    for path in [ALPHA, BETA, NAVIGATION] {
        std::fs::write(fixture.path().join(path), format!("base {path}\n")).unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(fixture.path(), &["commit", "-qm", "compact deferred base"]);
    std::fs::write(
        fixture.path().join(NAVIGATION),
        format!("{NAVIGATION_TEXT}\n"),
    )
    .unwrap();
    git(
        fixture.path(),
        &["commit", "-qam", "compact deferred navigation"],
    );
    std::fs::write(fixture.path().join(ALPHA), format!("{LOCAL_TEXT}\n")).unwrap();
    std::fs::write(fixture.path().join(BETA), format!("{STAGED_TEXT}\n")).unwrap();
    git(fixture.path(), &["add", BETA]);
    fixture
}

pub fn scenario_compact_changes_deferred_file(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    // The initial-file scenario owns the presentation matrix. These schedules
    // use the original failing compact presentation, not duplicate image/cache,
    // stash, refresh or binary-byte lifetime coverage from binary_diff_owner.
    let case = Case {
        height: 660.,
        zoom: 1.67,
        locale: "ja",
        preset: "apple-dark",
    };
    let fixture = read_fixture();
    valid_and_newer(cx, fixture.path(), case);
    for compare in [false, true] {
        navigation_retires(cx, fixture.path(), case, compare);
    }
    session_retires(cx, fixture.path(), case);
    panel_retires(cx, fixture.path(), case);
}

fn valid_and_newer(cx: &mut VisualTestAppContext, repo: &Path, case: Case) {
    let (app, win) = mount(cx, repo, case);
    let panel = changes(cx, &app, win);
    // No WIP-row/composer activation before this read. A blanket rejection of
    // compact-origin reads must fail here, even if all retirement cases pass.
    let release = hold_file(cx, &app, win, ALPHA);
    release.send(());
    assert_wip(cx, &app, win, repo, ALPHA, false, LOCAL_TEXT, &[]);

    let release = hold_file(cx, &app, win, ALPHA);
    click_file(cx, &app, win, BETA, true);
    assert_wip(cx, &app, win, repo, BETA, true, STAGED_TEXT, &[LOCAL_TEXT]);
    release.send(());
    assert_wip(cx, &app, win, repo, BETA, true, STAGED_TEXT, &[LOCAL_TEXT]);
    drop(panel);
    unmount(cx, app, win);
}

fn navigation_retires(cx: &mut VisualTestAppContext, repo: &Path, case: Case, compare: bool) {
    let (app, win) = mount(cx, repo, case);
    let panel = changes(cx, &app, win);
    let release = hold_file(cx, &app, win, ALPHA);
    let row = cx.read(|cx| {
        app.read(cx)
            .view()
            .rows
            .iter()
            .position(|row| {
                row.summary.as_ref()
                    == if compare {
                        "compact deferred base"
                    } else {
                        "compact deferred navigation"
                    }
            })
            .unwrap()
    });
    let commit = cx.read(|cx| app.read(cx).view().rows[row].id.clone());
    if compare {
        app.update(cx, |app, cx| {
            app.open_compare_with_head(commit.clone(), Some(cx));
            cx.notify();
        });
        settle(cx, win);
    } else {
        click(cx, win, &format!("graph-row-{row}"));
    }
    // Release before any new diff request: the pending WIP read must not
    // replace the authoritative Inspector/Compare with its former file.
    release.send(());
    assert_no_diff(cx, &app, win);
    commit_columns(win, case.dimensions());
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ui().selected, Some(row));
        if compare {
            let view = app.ui().compare_view.as_ref().unwrap().read(cx).view();
            assert_eq!(view.base, commit);
            assert_eq!(view.target, CompareTarget::Head);
            assert_eq!(view.files[0].path, Path::new(NAVIGATION));
        } else {
            assert!(app.ui().compare_view.is_none());
        }
    });
    click(cx, win, "inspector-file-0");
    let shown = assert_text(cx, &app, win, NAVIGATION, NAVIGATION_TEXT, &[LOCAL_TEXT]);
    if compare {
        assert!(matches!(
            shown.source,
            MainDiffSource::Compare { base, target: CompareTarget::Head, file_index: 0 }
                if base == commit
        ));
    } else {
        assert!(matches!(
            shown.source,
            MainDiffSource::Commit { row_index, file_index: 0, commit: Some(actual) }
                if row_index == row && actual == commit
        ));
    }
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(row));
    drop(panel);
    unmount(cx, app, win);
}

fn session_retires(cx: &mut VisualTestAppContext, repo: &Path, case: Case) {
    let other = build_fixture();
    const OTHER_FILE: &str = "second-session.txt";
    std::fs::write(other.path().join(OTHER_FILE), format!("{SESSION_TEXT}\n")).unwrap();
    let (app, win) = mount(cx, repo, case);
    let original = changes(cx, &app, win);
    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    let release = hold_file(cx, &app, win, ALPHA);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().canonicalize().unwrap(), cx));
    });
    let current = await_panel(cx, &app, win, other.path(), OTHER_FILE);
    assert_ne!(cx.read(|cx| current.read(cx).owner), owner);
    click_file(cx, &app, win, OTHER_FILE, false);
    assert_wip(
        cx,
        &app,
        win,
        other.path(),
        OTHER_FILE,
        false,
        SESSION_TEXT,
        &[LOCAL_TEXT],
    );
    release.send(());
    assert_wip(
        cx,
        &app,
        win,
        other.path(),
        OTHER_FILE,
        false,
        SESSION_TEXT,
        &[LOCAL_TEXT],
    );
    // Also inspect the old session after activation: rejecting a cross-tab
    // paint is insufficient if the stale read polluted its retained pane.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    let returned = await_panel(cx, &app, win, repo, ALPHA);
    assert_eq!(cx.read(|cx| app.read(cx).active_session()), Some(owner));
    assert_no_diff(cx, &app, win);
    columns(win, case.dimensions());
    click_file(cx, &app, win, BETA, true);
    assert_wip(
        cx,
        &app,
        win,
        repo,
        BETA,
        true,
        STAGED_TEXT,
        &[LOCAL_TEXT, SESSION_TEXT],
    );
    drop(returned);
    drop(current);
    drop(original);
    unmount(cx, app, win);
}

fn panel_retires(cx: &mut VisualTestAppContext, repo: &Path, case: Case) {
    let foreign = tempfile::tempdir().unwrap();
    let foreign_path = foreign.path().canonicalize().unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "compact-deferred-panel",
            foreign_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(foreign_path.join(ALPHA), format!("{FOREIGN_TEXT}\n")).unwrap();
    let (app, win) = mount(cx, repo, case);
    let original = changes(cx, &app, win);
    let release = hold_file(cx, &app, win, ALPHA);
    // The real worktree panel entry replaces this session's source entity.
    // No test-injected panel/state or binary-specific ownership seam is used.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_commit_panel_for_worktree(
                foreign_path.clone(),
                "compact-deferred-panel".into(),
                0,
                window,
                cx,
            );
        });
    })
    .unwrap();
    settle(cx, win);
    escape(cx, &app, win);
    let current = changes(cx, &app, win);
    assert_ne!(current.entity_id(), original.entity_id());
    release.send(());
    assert_no_diff(cx, &app, win);
    assert_wip_context(cx, &app, win, &foreign_path);
    columns(win, case.dimensions());
    // Same path, different actual repository and content. The replacement's
    // compact file click must work without borrowing the retired local read.
    click_file(cx, &app, win, ALPHA, false);
    assert_wip(
        cx,
        &app,
        win,
        &foreign_path,
        ALPHA,
        false,
        FOREIGN_TEXT,
        &[LOCAL_TEXT],
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .repo_path
            .as_ref()
            .map(|path| path.canonicalize().unwrap())),
        Some(repo.canonicalize().unwrap()),
        "replacing the panel must not change the active repository tab"
    );
    drop(current);
    drop(original);
    unmount(cx, app, win);
}

fn hold_file(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    path: &str,
) -> Reply<()> {
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    settle(cx, win);
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_main_diff_read_for_e2e(hold);
    click_file(cx, app, win, path, false);
    assert_no_diff(cx, app, win);
    release
}

fn click_file(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    path: &str,
    staged: bool,
) {
    let index = cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.as_ref().unwrap().read(cx);
        let files = if staged {
            &panel.state.staged
        } else {
            &panel.state.unstaged
        };
        files
            .iter()
            .position(|file| file.path == Path::new(path))
            .unwrap()
    });
    let id = format!("cp-{}-file-label-{index}", if staged { "st" } else { "us" });
    contained(
        bounds(
            win,
            if staged {
                "compact-changes-staged"
            } else {
                "compact-changes-unstaged"
            },
        ),
        bounds(win, &id),
        "activate the actual visible compact Changes file label",
    );
    click(cx, win, &id);
}

fn assert_no_diff(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    e2e::clear_control_bounds(win.window_id(), "main-diff-title");
    settle(cx, win);
    assert!(
        main_view(cx, app).is_none(),
        "retired/held WIP read must not open MainDiff"
    );
    assert!(e2e::control_bounds(win.window_id(), "main-diff-title").is_none());
}

fn assert_text(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    path: &str,
    added: &str,
    absent: &[&str],
) -> MainDiffView {
    e2e::diff_presentation::clear();
    e2e::clear_control_bounds(win.window_id(), "main-diff-title");
    settle(cx, win);
    let shown = main_view(cx, app).expect("current compact file activation must show MainDiff");
    assert_eq!(shown.title.as_ref(), path);
    assert!(
        shown.rows.iter().any(|row| {
            matches!(row, DiffRow::Line { text, .. } if text.strip_prefix('+') == Some(added))
        }),
        "visible diff must contain the actual requested repository content: {added}"
    );
    for stale in absent {
        assert!(
            !shown
                .rows
                .iter()
                .any(|row| { matches!(row, DiffRow::Line { text, .. } if text.contains(*stale)) }),
            "retired content must not leak into {path}: {stale}"
        );
    }
    let title = bounds(win, "main-diff-title");
    contained(
        viewport((WIDTH, 660.)),
        title,
        "MainDiff title is actually rendered",
    );
    assert!(title.size.width > px(0.) && title.size.height > px(0.));
    assert_eq!(e2e::diff_presentation::header().unwrap().displayed, path);
    shown
}

fn assert_wip_context(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.bottom_tab, BottomTab::Changes);
        assert!(app.bottom_panel_open);
        assert!(app.ui().selected.is_none());
        assert!(app.ui().compare_view.is_none());
        assert!(
            !app.ui().commit_panel_open,
            "Changes must not need the wide/composer visibility flag"
        );
        let panel = app.ui().commit_panel.as_ref().unwrap().read(cx);
        assert_eq!(panel.repo_path, repo.canonicalize().unwrap());
        assert_eq!(Some(panel.owner), app.active_session());
    });
    assert!(e2e::control_bounds(win.window_id(), "compact-composer").is_none());
}

fn assert_wip(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
    path: &str,
    staged: bool,
    added: &str,
    absent: &[&str],
) {
    let shown = assert_text(cx, app, win, path, added, absent);
    assert_wip_context(cx, app, win, repo);
    assert!(
        match &shown.source {
            MainDiffSource::Staged { path: actual } => staged && actual == Path::new(path),
            MainDiffSource::Unstaged { path: actual } => !staged && actual == Path::new(path),
            _ => false,
        },
        "MainDiff must retain the authoritative staged/unstaged source and path"
    );
    cx.read(|cx| {
        let panel = app.read(cx).ui().commit_panel.as_ref().unwrap().read(cx);
        let selected = panel.state.selected_file.as_ref().unwrap();
        let file = match selected {
            CommitPanelFileRef::Staged { index } => {
                assert!(staged);
                &panel.state.staged[*index]
            }
            CommitPanelFileRef::Unstaged { index } => {
                assert!(!staged);
                &panel.state.unstaged[*index]
            }
        };
        assert_eq!(
            file.path,
            Path::new(path),
            "visible file selection stays authoritative"
        );
    });
}

fn await_panel(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
    path: &str,
) -> Entity<CommitPanelView> {
    let repo = repo.canonicalize().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    loop {
        settle(cx, win);
        let panel = cx.read(|cx| {
            let app = app.read(cx);
            app.ui()
                .commit_panel
                .as_ref()
                .filter(|panel| {
                    let panel = panel.read(cx);
                    panel.repo_path == repo
                        && Some(panel.owner) == app.active_session()
                        && panel
                            .state
                            .unstaged
                            .iter()
                            .any(|file| file.path == Path::new(path))
                })
                .cloned()
        });
        if let Some(panel) = panel {
            assert_wip_context(cx, app, win, &repo);
            assert_eq!(
                cx.read(|cx| app
                    .read(cx)
                    .repo_path
                    .as_ref()
                    .map(|path| path.canonicalize().unwrap())),
                Some(repo),
            );
            return panel;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "visible Changes must load its active session's actual files"
        );
        cx.advance_clock(Duration::from_millis(100));
    }
}
