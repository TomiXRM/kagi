//! Long group identities, cardinality layout and targeted disclosure (#1305).
use gpui::VisualTestAppContext;
use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton};
use kagi::ui::list_a11y::{recorded_list, RecordedList};
use kagi::ui::sidebar::SidebarRow;
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n::Lang, theme};

use super::support::*;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::contained;

fn group_spec(tree: &RecordedList, identity: &str) -> (usize, Option<bool>, usize, usize) {
    let (ix, _) = tree
        .rows
        .iter()
        .find(|(_, (label, _))| label.contains(identity))
        .unwrap_or_else(|| panic!("full group identity {identity} missing: {:?}", tree.rows));
    tree.tree[ix]
}

fn assert_group_fit(window: AnyWindowHandle, key: &str, zoom: f32, count: usize) {
    let name = bounds(window, &format!("sidebar-group-{key}-name"));
    let count_box = bounds(window, &format!("sidebar-group-{key}-count"));
    let chevron = bounds(window, &format!("sidebar-group-{key}-chevron"));
    let sidebar = bounds(window, "worktree-sidebar");
    for (label, child) in [("name", name), ("count", count_box), ("chevron", chevron)] {
        contained(sidebar, child, label);
        assert!(
            child.size.width > px(0.) && child.size.height > px(0.),
            "{key}/{label} is visible"
        );
    }
    for (child, ellipsis) in [("name", true), ("count", false), ("chevron", false)] {
        let id = format!("sidebar-group-{key}-{child}");
        let paint = e2e::control_paint(window.window_id(), &id).expect("group child painted");
        assert_eq!(paint.ellipsis, ellipsis, "only group name truncates");
    }
    assert!(
        chevron.right() <= name.left() + px(1.),
        "chevron never overlaps name"
    );
    assert!(
        name.right() <= count_box.left() + px(1.),
        "count is a reserved trailing column, not clipped name text"
    );
    let expected = (28_f32).max(count.to_string().len() as f32 * 7. + 12.);
    assert!((f32::from(count_box.size.width) - f32::from(theme::scaled_px(expected))).abs() <= 1.);
    assert!(
        (f32::from(chevron.size.width) - f32::from(theme::scaled_px(10.))).abs() <= 1.,
        "fixed chevron at zoom {zoom}"
    );
}

fn group_toggle(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    target: (&str, &str),
    leaves: (&str, &str),
) {
    let (key, identity) = target;
    let (leaf, other_leaf) = leaves;
    let before = app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone());
    let at = bounds(window, &format!("sidebar-group-{key}-name")).center();
    cx.simulate_mouse_down(window, at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    paint(cx, app, window);
    let mut expected = before.clone();
    assert!(expected.insert(key.to_owned()));
    assert_eq!(
        app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone()),
        expected,
        "only clicked group toggles"
    );
    let tree = recorded_list("sidebar-remote").unwrap();
    assert_eq!(group_spec(&tree, identity).1, Some(false));
    assert!(!tree.rows.values().any(|(label, _)| label.contains(leaf)));
    assert!(
        tree.rows
            .values()
            .any(|(label, _)| label.contains(other_leaf)),
        "other group's leaf stays drawn"
    );
    let at = bounds(window, &format!("sidebar-group-{key}-chevron")).center();
    cx.simulate_mouse_down(window, at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    paint(cx, app, window);
    assert_eq!(
        app.read_with(cx, |app, _| app.ui().branch_groups_collapsed.clone()),
        before
    );
    assert_eq!(
        group_spec(&recorded_list("sidebar-remote").unwrap(), identity).1,
        Some(true)
    );
}

fn long_identity(stem: &str, suffix: &str) -> String {
    let name = format!("{}{suffix}", stem.repeat(19));
    assert_eq!(name.chars().count(), 200);
    name
}

