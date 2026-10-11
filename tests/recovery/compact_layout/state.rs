//! Roundtrip ownership and real staged-commit evidence.
use super::*;
use kagi::ui::commit_panel::CommitPanelFileRef;
#[derive(Debug, PartialEq)]
struct PanelSnapshot {
    draft: (String, String, String),
    selected: Option<CommitPanelFileRef>,
    unstaged: Vec<std::path::PathBuf>,
    staged: Vec<std::path::PathBuf>,
}

fn snapshot(cx: &VisualTestAppContext, panel: &Entity<CommitPanelView>) -> PanelSnapshot {
    let (selected, mut unstaged, mut staged) = cx.read(|cx| {
        let state = &panel.read(cx).state;
        (
            state.selected_file.clone(),
            state
                .unstaged
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
            state
                .staged
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>(),
        )
    });
    unstaged.sort();
    staged.sort();
    PanelSnapshot {
        draft: draft(cx, panel),
        selected,
        unstaged,
        staged,
    }
}
fn select_row(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    staged: bool,
) -> std::path::PathBuf {
    let action = bounds(
        win,
        if staged {
            "cp-first-row-unstage"
        } else {
            "cp-first-row-stage"
        },
    );
    let column = bounds(
        win,
        if staged {
            "compact-changes-staged"
        } else {
            "compact-changes-unstaged"
        },
    );
    cx.simulate_click(
        win,
        point(column.left() + theme::scaled_px(40.), action.center().y),
        Modifiers::none(),
    );
    settle(cx, win);
    cx.read(|cx| {
        let state = &panel.read(cx).state;
        match state
            .selected_file
            .as_ref()
            .expect("real row click selects file")
        {
            CommitPanelFileRef::Staged { index } if staged => state.staged[*index].path.clone(),
            CommitPanelFileRef::Unstaged { index } if !staged => {
                state.unstaged[*index].path.clone()
            }
            selected => panic!("row selected wrong section: {selected:?}"),
        }
    })
}
pub fn scenario_compact_layout_state(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    matrix(|case| state_case(cx, case));
}

fn state_case(cx: &mut VisualTestAppContext, case: Case) {
    let fixture = fixture();
    e2e::commit_panel_labels::clear();
    let (app, win) = mount(cx, fixture.path(), case);
    let panel = changes(cx, &app, win);
    rail_graph(win, case.dimensions());
    wide_first_leg(cx, &app, &panel, win, case);
    stage_selection(cx, &app, &panel, win, fixture.path());
    rail_graph(win, case.dimensions());
    open_wip(cx, win);
    set_draft(cx, &panel, win);
    graph_selection(cx, &app, win);
    let before = snapshot(cx, &panel);
    roundtrip(cx, &app, &panel, win, fixture.path(), case, &before);
    if e2e::control_bounds(win.window_id(), "compact-composer").is_some() {
        escape(cx, &app, win);
    }
    let reopened = changes(cx, &app, win);
    assert_eq!(reopened.entity_id(), panel.entity_id());
    assert_eq!(
        snapshot(cx, &panel),
        before,
        "surface roundtrip retains panel state"
    );
    columns(win, case.dimensions());
    successful_commit(cx, &app, &panel, win, fixture.path(), &before);
    drop(reopened);
    drop(panel);
    unmount(cx, app, win);
}

fn wide_first_leg(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    case: Case,
) {
    resize(cx, win, (1392., case.height));
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    settle(cx, win);
    set_draft(cx, panel, win);
    let before = snapshot(cx, panel);
    let identities = input_ids(cx, panel);
    resize(cx, win, case.dimensions());
    popup_geometry(win, case.dimensions());
    assert_eq!(
        snapshot(cx, panel),
        before,
        "wide-first shrink keeps the original draft/state"
    );
    assert_eq!(
        input_ids(cx, panel),
        identities,
        "wide-first shrink keeps both inputs"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        panel.entity_id(),
        "wide-first shrink pins the same owner"
    );
    escape(cx, app, win);
}

fn graph_selection(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let graph = bounds(win, "compact-graph");
    let wip = bounds(win, "compact-wip-row");
    let target = point(
        graph.right() - theme::scaled_px(20.),
        wip.bottom() + wip.size.height / 2.,
    );
    assert!(
        target.x > bounds(win, "compact-composer").right(),
        "real Graph row lies outside pinned popup"
    );
    cx.simulate_click(win, target, Modifiers::none());
    settle(cx, win);
    bounds(win, "compact-composer");
    assert!(
        !cx.read(|cx| app.read(cx).ui().commit_panel_open),
        "real Graph selection closes legacy right-pane flag, not pinned compact composer"
    );
    coauthor_keys(cx, app, win);
}

