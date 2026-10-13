//! #1449 / #1450: pane keyboard symmetry and filtered disclosure preservation.
#[path = "sidebar_identity/support.rs"]
mod support;

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, unmount};
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{list_a11y, settings, sidebar::SidebarRow, KagiApp};
use kagi_ui_core::{i18n::Lang, theme};
use support::{appearance, focus, focused, mount_dimensions, paint, Settings, Witness};

const SECTIONS: [&str; 5] = ["local", "remote", "worktrees", "tags", "stashes"];
const LISTS: [&str; 5] = [
    "sidebar-local",
    "sidebar-remote",
    "sidebar-worktrees",
    "sidebar-tags",
    "sidebar-stashes",
];

fn filter(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    cx.update_window(window, |_, window, cx| {
        let input = app.read(cx).sidebar.filter.clone().unwrap();
        input.update(cx, |input, cx| input.set_value(text, window, cx));
    })
    .unwrap();
    paint(cx, app, window);
}

fn tab_header(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) {
    let reached = cx
        .update_window(window, |_, window, cx| {
            app.read(cx).root_focus.clone().unwrap().focus(window, cx);
            (0..300).any(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                app.read(cx).sidebar_header_focused_for_e2e(window) == Some(pane)
            })
        })
        .unwrap();
    assert!(
        reached,
        "pane {pane} header must be keyboard reachable when no row is drawn"
    );
}

fn assert_header(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) {
    let header = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).sidebar_header_focused_for_e2e(window)
        })
        .unwrap();
    assert_eq!(
        header,
        Some(pane),
        "collapse returns focus to the remaining header"
    );
    assert_eq!(focused(cx, app, window), None, "no undrawn row keeps focus");
}

