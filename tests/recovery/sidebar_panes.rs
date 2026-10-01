//! #864: actual Graph sidebar pointer/geometry, row density and settings
//! persistence for the five panes LOCAL, REMOTE, WORKTREES, TAGS, STASHES.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton, Pixels,
    ScrollStrategy, VisualTestAppContext,
};
use kagi::ui::sidebar::SidebarRow;
use kagi::ui::{e2e, list_a11y, settings, theme, KagiApp};

use crate::macos::{build_fixture, git, mount, open_offscreen, unmount};
use crate::recovery_operations::paint;

/// Pane order, matching the Tree IDs and `sidebar_panes` weight indices.
const PANES: [&str; 5] = [
    "sidebar-local",
    "sidebar-remote",
    "sidebar-worktrees",
    "sidebar-tags",
    "sidebar-stashes",
];
/// Section header hitboxes, in the same order.
const HEADERS: [&str; 5] = ["local", "remote", "worktrees", "tags", "stashes"];
const LOCAL: usize = 0;
const REMOTE: usize = 1;
const WORKTREES: usize = 2;
/// Unscaled uniform slots: section headers, group headings and ordinary
/// leaves share one row; the worktree-only list is taller.
const ROW: f32 = 20.;
const WORKTREE_ROW: f32 = 24.;
/// The unreleased six-pane encoding (PR first). The five-pane parser must
/// reject it, so the app falls back to defaults and leaves the raw value.
const SIX_PANE_V1: &str = "v1:1200,2600,2200,1800,1100,1100:0";

fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    height: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    if height == 900. {
        return mount(cx, repo);
    }
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = open_offscreen(cx, size(px(1440.), px(height)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    });
    let app = captured.borrow().clone().expect("mounted KagiApp");
    cx.run_until_parked();
    (app, window.into())
}