fn coauthor_keys(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    use kagi_ui_core::i18n::Msg;
    let panel = cx.read(|cx| panel_for_app(app, cx));
    let preserved = draft(cx, &panel);
    focus_control(cx, win, "cp-coauthor");
    press_key(cx, win, "enter");
    assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
    let candidates = cx
        .read(|cx| panel.read(cx).coauthor_menu.clone())
        .expect("keyboard opens original co-author picker");
    assert_ax(
        "cp-coauthor-menu",
        gpui::Role::ListBox,
        Msg::AddCoAuthor.t(),
    );
    if candidates.is_empty() {
        assert_ax(
            "cp-coauthor-empty",
            gpui::Role::Status,
            Msg::NoRecentAuthors.t(),
        );
        press_key(cx, win, "down");
        assert_control_focus(cx, win, "cp-coauthor-empty");
    } else {
        for (key, index) in [("end", candidates.len() - 1), ("home", 0)] {
            press_key(cx, win, key);
            let id = format!("cp-coauthor-row-{index}");
            assert_control_focus(cx, win, &id);
            assert_ax(
                &id,
                gpui::Role::ListBoxOption,
                &format!("{} <{}>", candidates[index].name, candidates[index].email),
            );
            assert_eq!(
                e2e::commit_panel_labels::accessible_selected(&id),
                Some(true)
            );
        }
    }
    press_key(cx, win, "escape");
    assert!(cx.read(|cx| panel.read(cx).coauthor_menu.is_none()));
    assert_control_focus(cx, win, "cp-coauthor");
    assert_eq!(
        draft(cx, &panel),
        preserved,
        "picker navigation/dismissal keeps draft"
    );
    bounds(win, "compact-composer");
}

fn panel_for_app(app: &Entity<KagiApp>, cx: &gpui::App) -> Entity<CommitPanelView> {
    app.read(cx)
        .ui()
        .commit_panel
        .clone()
        .expect("original panel")
}

fn stage_selection(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    repo: &Path,
) {
    let before_index = index(repo);
    let path = select_row(cx, panel, win, false);
    click(cx, win, "cp-first-row-stage");
    assert_ne!(
        index(repo),
        before_index,
        "real Stage changes the fixture index"
    );
    cx.read(|cx| {
        let state = &panel.read(cx).state;
        assert!(
            state.staged.iter().any(|f| f.path == path),
            "clicked file is actually staged"
        );
        assert!(
            !state.unstaged.iter().any(|f| f.path == path),
            "staged untracked file leaves unstaged"
        );
    });
    select_row(cx, panel, win, true);
    // Dismiss the existing MainDiff, never replace its selected panel file.
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    settle(cx, win);
}

fn input_ids(
    cx: &VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
) -> (gpui::EntityId, gpui::EntityId) {
    cx.read(|cx| {
        let panel = panel.read(cx);
        (
            panel.title_input.as_ref().unwrap().entity_id(),
            panel.body_input.as_ref().unwrap().entity_id(),
        )
    })
}

fn roundtrip(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    repo: &Path,
    case: Case,
    before: &PanelSnapshot,
) {
    let staged_index = index(repo);
    let identities = input_ids(cx, panel);
    for dimensions in [(1392., case.height), case.dimensions()] {
        resize(cx, win, dimensions);
        if dimensions.0 == WIDTH {
            rail_graph(win, dimensions);
        } else {
            assert!(
                e2e::control_bounds(win.window_id(), "compact-rail").is_none(),
                "1392px is wide even at 167%: breakpoint is fixed logical 1000px"
            );
            contained(
                viewport(dimensions),
                bounds(win, "cp-commit-action"),
                "wide commit action remains reachable",
            );
        }
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
            panel.entity_id(),
            "940→1392→940 retains single commit-panel entity"
        );
        assert_eq!(
            &snapshot(cx, panel),
            before,
            "resize preserves title/body/draft, selection and staging"
        );
        assert_eq!(
            index(repo),
            staged_index,
            "resize never mutates Git staging"
        );
        assert_eq!(
            input_ids(cx, panel),
            identities,
            "resize retains both input entities"
        );
    }
}
fn git_text(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} reads real commit evidence"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}
fn successful_commit(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    repo: &Path,
    before: &PanelSnapshot,
) {
    let old_head = git_text(repo, &["rev-parse", "HEAD"]);
    assert!(
        !before.staged.is_empty() && !before.unstaged.is_empty(),
        "commit fixture retains WIP"
    );
    assert!(
        !git_text(repo, &["diff", "--cached", "--name-only"]).is_empty(),
        "real index has commit-ready staged content"
    );
    open_wip(cx, win);
    assert_eq!(
        draft(cx, panel),
        before.draft,
        "real Commit receives the retained roundtrip draft"
    );
    click(cx, win, "cp-commit-action");
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    loop {
        if git_text(repo, &["rev-parse", "HEAD"]) != old_head
            && git_text(repo, &["diff", "--cached", "--name-only"]).is_empty()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real Commit must advance HEAD and consume staging"
        );
        cx.advance_clock(Duration::from_millis(100));
        settle(cx, win);
    }
    settle(cx, win);
    assert_eq!(
        git_text(repo, &["rev-parse", "HEAD^"]),
        old_head,
        "exactly one real commit"
    );
    assert_eq!(
        git_text(repo, &["log", "-1", "--format=%B"]),
        before.draft.2,
        "Git commit contains the actual retained title/body message"
    );
    assert_eq!(
        git_text(repo, &["ls-files", "--others", "--exclude-standard"])
            .lines()
            .count(),
        before.unstaged.len(),
        "unstaged fixture files remain untouched"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "successful UI Commit dismisses popup"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "compact-popup-close").is_none(),
        "successful UI Commit retires close control"
    );
    bounds(win, "compact-wip-row");
    assert!(
        cx.read(|cx| app.read(cx).active_modal.is_none()),
        "successful Commit leaves no modal"
    );
}