fn layout(cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> settings::SidebarPaneLayout {
    app.read_with(cx, |app, _| settings::SidebarPaneLayout {
        weights: app.sidebar.pane_weights,
        collapsed_mask: SECTIONS.iter().enumerate().fold(0, |mask, (i, section)| {
            mask | if app.sidebar.collapsed.contains(section) {
                1 << i
            } else {
                0
            }
        }),
    })
}

fn assert_saved(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    expected_mask: u8,
    weights: [u16; 5],
) {
    let expected = settings::SidebarPaneLayout {
        weights,
        collapsed_mask: expected_mask,
    };
    assert_eq!(
        layout(cx, app),
        expected,
        "only the selected section bit changes; weights are untouched"
    );
    assert_eq!(
        settings::Settings::load().sidebar_pane_layout(),
        expected,
        "the same pane layout is persisted"
    );
}

fn populated(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) {
    let rows = app.read_with(cx, |app, _| app.sidebar_row_keys_for_e2e(pane));
    assert!(!rows.is_empty(), "pane {pane} fixture has rows");
    // Each expanded pane still has one roving row stop, not an additional header stop.
    let stops = cx
        .update_window(window, |_, window, cx| {
            app.read(cx).root_focus.clone().unwrap().focus(window, cx);
            let mut rows_seen = std::collections::HashSet::new();
            let mut first = None;
            let mut cycled = false;
            for _ in 0..300 {
                window.focus_next(cx);
                window.draw(cx).clear();
                let current = window.focused(cx);
                if first.as_ref() == Some(&current) {
                    cycled = true;
                    break;
                }
                if first.is_none() {
                    first = Some(current);
                }
                assert_ne!(
                    app.read(cx).sidebar_header_focused_for_e2e(window),
                    Some(pane)
                );
                if let Some((at, key)) = app.read(cx).sidebar_row_focused_for_e2e(window) {
                    if at == pane {
                        rows_seen.insert(key);
                    }
                }
            }
            assert!(
                cycled,
                "inspect one complete Tab cycle without redundant redraws"
            );
            rows_seen
        })
        .unwrap();
    assert_eq!(
        stops.len(),
        1,
        "one roving row Tab stop for expanded pane {pane}"
    );
    focus(cx, app, window, pane, &rows[0]);
    keys(cx, window, "left");
    paint(cx, app, window);
    assert!(
        app.read_with(cx, |app, _| app.sidebar.collapsed.contains(SECTIONS[pane])),
        "Left on a pane row must collapse its expanded pane"
    );
    assert_header(cx, app, window, pane);
}

fn pane_cycle(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) {
    let weights = layout(cx, app).weights;
    populated(cx, app, window, pane);
    assert_saved(cx, app, 1 << pane, weights);
    keys(cx, window, "enter");
    paint(cx, app, window);
    assert_saved(cx, app, 0, weights);
    assert_eq!(focused(cx, app, window).map(|(at, _)| at), Some(pane));
    keys(cx, window, "left");
    paint(cx, app, window);
    assert_saved(cx, app, 1 << pane, weights);
    assert_header(cx, app, window, pane);
    keys(cx, window, "space");
    paint(cx, app, window);
    assert_saved(cx, app, 0, weights);
    // An expanded empty pane uses the same Enter/Space toggle.
    filter(cx, app, window, "no-matching-sidebar-entry");
    tab_header(cx, app, window, pane);
    keys(cx, window, "enter");
    paint(cx, app, window);
    assert_saved(cx, app, 1 << pane, weights);
    assert_header(cx, app, window, pane);
    keys(cx, window, "space");
    paint(cx, app, window);
    assert_saved(cx, app, 0, weights);
    assert_header(cx, app, window, pane);
    filter(cx, app, window, "");
}

pub fn scenario_sidebar_keyboard_collapse(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "topic/one"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/topic/one", "HEAD"],
    );
    git(fixture.path(), &["tag", "v1"]);
    let linked = tempfile::tempdir().unwrap();
    let worktree = linked.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            worktree.to_str().unwrap(),
        ],
    );
    std::fs::write(fixture.path().join("README.md"), "stash fixture\n").unwrap();
    git(fixture.path(), &["stash", "push", "-m", "pane fixture"]);
    let before = Witness::read(fixture.path());
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for language in [Lang::En, Lang::Ja] {
                let (app, window) = mount_dimensions(cx, fixture.path(), dimensions);
                for pane in 0..5 {
                    appearance(cx, &app, window, language, "apple-light", zoom, pane);
                    pane_cycle(cx, &app, window, pane);
                }
                // Persist a nonzero mask, then recreate the actual window-owned state.
                populated(cx, &app, window, 4);
                let saved = layout(cx, &app);
                unmount(cx, app, window);
                let (restored, window) = mount_dimensions(cx, fixture.path(), dimensions);
                assert_eq!(
                    layout(cx, &restored),
                    saved,
                    "remount restores mask and weights"
                );
                unmount(cx, restored, window);
            }
        }
    }
    assert_eq!(
        Witness::read(fixture.path()),
        before,
        "keyboard presentation causes zero Git writes"
    );
}

fn group_state(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    key: &str,
    pane: usize,
) {
    paint(cx, app, window);
    let (index, expanded) = app.read_with(cx, |app, _| {
        app.sidebar
            .rows
            .iter()
            .enumerate()
            .find_map(|(index, row)| match row {
                SidebarRow::LocalGroupHeader {
                    key: found,
                    collapsed,
                    ..
                }
                | SidebarRow::RemoteHeader {
                    key: found,
                    collapsed,
                    ..
                }
                | SidebarRow::RemoteSubGroup {
                    key: found,
                    collapsed,
                    ..
                } if found == key => {
                    Some((index - app.sidebar.pane_ranges[pane].start, !collapsed))
                }
                _ => None,
            })
            .expect("matching group stays in filter results")
    });
    assert!(expanded, "filter auto-expansion keeps chevron expanded");
    let recorded = list_a11y::recorded_list(LISTS[pane]).unwrap();
    assert_eq!(
        recorded.tree.get(&index).map(|spec| spec.1),
        Some(Some(true)),
        "AX expanded matches visible chevron"
    );
    assert!(
        app.read_with(cx, |app, _| app
            .sidebar
            .rows
            .iter()
            .any(|row| matches!(row,
        SidebarRow::LocalBranchLeaf { name, .. } if name == "topic/one"))),
        "matching local child remains visible"
    );
    assert!(
        app.read_with(cx, |app, _| app
            .sidebar
            .rows
            .iter()
            .any(|row| matches!(row,
        SidebarRow::RemoteLeaf { display, .. } if display == "origin/topic/one"))),
        "matching remote child remains visible"
    );
}

