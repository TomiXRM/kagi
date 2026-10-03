//! The Graph sidebar's rows from the keyboard (#981), Tier A: ↑/↓ step a
//! pane's rows and never leave the pane; a collapsed pane's Tab stop is its
//! header, and Enter opens it onto its first row; Enter on a worktree row
//! opens its inspection card and Escape closes it, the row keeping the focus.
use std::path::PathBuf;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::sidebar::SidebarRow;
use kagi::ui::{e2e, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};

const LOCAL: usize = 0;
const WORKTREES: usize = 2;
const TAGS: usize = 3;

/// The sidebar row holding the focus, as of a fresh frame.
fn focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Option<(usize, String)> {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx).sidebar_row_focused_for_e2e(window)
    })
    .unwrap()
}

fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), name);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(window.window_id(), name).is_some()
}

pub fn scenario_sidebar_rows(cx: &mut VisualTestAppContext) {
    // Collapsing a pane is saved.
    let _saved = crate::gui_isolation::SavedKeys::keep(&["sidebar_panes"]);
    let fixture = build_fixture();
    let repo = fixture.path();
    for branch in ["alpha", "beta", "gamma"] {
        git(repo, &["branch", branch]);
    }
    git(repo, &["tag", "v1"]);
    git(repo, &["tag", "v2"]);
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("wt-one");
    git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wt-one",
            linked.to_str().unwrap(),
        ],
    );
    let (app, window) = mount(cx, repo);

    // ↑/↓ step LOCAL's rows and stop at its ends: never into REMOTE /
    // WORKTREES below it, nor out above it.
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(LOCAL, "branch:alpha", window, cx)
        });
    })
    .unwrap();
    assert_eq!(
        focused(cx, &app, window),
        Some((LOCAL, "branch:alpha".into()))
    );
    keys(cx, window, "down");
    let second = focused(cx, &app, window).expect("a row keeps the focus");
    assert_eq!(second.0, LOCAL);
    assert_ne!(second.1, "branch:alpha", "↓ moves to the next row");
    let mut last = second;
    for _ in 0..12 {
        keys(cx, window, "down");
        let now = focused(cx, &app, window).expect("a row keeps the focus");
        assert_eq!(now.0, LOCAL, "↓ stays in LOCAL ({now:?})");
        last = now;
    }
    assert_eq!(last.1, "branch:wt-one", "↓ stops at LOCAL's last row");
    for _ in 0..12 {
        keys(cx, window, "up");
        let now = focused(cx, &app, window).expect("a row keeps the focus");
        assert_eq!(now.0, LOCAL, "↑ stays in LOCAL ({now:?})");
        last = now;
    }
    assert_eq!(last.1, "branch:alpha", "↑ stops at LOCAL's first row");

    // TAGS collapsed: its rows are gone, and its header is its Tab stop.
    crate::app_conflict::click_control(cx, window, "tags");
    assert!(
        app.read_with(cx, |app, _| app.sidebar.collapsed.contains("tags")),
        "precondition: TAGS collapsed by its header"
    );
    let header = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            window.draw(cx).clear();
            (0..300).find_map(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                app.read(cx)
                    .sidebar_header_focused_for_e2e(window)
                    .filter(|&pane| pane == TAGS)
            })
        })
        .unwrap();
    assert_eq!(
        header,
        Some(TAGS),
        "Tab reaches the collapsed pane's header"
    );
    // Enter opens TAGS onto its first row.
    keys(cx, window, "enter");
    assert!(
        app.read_with(cx, |app, _| !app.sidebar.collapsed.contains("tags")),
        "Enter on the header opens TAGS"
    );
    assert_eq!(
        focused(cx, &app, window),
        Some((TAGS, "tag:v1".into())),
        "the opened pane's first row has the focus"
    );

    // Enter on a worktree row opens its inspection card; Escape closes it
    // and the row keeps the focus.
    let path: PathBuf = app.read_with(cx, |app, _| {
        app.sidebar
            .rows
            .iter()
            .find_map(|row| match row {
                SidebarRow::Worktree { path, .. } => Some(path.clone()),
                _ => None,
            })
            .expect("the linked worktree's row")
    });
    let key = format!("worktree:{}", path.display());
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(WORKTREES, &key, window, cx)
        });
    })
    .unwrap();
    assert_eq!(focused(cx, &app, window), Some((WORKTREES, key.clone())));
    assert!(!drawn(cx, window, "sidebar-worktree-keyboard-card"));
    keys(cx, window, "enter");
    assert_eq!(
        app.read_with(cx, |app, _| app.sidebar_keyboard_card_for_e2e()),
        Some(path.clone())
    );
    assert!(
        drawn(cx, window, "sidebar-worktree-keyboard-card"),
        "Enter opens the worktree's inspection card"
    );
    keys(cx, window, "escape");
    assert_eq!(
        app.read_with(cx, |app, _| app.sidebar_keyboard_card_for_e2e()),
        None
    );
    assert!(
        !drawn(cx, window, "sidebar-worktree-keyboard-card"),
        "Escape closes the card"
    );
    assert_eq!(
        focused(cx, &app, window),
        Some((WORKTREES, key)),
        "the row keeps the focus"
    );

    unmount(cx, app, window);
}
