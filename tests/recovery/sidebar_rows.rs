//! The Graph sidebar's rows from the keyboard (#981), Tier A: ↑/↓ step a
//! pane's rows and never leave the pane; hiding the sidebar with a row
//! focused hands the focus to the window on that very frame; a pane with no
//! rows (empty, or collapsed) keeps a Tab stop on its header, and on a
//! collapsed one Enter opens it onto its first row; Enter on a worktree row
//! opens its inspection card and Escape closes it, the row keeping the focus.
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::sidebar::SidebarRow;
use kagi::ui::{e2e, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, open_offscreen, unmount};
use crate::pr_fields_focus::OfflineGh;

/// Tab from the window, drawing between steps, until `pane`'s header holds
/// the focus; `None` when a whole cycle passes it by.
fn tab_to_header(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) -> Option<usize> {
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().unwrap();
        root.focus(window, cx);
        window.draw(cx).clear();
        (0..300).find_map(|_| {
            window.focus_next(cx);
            window.draw(cx).clear();
            app.read(cx)
                .sidebar_header_focused_for_e2e(window)
                .filter(|&at| at == pane)
        })
    })
    .unwrap()
}

/// A sidebar too short to draw any pane's rows (#987 review): a 300px-high
/// window. LOCAL has rows, none drawn, so its header is its Tab stop.
pub fn scenario_sidebar_rows_short(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    crate::gui_evidence::fixture(fixture.path());
    let state = e2e::app_state(fixture.path()).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = open_offscreen(cx, size(px(1440.), px(300.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    });
    let app = captured.borrow().clone().expect("mounted KagiApp");
    let window: AnyWindowHandle = window.into();
    cx.run_until_parked();
    assert!(
        app.read_with(cx, |app, _| app
            .sidebar
            .rows
            .iter()
            .any(|row| matches!(row, SidebarRow::LocalBranchLeaf { .. }))),
        "precondition: LOCAL has rows"
    );
    assert_eq!(
        tab_to_header(cx, &app, window, LOCAL),
        Some(LOCAL),
        "Tab reaches LOCAL's header when none of its rows is drawn"
    );
    unmount(cx, app, window);
}

/// LOCAL scrolled after its Tab stop was picked (#987 review: a wheel, a
/// divider drag or a resize changes what the list draws). On that very frame
/// a row it draws is the stop — not the remembered row, scrolled out — and a
/// focused row scrolled out hands its focus to a drawn row on the next.
pub fn scenario_sidebar_rows_scroll(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    for index in 0..40 {
        git(repo, &["branch", &format!("b{index:02}")]);
    }
    let (app, window) = mount(cx, repo);
    let focus_row = |cx: &mut VisualTestAppContext, key: &str| {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.update(cx, |app, cx| {
                app.focus_sidebar_row_for_e2e(LOCAL, key, window, cx)
            });
            window.draw(cx).clear();
        })
        .unwrap();
    };
    let scroll_local = |cx: &mut VisualTestAppContext, item: usize| {
        app.read_with(cx, |app, _| {
            app.sidebar.scroll_handles[LOCAL].scroll_to_item_strict(item, gpui::ScrollStrategy::Top)
        });
    };

    // b00 remembered, then the window takes the focus and LOCAL scrolls to
    // its end: Tab over the one frame that scrolls reaches a drawn LOCAL row.
    focus_row(cx, "branch:b00");
    let reached = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            window.draw(cx).clear();
            app.read(cx).sidebar.scroll_handles[LOCAL]
                .scroll_to_item_strict(40, gpui::ScrollStrategy::Top);
            window.draw(cx).clear();
            (0..300).find_map(|_| {
                window.focus_next(cx);
                app.read(cx).sidebar_row_focused_for_e2e(window)
            })
        })
        .unwrap();
    let (pane, key) = reached.expect("Tab reaches a LOCAL row");
    assert_eq!(pane, LOCAL);
    assert_ne!(key, "branch:b00", "not the row scrolled out");

    // A focused row scrolled out: the next frame hands its focus on.
    scroll_local(cx, 0);
    focus_row(cx, "branch:b00");
    scroll_local(cx, 40);
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        window.draw(cx).clear();
    })
    .unwrap();
    let after = cx
        .update_window(window, |_, window, cx| {
            app.read(cx).sidebar_row_focused_for_e2e(window)
        })
        .unwrap();
    assert!(
        after
            .as_ref()
            .is_some_and(|(pane, key)| *pane == LOCAL && key != "branch:b00"),
        "the focus moved to a drawn LOCAL row ({after:?})"
    );
    unmount(cx, app, window);
}

