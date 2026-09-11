//! Pull/Push/Branch/Stash/Pop/Undo/Redo/Terminal belong to Graph. PRs, Editor and
//! Analyze must not draw them, and returning to Graph brings them back.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi::ui::workspace_mode::WorkspaceMode;

const REPO_ACTIONS: &str = "tb-repo-actions";

/// Draw one fresh frame and report whether it laid out the repo actions.
fn repo_actions_drawn(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> bool {
    cx.run_until_parked();
    let id = win.window_id();
    e2e::clear_control_bounds(id, REPO_ACTIONS);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(id, REPO_ACTIONS).is_some()
}

pub fn scenario_workspace_mode_toolbar(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount(cx, &repo);

    let steps: [(
        &str,
        fn(&mut kagi::ui::KagiApp, &mut gpui::Context<kagi::ui::KagiApp>),
    ); 5] = [
        ("graph", |_, _| {}),
        ("prs", |app, cx| app.show_pr_mode(cx)),
        ("editor", |app, cx| app.show_editor_mode(cx)),
        ("analyze", |app, cx| app.open_ecosystem_view(cx)),
        ("graph again", |app, cx| app.show_graph_mode(cx)),
    ];
    for (name, enter) in steps {
        app.update(cx, |app, cx| enter(app, cx));
        let mode = cx.read(|cx| app.read(cx).workspace_mode());
        let is_graph = mode == WorkspaceMode::Graph;
        assert_eq!(is_graph, name.starts_with("graph"), "{name}: mode {mode:?}");
        assert_eq!(
            repo_actions_drawn(cx, win),
            is_graph,
            "{name} ({mode:?}): repo actions drawn only in Graph"
        );
    }

    assert_eq!(before, repo_fingerprint(&repo), "repo mutated");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS workspace_mode_toolbar");
}