fn show_group(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
    key: &str,
) {
    app.update(cx, |app, cx| {
        app.sidebar.pane_weights = [1; 5];
        app.sidebar.pane_weights[pane] = 100;
        let row = app
            .sidebar_row_keys_for_e2e(pane)
            .iter()
            .position(|row| row == &format!("group:{key}"))
            .expect("real group row");
        app.sidebar.scroll_handles[pane].scroll_to_item_strict(row, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    paint(cx, app, window);
}

/// Render cardinality boundaries on already cached real headers. This is
/// explicitly layout coverage, not a claim about Git-derived branch counts.
/// Keys, names, callbacks, sibling positions and real rows remain unchanged.
fn set_header_count(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    key: &str,
    count: usize,
) -> usize {
    app.update(cx, |app, cx| {
        let old = app
            .sidebar
            .rows
            .iter_mut()
            .find_map(|row| match row {
                SidebarRow::LocalGroupHeader {
                    key: at,
                    count: value,
                    ..
                }
                | SidebarRow::RemoteHeader {
                    key: at,
                    count: value,
                    ..
                }
                | SidebarRow::RemoteSubGroup {
                    key: at,
                    count: value,
                    ..
                } if at == key => Some(std::mem::replace(value, count)),
                _ => None,
            })
            .expect("existing group header");
        cx.notify();
        old
    })
}

fn keyboard_reveal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
    key: &str,
    identity: &str,
) {
    let focus_key = format!("group:{key}");
    focus(cx, app, window, pane, &focus_key);
    let before = app.read_with(cx, |app, _| {
        (app.ui().branch_groups_collapsed.clone(), app.ui().selected)
    });
    let id = format!("sidebar-group-{key}");
    let card = format!("{id}-reveal");
    e2e::sidebar_labels::clear();
    e2e::clear_control_bounds(window.window_id(), &card);
    keys(cx, window, "shift-f10");
    paint(cx, app, window);
    assert_eq!(
        e2e::sidebar_labels::recorded_tooltip(&id).as_deref(),
        Some(identity)
    );
    contained(
        Bounds::new(point(px(0.), px(0.)), size(px(940.), px(660.))),
        bounds(window, &card),
        "keyboard identity card",
    );
    assert_eq!(focused(cx, app, window), Some((pane, focus_key.clone())));
    app.read_with(cx, |app, _| {
        assert_eq!(
            (app.ui().branch_groups_collapsed.clone(), app.ui().selected),
            before
        );
        assert!(app.branch_menu.is_none() && app.tag_menu.is_none() && app.stash_menu.is_none());
    });
    e2e::clear_control_bounds(window.window_id(), &card);
    keys(cx, window, "escape");
    paint(cx, app, window);
    assert!(
        e2e::control_bounds(window.window_id(), &card).is_none(),
        "Escape removes keyboard reveal"
    );
    assert_eq!(focused(cx, app, window), Some((pane, focus_key)));
}

fn group_checks(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    target: (&str, &str, usize, usize),
    zoom: f32,
) {
    let (key, identity, pane, level) = target;
    show_group(cx, app, window, pane, key);
    let tree_id = if pane == 0 {
        "sidebar-local"
    } else {
        "sidebar-remote"
    };
    for count in [0, 1, 1000] {
        let old = set_header_count(cx, app, key, count);
        paint(cx, app, window);
        assert_group_fit(window, key, zoom, count);
        let tree = recorded_list(tree_id).unwrap();
        let spec = group_spec(&tree, identity);
        assert_eq!((spec.0, spec.1), (level, Some(true)));
        let label = &tree
            .rows
            .values()
            .find(|(label, _)| label.contains(identity))
            .unwrap()
            .0;
        assert!(
            label.contains(&count.to_string()),
            "AX retains boundary count: {label}"
        );
        set_header_count(cx, app, key, old);
        paint(cx, app, window);
    }
    let id = format!("sidebar-group-{key}");
    let tip = hover(
        cx,
        app,
        window,
        &id,
        bounds(window, &format!("{id}-name")).center(),
    );
    assert_eq!(
        tip, identity,
        "hover reveals full identity, including differing suffix"
    );
    keyboard_reveal(cx, app, window, pane, key, identity);
}

fn group_targets(
    remote: &str,
    other: &str,
    prefix: &str,
    other_prefix: &str,
) -> [(String, String, usize, usize); 6] {
    [
        (format!("local:{prefix}"), prefix.to_owned(), 0, 2),
        (
            format!("local:{other_prefix}"),
            other_prefix.to_owned(),
            0,
            2,
        ),
        (format!("remote:{remote}"), remote.to_owned(), 1, 2),
        (format!("remote:{other}"), other.to_owned(), 1, 2),
        (
            format!("remote:{remote}:{prefix}"),
            format!("{remote}/{prefix}"),
            1,
            3,
        ),
        (
            format!("remote:{remote}:{other_prefix}"),
            format!("{remote}/{other_prefix}"),
            1,
            3,
        ),
    ]
}

fn group_cell(cx: &mut VisualTestAppContext, language: Lang, slug: &str, zoom: f32) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote = long_identity("remote-id-", "suffix-one");
    let remote_other = long_identity("remote-id-", "suffix-two");
    let prefix = long_identity("prefix-id-", "suffix-one");
    let prefix_other = long_identity("prefix-id-", "suffix-two");
    let leaf = format!("{remote}/{prefix}/a");
    let sibling = format!("{remote}/{prefix_other}/b");
    let other_leaf = format!("{remote_other}/{prefix}/c");
    for name in [&leaf, &sibling, &other_leaf] {
        git(
            repo,
            &["update-ref", &format!("refs/remotes/{name}"), "HEAD"],
        );
    }
    for name in [format!("{prefix}/local"), format!("{prefix_other}/local")] {
        git(repo, &["branch", &name]);
    }
    let (app, window) = mount_dimensions(cx, repo, (940., 660.));
    appearance(cx, &app, window, language, slug, zoom, 1);
    let before = Witness::read(repo);
    let subgroup = format!("remote:{remote}:{prefix}");
    let groups = group_targets(&remote, &remote_other, &prefix, &prefix_other);
    for width in [120., 240., 340.] {
        app.update(cx, |app, cx| {
            app.sidebar.width = width;
            cx.notify();
        });
        paint(cx, &app, window);
        for (key, identity, pane, level) in &groups {
            group_checks(cx, &app, window, (key, identity, *pane, *level), zoom);
        }
        show_group(cx, &app, window, 1, &subgroup);
        group_toggle(
            cx,
            &app,
            window,
            (&subgroup, &format!("{remote}/{prefix}")),
            (&leaf, &sibling),
        );
        let remote_key = format!("remote:{remote}");
        show_group(cx, &app, window, 1, &remote_key);
        group_toggle(
            cx,
            &app,
            window,
            (&remote_key, &remote),
            (&sibling, &other_leaf),
        );
    }
    assert_eq!(
        Witness::read(repo),
        before,
        "group presentation and toggles only change UI state"
    );
    unmount(cx, app, window);
}

/// Runner registrations cover all eight appearances independently.
pub fn scenario_remote_group_header_fit_cell(
    cx: &mut VisualTestAppContext,
    language: Lang,
    slug: &str,
    zoom: f32,
) {
    let _settings = Settings::capture();
    group_cell(cx, language, slug, zoom);
}

pub fn scenario_remote_group_header_fit(cx: &mut VisualTestAppContext) {
    scenario_remote_group_header_fit_cell(cx, Lang::En, "apple-light", 1.);
}