fn filtered_activation(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    group: (usize, &str),
    keyboard: bool,
    activation: &str,
    language: Lang,
) {
    let (pane, key) = group;
    group_state(cx, app, window, key, pane);
    if keyboard {
        focus(cx, app, window, pane, &format!("group:{key}"));
        keys(cx, window, activation);
    } else {
        let bounds = support::bounds(window, &format!("sidebar-group-{key}"));
        cx.simulate_click(window, bounds.center(), Modifiers::none());
    }
    group_state(cx, app, window, key, pane);
    let expected = if language == Lang::Ja {
        "絞り込み中はグループを展開したままにします。グループを折りたたむには絞り込みを解除してください。"
    } else {
        "Groups stay expanded while filtering. Clear the filter to collapse a group."
    };
    app.read_with(cx, |app, cx| {
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        assert_eq!(
            toast.message.as_ref(),
            expected,
            "pointer and keyboard report a concrete localized reason"
        );
    });
}

fn normal_group_collapsed(app: &KagiApp, key: &str) -> bool {
    app.sidebar
        .rows
        .iter()
        .find_map(|row| match row {
            SidebarRow::LocalGroupHeader {
                key: found,
                collapsed,
                ..
            }
            | SidebarRow::RemoteHeader {
                key: found,
                collapsed,
                ..
            }
            | SidebarRow::RemoteSubGroup {
                key: found,
                collapsed,
                ..
            } if found == key => Some(*collapsed),
            _ => None,
        })
        .expect("group survives clearing filter")
}

fn filtered_group(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    group: (usize, &str),
    initially_closed: bool,
    language: Lang,
) {
    let (pane, key) = group;
    app.update(cx, |app, cx| {
        let ui = app.ui_mut().unwrap();
        ui.branch_groups_collapsed.clear();
        if initially_closed {
            ui.branch_groups_collapsed.insert(key.to_owned());
        }
        cx.notify();
    });
    let normal = app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone());
    filter(cx, app, window, "topic");
    let id = format!("sidebar-group-{key}");
    let at = support::bounds(window, &id).center();
    let full_name = match key {
        "local:topic" => "topic",
        "remote:origin" => "origin",
        _ => "origin/topic",
    };
    assert_eq!(
        support::hover(cx, app, window, &id, at),
        full_name,
        "filter preserves full group identity reveal"
    );
    for keyboard in [false, true] {
        for activation in ["enter", "space"] {
            filtered_activation(cx, app, window, (pane, key), keyboard, activation, language);
            assert_eq!(
                app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone()),
                normal,
                "filtered disclosure must not silently toggle normal collapse choices"
            );
        }
    }
    filter(cx, app, window, "");
    assert_eq!(
        app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone()),
        normal,
        "clearing filter restores the unchanged normal choice"
    );
    assert_eq!(
        app.read_with(cx, |app, _| normal_group_collapsed(app, key)),
        initially_closed
    );
}

pub fn scenario_sidebar_filtered_disclosure(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "topic/one"]);
    git(fixture.path(), &["branch", "topic/two"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/topic/one", "HEAD"],
    );
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/topic/two", "HEAD"],
    );
    let before = Witness::read(fixture.path());
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for language in [Lang::En, Lang::Ja] {
                let (app, window) = mount_dimensions(cx, fixture.path(), dimensions);
                for (pane, key) in [
                    (0, "local:topic"),
                    (1, "remote:origin"),
                    (1, "remote:origin:topic"),
                ] {
                    for initially_closed in [false, true] {
                        appearance(cx, &app, window, language, "apple-light", zoom, pane);
                        filtered_group(cx, &app, window, (pane, key), initially_closed, language);
                    }
                }
                unmount(cx, app, window);
            }
        }
    }
    assert_eq!(
        Witness::read(fixture.path()),
        before,
        "filter disclosure causes zero Git writes"
    );
    theme::set_zoom(1.);
}
