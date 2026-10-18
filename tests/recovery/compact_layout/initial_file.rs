//! #1662: the first Changes file activation must not depend on visiting WIP.
use super::*;
use kagi::ui::{
    commit_panel::CommitPanelFileRef,
    diff_view::{DiffRow, MainDiffSource},
};
use kagi_domain::diff::DiffLineKind;

const PATH: &str = "initial-file.txt";
const HEAD_TEXT: &str = "initial file HEAD content";
const INDEX_TEXT: &str = "initial file staged content";
const WORKTREE_TEXT: &str = "initial file unstaged content";

fn initial_fixture() -> tempfile::TempDir {
    // The shared compact fixture has 144 files and 64 extra commits. A single
    // tracked file with different HEAD/index/worktree bytes proves both real
    // diff boundaries without scrolling or any preselected/installed UI state.
    let fixture = build_fixture();
    let path = fixture.path().join(PATH);
    std::fs::write(&path, format!("{HEAD_TEXT}\n")).unwrap();
    git(fixture.path(), &["add", PATH]);
    git(fixture.path(), &["commit", "-q", "-m", "initial file baseline"]);
    std::fs::write(&path, format!("{INDEX_TEXT}\n")).unwrap();
    git(fixture.path(), &["add", PATH]);
    std::fs::write(&path, format!("{WORKTREE_TEXT}\n")).unwrap();
    fixture
}

pub fn scenario_compact_changes_initial_file(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = initial_fixture();
    // Pair presentation dimensions rather than multiplying identical input
    // coverage across a full matrix. Each side/input leg gets a FRESH root:
    // even the other section has never activated a file on that owner.
    for (case, key) in [
        (Case { height: 660., zoom: 1., locale: "en", preset: "apple-light" }, None),
        (Case { height: 660., zoom: 1.67, locale: "ja", preset: "apple-dark" }, None),
        (Case { height: 660., zoom: 1.67, locale: "en", preset: "apple-dark" }, Some("enter")),
        (Case { height: 660., zoom: 1., locale: "ja", preset: "apple-light" }, Some("space")),
    ] {
        for staged in [false, true] {
            e2e::commit_panel_labels::clear();
            let (app, win) = mount(cx, fixture.path(), case);
            cx.read(|cx| {
                let app = app.read(cx);
                assert!(!app.ui().commit_panel_open, "fresh root has not opened WIP");
                assert!(app.ui().main_diff.is_none(), "fresh root has no previous diff");
            });
            // mount only creates/draws the root. changes opens the actual bottom
            // tab (Cmd-J if needed); unlike open_wip/set_draft it never visits
            // the graph WIP or opens and dismisses the composer first.
            let panel = changes(cx, &app, win);
            columns(win, case.dimensions());
            assert_no_composer(cx, &app, win);
            let file = cx.read(|cx| {
                let state = &panel.read(cx).state;
                assert!(state.selected_file.is_none(), "no setup file activation");
                let files = if staged { &state.staged } else { &state.unstaged };
                let index = files.iter().position(|file| file.path == Path::new(PATH))
                    .expect("real status publishes initial file in this section");
                if staged {
                    CommitPanelFileRef::Staged { index }
                } else {
                    CommitPanelFileRef::Unstaged { index }
                }
            });
            let index = match &file {
                CommitPanelFileRef::Staged { index } | CommitPanelFileRef::Unstaged { index } => *index,
            };
            let row = format!("cp-{}-row-{index}", if staged { "st" } else { "us" });
            contained(
                bounds(win, if staged { "compact-changes-staged" } else { "compact-changes-unstaged" }),
                bounds(win, &row),
                "first activation targets a visible real Changes row",
            );
            if let Some(key) = key {
                // Focus the renderer's production RowList handle, not a click
                // that would itself request the diff before the keyboard leg.
                focus_control(cx, win, &row);
                assert_control_focus(cx, win, &row);
                assert!(cx.read(|cx| app.read(cx).ui().main_diff.is_none()));
                press_key(cx, win, key);
            } else {
                click(cx, win, &row);
            }
            assert_initial_diff(cx, &app, &panel, win, staged, &file);
            assert_no_composer(cx, &app, win);
            let pane = cx.read(|cx| app.read(cx).ui().main_diff.as_ref().unwrap().entity_id());

            // The wide adapter must not replace the owner or clear selection;
            // returning to actual compact Changes must keep the same visible
            // diff without another file activation or an intervening WIP visit.
            resize(cx, win, (desktop_width(), case.height));
            assert_initial_diff(cx, &app, &panel, win, staged, &file);
            resize(cx, win, case.dimensions());
            let returned = changes(cx, &app, win);
            assert_eq!(returned.entity_id(), panel.entity_id(), "resize retains file owner");
            assert_eq!(
                cx.read(|cx| app.read(cx).ui().main_diff.as_ref().unwrap().entity_id()),
                pane,
                "resize retains the displayed diff owner",
            );
            assert_initial_diff(cx, &app, &returned, win, staged, &file);
            assert_eq!(e2e::commit_panel_labels::accessible_selected(&row), Some(true));
            assert_no_composer(cx, &app, win);
            drop(returned);
            drop(panel);
            unmount(cx, app, win);
        }
    }
}

fn assert_no_composer(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) {
    assert!(e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "file activation must not open the composer");
    assert!(!cx.read(|cx| app.read(cx).ui().commit_panel_open),
        "Changes file activation must not depend on the desktop composer flag");
}

fn assert_initial_diff(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    staged: bool,
    file: &CommitPanelFileRef,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ui().commit_panel.as_ref().unwrap().entity_id(), panel.entity_id());
        assert_eq!(panel.read(cx).state.selected_file.as_ref(), Some(file));
        let pane = app.ui().main_diff.as_ref()
            .expect("first Changes activation must publish MainDiff without visiting WIP")
            .read(cx);
        assert!(pane.fit.control_bounds("main-diff-back").is_some(),
            "MainDiff must actually render, not merely retain an entity");
        assert_eq!(pane.view.title.as_ref(), PATH);
        match &pane.view.source {
            MainDiffSource::Staged { path } if staged => assert_eq!(path, Path::new(PATH)),
            MainDiffSource::Unstaged { path } if !staged => assert_eq!(path, Path::new(PATH)),
            _ => panic!("first Changes file opened the wrong diff source"),
        }
        let (removed, added, forbidden) = if staged {
            (HEAD_TEXT, INDEX_TEXT, WORKTREE_TEXT)
        } else {
            (INDEX_TEXT, WORKTREE_TEXT, HEAD_TEXT)
        };
        for (kind, expected) in [(DiffLineKind::Removed, removed), (DiffLineKind::Added, added)] {
            assert!(pane.view.rows.iter().any(|row| matches!(row,
                DiffRow::Line { kind: actual, text, .. }
                    if *actual == kind && text.as_ref() == expected)),
                "real diff must show {kind:?} {expected:?}");
        }
        assert!(!pane.view.rows.iter().any(|row| matches!(row,
            DiffRow::Line { text, .. } if text.as_ref() == forbidden)),
            "diff must not conflate HEAD/index/worktree content");
    });
    let title = bounds(win, "main-diff-title");
    assert!(title.size.width > px(0.) && title.size.height > px(0.),
        "correct diff title is visibly rendered");
}