/// The message of the stash row keyed `key` in STASHES, as the sidebar has
/// its rows now.
fn stash_message(app: &KagiApp, key: &str) -> Option<String> {
    let keys = app.sidebar_row_keys_for_e2e(STASHES);
    let item = keys.iter().position(|k| k == key)?;
    let start = app.sidebar.pane_ranges[STASHES].start + 1;
    match app.sidebar.rows.get(start + item)? {
        SidebarRow::Stash { message, .. } => Some(message.clone()),
        _ => None,
    }
}

/// Two #987 review cases. A focused stash row keeps the focus on its stash
/// when a new stash pushes it down (the key is the stash commit, not its
/// index). A pane header focused while the pane had no row hands the focus
/// to the row that appears (a refresh lands one).
pub fn scenario_sidebar_rows_keys(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("s.txt"), "base\n").unwrap();
    git(repo, &["add", "s.txt"]);
    git(repo, &["commit", "-qm", "s"]);
    std::fs::write(repo.join("s.txt"), "one\n").unwrap();
    git(repo, &["stash", "push", "-qm", "first"]);
    let (app, window) = mount(cx, repo);
    let draw = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
    };
    let refresh = |cx: &mut VisualTestAppContext| {
        app.update(cx, |app, cx| app.reload(cx));
        cx.run_until_parked();
        draw(cx);
        draw(cx);
    };

    // The stash row focused, then another stash pushed in front of it.
    let key = app.read_with(cx, |app, _| app.sidebar_row_keys_for_e2e(STASHES))[0].clone();
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(STASHES, &key, window, cx)
        });
    })
    .unwrap();
    assert_eq!(focused(cx, &app, window), Some((STASHES, key.clone())));
    std::fs::write(repo.join("s.txt"), "two\n").unwrap();
    git(repo, &["stash", "push", "-qm", "second"]);
    refresh(cx);
    let (pane, now) = focused(cx, &app, window).expect("a stash row keeps the focus");
    assert_eq!(pane, STASHES);
    let message = app.read_with(cx, |app, _| stash_message(app, &now));
    assert!(
        message.as_deref().is_some_and(|m| m.ends_with("first")),
        "the focus stays on the stash it was on ({message:?})"
    );

    // REMOTE empty, its header focused; a refresh brings a remote branch.
    assert_eq!(tab_to_header(cx, &app, window, REMOTE), Some(REMOTE));
    git(repo, &["update-ref", "refs/remotes/origin/feature", "HEAD"]);
    refresh(cx);
    assert_eq!(
        focused(cx, &app, window).map(|(pane, _)| pane),
        Some(REMOTE),
        "the header's focus moved to the row that appeared"
    );
    unmount(cx, app, window);
}

const LOCAL: usize = 0;
const REMOTE: usize = 1;
const WORKTREES: usize = 2;
const TAGS: usize = 3;
const STASHES: usize = 4;

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

    // The sidebar hidden with a row focused (#981 review): the frame that
    // stops drawing it may be the last one drawn, so on that frame the focus
    // goes to the window — it is not left on a row that is no longer there.
    let (row_after, root_after) = cx
        .update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("view.toggleSidebar", window, cx)
            });
            window.draw(cx).clear();
            let root = app.read(cx).root_focus.clone().unwrap();
            (
                app.read(cx).sidebar_row_focused_for_e2e(window),
                root.is_focused(window),
            )
        })
        .unwrap();
    assert_eq!(row_after, None, "no row keeps the focus once hidden");
    assert!(root_after, "the window has the focus on the hiding frame");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.toggleSidebar", window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();

    // REMOTE is open but empty (no remote): its header is its Tab stop, so
    // every pane keeps one (#981 review).
    assert!(
        app.read_with(cx, |app, _| !app.sidebar.collapsed.contains("remote")),
        "precondition: REMOTE is open"
    );
    let empty = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            window.draw(cx).clear();
            (0..300).find_map(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                app.read(cx)
                    .sidebar_header_focused_for_e2e(window)
                    .filter(|&pane| pane == REMOTE)
            })
        })
        .unwrap();
    assert_eq!(empty, Some(REMOTE), "Tab reaches the empty pane's header");

    // Issues in front, then Graph again (#987 review): the frame the panes
    // come back on is the only one drawn, and Tab over it, with no frame
    // after, still reaches a LOCAL row.
    let _gh = OfflineGh::with_script("#!/bin/sh\necho 'gh: offline' >&2\nexit 1\n");
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let back = cx
        .update_window(window, |_, window, cx| {
            // Back to Graph and its one frame, in one update: no other frame
            // may run in between.
            app.update(cx, |app, cx| app.show_graph_mode(cx));
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
            window.draw(cx).clear();
            (0..300).find_map(|_| {
                window.focus_next(cx);
                app.read(cx).sidebar_row_focused_for_e2e(window)
            })
        })
        .unwrap();
    assert_eq!(
        back.map(|(pane, _)| pane),
        Some(LOCAL),
        "Tab over the frame Graph comes back on reaches a LOCAL row"
    );

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
