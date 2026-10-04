//! The content panes of the two tab lists, to assistive technology (#979),
//! Tier A: Home's Repositories / Pull requests / Issues switch and the
//! workspace-mode nav (Graph / PRs / Issues) each name the pane under them a
//! tab panel after the selected tab; a takeover (Branch Cleanup), which
//! selects no tab, leaves the sidebar page no tab panel. (Editor replaces
//! the sidebar with its file tree, so it draws no page at all.)
use gpui::{AnyWindowHandle, Role, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::i18n::Msg;
use kagi::ui::tab_panel_a11y::{clear_recorded_tab_panels, recorded_tab_panel};
use kagi::ui::workspace_mode::WorkspaceMode;

use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

/// The label the next frame gives tab panel `id`, if it draws it — with the
/// tab panel role, or the test fails.
fn panel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> Option<String> {
    cx.update_window(window, |_, window, cx| {
        clear_recorded_tab_panels();
        window.draw(cx).clear();
    })
    .unwrap();
    recorded_tab_panel(id).map(|(role, label)| {
        assert_eq!(role, Role::TabPanel, "{id} is a tab panel");
        label
    })
}

pub fn scenario_tab_panels(cx: &mut VisualTestAppContext) {
    // No GitHub in Tier A: every gh call fails.
    let _gh = OfflineGh::with_script("#!/bin/sh\necho 'gh: offline' >&2\nexit 1\n");
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());

    // The workspace-mode nav: the sidebar page is the selected mode's.
    let modes = [
        (WorkspaceMode::Prs, Msg::WorkspacePrs),
        (WorkspaceMode::Issues, Msg::WorkspaceIssues),
        (WorkspaceMode::Graph, Msg::WorkspaceGraph),
    ];
    for (mode, name) in modes {
        app.update(cx, |app, cx| match mode {
            WorkspaceMode::Prs => app.show_pr_mode(cx),
            WorkspaceMode::Issues => app.show_issues_mode(cx),
            _ => app.show_graph_mode(cx),
        });
        cx.run_until_parked();
        assert_eq!(app.read_with(cx, |app, _| app.workspace_mode()), mode);
        assert_eq!(
            panel(cx, window, "sidebar-mode-panel").as_deref(),
            Some(name.t()),
            "the sidebar page is the {mode:?} tab's panel"
        );
    }
    // A takeover selects no cell of the nav, and the sidebar stays: its
    // page is no tab panel.
    app.update(cx, |app, cx| app.open_branch_cleanup_view(cx));
    cx.run_until_parked();
    assert_eq!(
        app.read_with(cx, |app, _| app.workspace_mode()),
        WorkspaceMode::Takeover
    );
    assert!(
        crate::home_github::drawn(cx, window, "sidebar-mode-nav"),
        "precondition: the sidebar is drawn over the takeover"
    );
    assert_eq!(
        panel(cx, window, "sidebar-mode-panel"),
        None,
        "no tab panel while no nav tab is selected"
    );
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    cx.run_until_parked();

    // Home: the pane under the switch is the selected pane's, named without
    // the count its tab shows.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    let panes = [
        (HomePane::Prs, Msg::HomePanePrs),
        (HomePane::Issues, Msg::HomePaneIssues),
        (HomePane::Repos, Msg::HomePaneRepos),
    ];
    for (pane, name) in panes {
        app.update(cx, |app, cx| app.set_home_pane(pane, cx));
        cx.run_until_parked();
        assert_eq!(
            panel(cx, window, "home-pane-panel").as_deref(),
            Some(name.t()),
            "Home's content is the {pane:?} tab's panel"
        );
    }

    unmount(cx, app, window);
}

/// The repository tab strip (#983): the workspace body is the active tab's
/// panel, named after it, and follows a switch; with Home in front, Home's
/// body is the Home cell's panel and the workspace body is not drawn.
pub fn scenario_repo_tab_panels(cx: &mut VisualTestAppContext) {
    let _gh = OfflineGh::with_script("#!/bin/sh\necho 'gh: offline' >&2\nexit 1\n");
    let first = build_fixture();
    let second = build_fixture();
    let (app, window) = mount(cx, first.path());
    let name = |cx: &mut VisualTestAppContext, i: usize| {
        app.read_with(cx, |app, _| app.tabs[i].name.clone())
    };
    assert_eq!(
        panel(cx, window, "repo-tab-panel"),
        Some(name(cx, 0)),
        "the body is the first tab's panel"
    );

    app.update(cx, |app, cx| {
        assert!(app.open_repository(second.path().to_path_buf(), cx))
    });
    cx.run_until_parked();
    assert_ne!(name(cx, 0), name(cx, 1), "precondition: two names");
    assert_eq!(
        panel(cx, window, "repo-tab-panel"),
        Some(name(cx, 1)),
        "the body follows to the opened tab"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    assert_eq!(
        panel(cx, window, "repo-tab-panel"),
        Some(name(cx, 0)),
        "the body follows the switch back"
    );

    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        panel(cx, window, "home-tab-panel").as_deref(),
        Some(Msg::HomeTabTitle.t()),
        "Home's body is the Home cell's panel"
    );
    assert_eq!(
        panel(cx, window, "repo-tab-panel"),
        None,
        "no repository panel while Home is in front"
    );

    unmount(cx, app, window);
}