fn bounds(cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> [(f32, f32); 5] {
    cx.read(|cx| {
        let sidebar = &app.read(cx).sidebar;
        std::array::from_fn(|index| sidebar.pane_geom[index].get())
    })
}

fn height(pane: (f32, f32)) -> f32 {
    pane.1 - pane.0
}

fn redraw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    list_a11y::clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn disk_layout() -> String {
    let path = settings::settings_path().expect("settings file path");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    json["sidebar_panes"]
        .as_str()
        .expect("flat string value")
        .to_string()
}

fn saved_layout() -> settings::SidebarPaneLayout {
    settings::SidebarPaneLayout::parse(&settings::read_setting("sidebar_panes").unwrap())
        .expect("a five-pane layout was written")
}

fn pane_weights(cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> [u16; 5] {
    cx.read(|cx| app.read(cx).sidebar.pane_weights)
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 0.5
}

fn top(bounds: Bounds<Pixels>) -> f32 {
    f32::from(bounds.origin.y)
}

fn tall(bounds: Bounds<Pixels>) -> f32 {
    f32::from(bounds.size.height)
}

fn click_header(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
) {
    let header = e2e::control_bounds(window.window_id(), HEADERS[pane])
        .unwrap_or_else(|| panic!("{} header hitbox", HEADERS[pane]));
    cx.simulate_click(window, header.center(), Modifiers::none());
    redraw(cx, app, window);
}

/// Measured section header heights, cleared first so each is this frame's.
fn header_heights(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> [f32; 5] {
    for header in HEADERS {
        e2e::clear_control_bounds(window.window_id(), header);
    }
    redraw(cx, app, window);
    HEADERS.map(|header| {
        tall(
            e2e::control_bounds(window.window_id(), header)
                .unwrap_or_else(|| panic!("{header} header laid out")),
        )
    })
}

/// Real rows in the LOCAL and WORKTREES lists have exactly their pane's
/// uniform slot: the pinned header, a group heading, two leaves and two
/// worktree rows are measured after placing the group heading at the top of
/// LOCAL's list and the first worktree at the top of WORKTREES' list.
fn assert_row_density(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    zoom: f32,
) {
    let (leaves, worktrees) = app.update(cx, |app, cx| {
        let sidebar = &app.sidebar;
        let rows = &sidebar.rows;
        let first = rows
            .iter()
            .position(|row| {
                matches!(row, SidebarRow::LocalBranchLeaf { name, .. } if name == "scroll/00")
            })
            .expect("scroll/00 local leaf");
        let group = first - 1;
        assert!(
            matches!(rows[group], SidebarRow::LocalGroupHeader { .. }),
            "scroll/ leaves follow their group heading: {:?}",
            rows[group]
        );
        let leaf = |index: usize| match &rows[index] {
            SidebarRow::LocalBranchLeaf { name, .. } => name.clone(),
            other => panic!("expected a local leaf, got {other:?}"),
        };
        let leaves = [leaf(first), leaf(first + 1)];
        sidebar.scroll_handles[LOCAL].scroll_to_item_strict(
            group - (sidebar.pane_ranges[LOCAL].start + 1),
            ScrollStrategy::Top,
        );
        let worktrees: Vec<String> = rows[sidebar.pane_ranges[WORKTREES].clone()]
            .iter()
            .filter_map(|row| match row {
                SidebarRow::Worktree { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        sidebar.scroll_handles[WORKTREES].scroll_to_item_strict(0, ScrollStrategy::Top);
        cx.notify();
        (leaves, worktrees)
    });
    assert!(
        worktrees.len() >= 2,
        "two linked worktree rows, with the main worktree hidden: {worktrees:?}"
    );
    let main_name = cx.read(|cx| {
        app.read(cx)
            .view()
            .worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .expect("main worktree remains in the read model")
            .name
            .clone()
    });
    assert!(
        !worktrees.contains(&main_name),
        "main worktree got a sidebar row"
    );
    let id = window.window_id();
    let names = [
        HEADERS[LOCAL].to_string(),
        format!("sidebar-local-{}", leaves[0]),
        format!("sidebar-local-{}", leaves[1]),
        HEADERS[WORKTREES].to_string(),
        format!("sidebar-worktree-{}", worktrees[0]),
        format!("sidebar-worktree-{}", worktrees[1]),
    ];
    for name in &names {
        e2e::clear_control_bounds(id, name);
    }
    redraw(cx, app, window);
    let panes = bounds(cx, app);
    let [header, first, second, wt_header, wt_first, wt_second] = names.clone().map(|name| {
        e2e::control_bounds(id, &name)
            .unwrap_or_else(|| panic!("{name} laid out at {zoom}x; panes {panes:?}"))
    });
    let row = ROW * zoom;
    let wt_row = WORKTREE_ROW * zoom;
    for (name, measured) in [(&names[0], header), (&names[3], wt_header)] {
        assert!(
            near(tall(measured), row),
            "{zoom}x {name} header is one {row}px row: {measured:?}"
        );
    }
    assert!(
        near(tall(first), row) && near(tall(second), row),
        "{zoom}x LOCAL leaves fill their {row}px slot: {first:?} {second:?}"
    );
    assert!(
        near(top(second) - top(first), row),
        "{zoom}x LOCAL uniform pitch is {row}px: {first:?} {second:?}"
    );
    assert!(
        near(top(first) - (top(header) + tall(header)), row),
        "{zoom}x the group heading takes one {row}px slot under the pinned header: header={header:?} leaf={first:?}"
    );
    assert!(
        near(tall(wt_first), wt_row) && near(tall(wt_second), wt_row),
        "{zoom}x worktree rows are {wt_row}px: {wt_first:?} {wt_second:?}"
    );
    assert!(
        near(top(wt_second) - top(wt_first), wt_row),
        "{zoom}x WORKTREES uniform pitch is {wt_row}px: {wt_first:?} {wt_second:?}"
    );
    assert!(
        near(top(wt_first), top(wt_header) + tall(wt_header)),
        "{zoom}x the first worktree row starts under its header: {wt_header:?} {wt_first:?}"
    );
}

/// Press on a divider, then move it to each offset in `steps` (pixels below
/// the press), painting after each move. The first move starts the drag;
/// `after_move` sees the pointer of every later move.
fn drag_divider(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    divider: &str,
    steps: &[f32],
    mut after_move: impl FnMut(gpui::Point<Pixels>),
) {
    let center = e2e::control_bounds(window.window_id(), divider)
        .unwrap_or_else(|| panic!("{divider} hitbox"))
        .center();
    cx.simulate_mouse_move(window, center, None, Modifiers::none());
    cx.simulate_mouse_down(window, center, MouseButton::Left, Modifiers::none());
    let mut pointer = center;
    for (index, &step) in steps.iter().enumerate() {
        pointer = point(center.x, center.y + px(step));
        cx.simulate_mouse_move(window, pointer, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        paint(cx, window);
        if index > 0 {
            after_move(pointer);
        }
    }
    cx.simulate_mouse_up(window, pointer, MouseButton::Left, Modifiers::none());
    redraw(cx, app, window);
}

pub fn scenario_sidebar_panes(cx: &mut VisualTestAppContext) {
    // #899 guard: the scenario saves and corrupts `sidebar_panes` and zooms.
    let _saved = crate::gui_isolation::SavedKeys::keep(&["sidebar_panes", "ui_zoom"]);
    let fixture = build_fixture();
    let repo = fixture.path();
    let previous_setting = settings::read_setting("sidebar_panes");
    let previous_zoom = theme::zoom();
    theme::set_zoom(1.);
    assert!(
        settings::SidebarPaneLayout::parse(SIX_PANE_V1).is_none(),
        "the six-pane v1 value is not a five-pane layout"
    );
    settings::write_setting("sidebar_panes", Some(SIX_PANE_V1));
    settings::flush();
    // Both lists contain enough rows to scroll separately even after the
    // bottom panel and all five pinned headers have taken their space.
    let git2_repo = git2::Repository::open(repo).unwrap();
    let head = git2_repo.head().unwrap().peel_to_commit().unwrap();
    for index in 0..48 {
        git2_repo
            .branch(&format!("scroll/{index:02}"), &head, false)
            .unwrap();
        git2_repo
            .reference(
                &format!("refs/remotes/origin/scroll/{index:02}"),
                head.id(),
                false,
                "fixture",
            )
            .unwrap();
    }
    drop(head);
    drop(git2_repo);
    // Two linked worktree rows give WORKTREES a measurable uniform pitch
    // without displaying the main worktree.
    let linked_root = tempfile::tempdir().unwrap();
    for name in ["wt-density", "wt-density-second"] {
        let linked = linked_root.path().join(name);
        git(
            repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                name,
                linked.to_str().unwrap(),
            ],
        );
    }

    let (app, window) = mount_at(cx, repo, 900.);
    redraw(cx, &app, window);
    assert_eq!(
        disk_layout(),
        SIX_PANE_V1,
        "mount/draw must not repair settings"
    );
    let defaults = settings::SidebarPaneLayout::default().weights;
    assert_eq!(defaults, [3500, 2500, 2000, 1000, 1000]);
    assert_eq!(
        pane_weights(cx, &app),
        defaults,
        "a six-pane value falls back to the five-pane defaults"
    );
    assert!(cx.read(|cx| app.read(cx).sidebar.collapsed.is_empty()));
    for pane in PANES {
        let tree = list_a11y::recorded_list(pane).unwrap_or_else(|| panic!("{pane} Tree drawn"));
        assert_eq!(tree.role, Some(gpui::Role::Tree), "{pane}");
    }
    assert!(
        list_a11y::recorded_list("sidebar-prs").is_none(),
        "the Graph sidebar has no PULL REQUESTS pane"
    );
    let initial = bounds(cx, &app);
    for (index, &pane) in initial.iter().enumerate() {
        assert!(
            height(pane) >= ROW - 0.5,
            "pane {index} header fits: {initial:?}"
        );
        if index > 0 {
            assert!(pane.0 >= initial[index - 1].1, "pane order: {initial:?}");
        }
    }

    // Measured density at 100% and 125% in the 900px window. Changing zoom
    // is not a layout edit and still leaves the six-pane value verbatim.
    assert_row_density(cx, &app, window, 1.);
    theme::set_zoom(1.25);
    redraw(cx, &app, window);
    assert_row_density(cx, &app, window, 1.25);
    theme::set_zoom(1.);
    redraw(cx, &app, window);
    settings::flush();
    assert_eq!(disk_layout(), SIX_PANE_V1, "zoom/scroll must not repair");

    // The first actual header click (LOCAL) commits the default weights plus
    // the new collapse mask, replacing the six-pane value; LOCAL is bit 1.
    click_header(cx, &app, window, LOCAL);
    settings::flush();
    let repaired = settings::SidebarPaneLayout::parse(&disk_layout())
        .expect("first explicit edit writes a five-pane layout");
    assert_eq!(repaired.weights, defaults);
    assert_eq!(repaired.collapsed_mask, 1);
    assert!(
        near(height(bounds(cx, &app)[LOCAL]), ROW),
        "collapsed LOCAL keeps only its header: {:?}",
        bounds(cx, &app)
    );
    click_header(cx, &app, window, LOCAL);
    assert_eq!(saved_layout().collapsed_mask, 0);
    let initial = bounds(cx, &app);

    // LOCAL and REMOTE have substantially different whole-pane heights. Real
    // pointer down/move/up on their divider: the separator must track the
    // first small drag, not jump to a header-subtracted ratio when the flex
    // weights size entire panes, and keep tracking afterwards.
    assert!(
        height(initial[LOCAL]) - height(initial[REMOTE]) > 24.,
        "unequal LOCAL/REMOTE panes required: {initial:?}"
    );
    let divider = "sidebar-local-divider";
    drag_divider(cx, &app, window, divider, &[8., 12., 32.], |pointer| {
        let separator = e2e::control_bounds(window.window_id(), divider).expect("moved divider");
        let error = (f32::from(separator.center().y) - f32::from(pointer.y)).abs();
        assert!(
            error <= 2.,
            "LOCAL/REMOTE separator stays under the pointer: {initial:?}, target={pointer:?}, actual={separator:?}, error={error}"
        );
    });
    let after_drag = bounds(cx, &app);
    let weights_after = pane_weights(cx, &app);
    assert!(
        height(after_drag[LOCAL]) > height(initial[LOCAL]) + 8.,
        "local grows after drag: {initial:?} -> {after_drag:?}"
    );
    assert!(
        height(after_drag[REMOTE]) < height(initial[REMOTE]) - 8.,
        "remote shrinks after drag: {initial:?} -> {after_drag:?}"
    );
    assert_ne!(weights_after[LOCAL], defaults[LOCAL]);
    assert_eq!(
        u32::from(weights_after[LOCAL]) + u32::from(weights_after[REMOTE]),
        u32::from(defaults[LOCAL]) + u32::from(defaults[REMOTE]),
        "the drag only redistributes the LOCAL/REMOTE pair"
    );
    assert_eq!(weights_after[WORKTREES..], defaults[WORKTREES..]);
    settings::flush();
    assert_eq!(
        settings::SidebarPaneLayout::parse(&disk_layout())
            .unwrap()
            .weights,
        weights_after
    );
    let weights = weights_after;

    // Scroll only LOCAL: REMOTE's drawn rows stay put; then only REMOTE.
    let remote_before = list_a11y::recorded_list(PANES[REMOTE]).unwrap().rows;
    app.update(cx, |app, cx| {
        app.sidebar.scroll_handles[LOCAL].scroll_to_item(42, ScrollStrategy::Center);
        cx.notify();
    });
    redraw(cx, &app, window);
    let local = list_a11y::recorded_list(PANES[LOCAL]).unwrap();
    assert!(
        local.rows.keys().any(|&position| position > 30),
        "local scroll reached distant refs: {local:?}"
    );
    assert_eq!(
        list_a11y::recorded_list(PANES[REMOTE]).unwrap().rows,
        remote_before
    );
    let local_before = local.rows;
    app.update(cx, |app, cx| {
        app.sidebar.scroll_handles[REMOTE].scroll_to_item(42, ScrollStrategy::Center);
        cx.notify();
    });
    redraw(cx, &app, window);
    let remote = list_a11y::recorded_list(PANES[REMOTE]).unwrap();
    assert!(
        remote.rows.keys().any(|&position| position > 30),
        "remote scroll reached distant refs: {remote:?}"
    );
    assert_eq!(
        list_a11y::recorded_list(PANES[LOCAL]).unwrap().rows,
        local_before,
        "scrolling REMOTE moved LOCAL"
    );

    // A real header click persists REMOTE's collapse bit (2); expanding
    // restores the unchanged pair weights and LOCAL's body height.
    click_header(cx, &app, window, REMOTE);
    assert!(cx.read(|cx| app.read(cx).sidebar.collapsed.contains("remote")));
    assert_eq!(saved_layout().collapsed_mask, 2);
    assert_eq!(pane_weights(cx, &app), weights);
    assert!(height(bounds(cx, &app)[LOCAL]) > height(after_drag[LOCAL]));
    click_header(cx, &app, window, REMOTE);
    assert_eq!(saved_layout().collapsed_mask, 0);
    assert_eq!(pane_weights(cx, &app), weights);
    let expanded = bounds(cx, &app);
    assert!(
        (height(expanded[LOCAL]) - height(after_drag[LOCAL])).abs() < 3.,
        "{after_drag:?} -> {expanded:?}"
    );

    // Persist a collapsed REMOTE, then drag LOCAL's divider: it skips
    // REMOTE's fixed header and resizes LOCAL against the nearest expanded
    // pane (WORKTREES), never mutating REMOTE's saved weight.
    click_header(cx, &app, window, REMOTE);
    let collapsed_bounds = bounds(cx, &app);
    assert!(
        near(height(collapsed_bounds[REMOTE]), ROW),
        "collapsed REMOTE is one header row: {collapsed_bounds:?}"
    );
    drag_divider(cx, &app, window, divider, &[8., 24.], |_| {});
    let skipped = pane_weights(cx, &app);
    assert_eq!(
        skipped[REMOTE], weights[REMOTE],
        "collapsed pane weight retained"
    );
    assert_ne!(
        skipped[WORKTREES], weights[WORKTREES],
        "nearest expanded pane changes"
    );
    assert_eq!(
        u32::from(skipped[LOCAL]) + u32::from(skipped[WORKTREES]),
        u32::from(weights[LOCAL]) + u32::from(weights[WORKTREES])
    );
    assert_eq!(skipped[WORKTREES + 1..], weights[WORKTREES + 1..]);
    let saved = saved_layout();
    assert_eq!((saved.weights, saved.collapsed_mask), (skipped, 2));
    let weights = skipped;
    settings::flush();
    unmount(cx, app, window);

    // An actual new app at 600px restores the weights and REMOTE's collapse.
    let (reopened, compact_window) = mount_at(cx, repo, 600.);
    redraw(cx, &reopened, compact_window);
    assert_eq!(pane_weights(cx, &reopened), weights);
    assert!(cx.read(|cx| reopened.read(cx).sidebar.collapsed.contains("remote")));
    let headers = header_heights(cx, &reopened, compact_window);
    assert!(
        headers.iter().all(|&measured| near(measured, ROW)),
        "five {ROW}px headers at 600px: {headers:?}"
    );
    let compact = bounds(cx, &reopened);
    let sidebar = e2e::control_bounds(compact_window.window_id(), "worktree-sidebar").unwrap();
    let sidebar_bottom = f32::from(sidebar.origin.y + sidebar.size.height);
    assert!(
        compact[4].1 <= sidebar_bottom + 1.,
        "five panes fit 600px: {compact:?}, sidebar={sidebar:?}"
    );
    assert!(near(height(compact[REMOTE]), ROW), "{compact:?}");

    theme::set_zoom(1.25);
    let headers = header_heights(cx, &reopened, compact_window);
    assert!(
        headers.iter().all(|&measured| near(measured, ROW * 1.25)),
        "five scaled headers at 600px/125%: {headers:?}"
    );
    let scaled = bounds(cx, &reopened);
    assert!(
        scaled.iter().all(|&pane| height(pane) >= ROW * 1.25 - 0.5),
        "all five scaled panes retain their header: {scaled:?}"
    );
    let sidebar = e2e::control_bounds(compact_window.window_id(), "worktree-sidebar").unwrap();
    let pane_viewport = e2e::control_bounds(compact_window.window_id(), "sidebar-panes").unwrap();
    let viewport_bottom = f32::from(pane_viewport.origin.y + pane_viewport.size.height);
    assert!(viewport_bottom <= f32::from(sidebar.origin.y + sidebar.size.height) + 1.);
    if scaled[4].1 > viewport_bottom {
        // The scaled stack exceeds the Graph pane. Scroll the real stack on
        // LOCAL's pinned header so no leaf list eats the wheel event; the
        // last section must become reachable.
        cx.simulate_event(
            compact_window,
            gpui::ScrollWheelEvent {
                position: point(
                    pane_viewport.origin.x + px(8.),
                    pane_viewport.origin.y + px(ROW * 1.25 / 2.),
                ),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-1000.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        redraw(cx, &reopened, compact_window);
        let scrolled = bounds(cx, &reopened);
        assert!(
            scrolled[4].1 <= viewport_bottom + 1. && scrolled[4].0 >= f32::from(pane_viewport.origin.y),
            "last header reachable by stack scroll: {scaled:?} -> {scrolled:?}, viewport={pane_viewport:?}"
        );
    }
    unmount(cx, reopened, compact_window);

    settings::write_setting("sidebar_panes", previous_setting.as_deref());
    settings::flush();
    theme::set_zoom(previous_zoom);
    drop(linked_root);
    eprintln!("[gui-e2e] PASS sidebar_panes: five panes without PULL REQUESTS, 20px rows / 24px worktree rows at 100% and 125%, six-pane v1 raw fallback and first-edit repair via LOCAL, LOCAL/REMOTE pointer-tracking drag, independent LOCAL/REMOTE scroll, collapsed REMOTE skip to WORKTREES, remount at 600px and 125% zoom");
}
