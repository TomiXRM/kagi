//! Presentation-only rows must not borrow a covered real row's context menu.
use super::{
    drawn, pointer,
    pointer_other::{focus, mount, root_focus},
    support, POPUP,
};
use crate::macos::{build_fixture, git, unmount};
use gpui::{
    point, px, AnyWindowHandle, Entity, FileDropEvent, Modifiers, MouseButton, VisualTestAppContext,
};
use kagi::ui::{e2e, list_a11y::recorded_list, sidebar, KagiApp};
use kagi_ui_core::theme;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn drop_repository(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    at: Instant,
    hit: gpui::Point<gpui::Pixels>,
    target: &Path,
) -> bool {
    cx.simulate_mouse_move(window, hit, None, Modifiers::none());
    cx.simulate_event(
        window,
        FileDropEvent::Entered {
            position: hit,
            paths: gpui::ExternalPaths(vec![target.to_path_buf()].into()),
        },
    );
    drawn(cx, app, window, at);
    cx.simulate_event(window, FileDropEvent::Submit { position: hit });
    cx.run_until_parked();
    app.read_with(cx, |app, _| app.repo_path.clone()) == Some(target.canonicalize().unwrap())
}

fn ghost(cx: &mut VisualTestAppContext, compact: bool, event: u8, failures: &mut Vec<String>) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["tag", "a-removed", "HEAD"]);
    git(repo, &["tag", "z-live", "HEAD~1"]);
    let (app, window, t) = mount(cx, repo, compact, 3);
    let baseline = app.read_with(cx, |app, _| app.ui().selected);
    git(repo, &["tag", "-d", "a-removed"]);
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| app.reload(cx));
    drawn(cx, &app, window, t);
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let ghost = sidebar::sidebar_motion_for_e2e(3, "tag:a-removed", "ghost")
        .expect("removed top row is visibly retained");
    assert!(ghost.opacity > 0. && ghost.opacity < 1.);
    assert!(
        !app.read_with(cx, |app, _| app
            .sidebar_row_keys_for_e2e(3)
            .iter()
            .any(|key| key == "tag:a-removed")),
        "ghost is not a keyboard/AX row"
    );
    let removed = e2e::control_bounds(window.window_id(), "sidebar-tag-a-removed").unwrap();
    let live = e2e::control_bounds(window.window_id(), "sidebar-tag-z-live").unwrap();
    let header = e2e::control_bounds(window.window_id(), "tags").unwrap();
    let top = removed.top().max(live.top()).max(header.bottom());
    let bottom = removed.bottom().min(live.bottom());
    if bottom - top <= px(2.) {
        eprintln!("[gui-e2e] SAFE sidebar ghost compact={compact}: no visible ghost/real intersection removed={removed:?}, live={live:?}, header={header:?}");
        unmount(cx, app, window);
        return;
    }
    // There is only one surviving real row. The ghost is painted after that
    // anchor within its slot, so no later real sibling can cover this overlap.
    let hit = point(removed.left() + px(32.), (top + bottom) * 0.5);
    let witness = support::Witness::read(repo);
    focus(cx, &app, window, 3, "tag:z-live");
    let label = format!("compact={compact}/top-removal ghost event={event}");
    match event {
        0 => {
            pointer(cx, window, hit, MouseButton::Right);
            let menu = app.read_with(cx, |app, _| {
                app.tag_menu.as_ref().map(|menu| menu.name.clone())
            });
            if menu.is_some() {
                failures.push(format!("{label}: covered menu {menu:?}"));
            }
            root_focus(cx, &app, window, &label, failures);
        }
        1 => {
            pointer(cx, window, hit, MouseButton::Left);
            let target = kagi_git::CommitId(support::output(repo, &["rev-parse", "z-live"]));
            let expected = app.read_with(cx, |app, _| Some(app.view().commit_row_index[&target]));
            assert_ne!(
                expected, baseline,
                "transparent left-click oracle must not be a no-op"
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.ui().selected),
                expected,
                "ghost left remains transparent to actual live content underneath"
            );
            root_focus(cx, &app, window, &label, failures);
        }
        2 => {
            let target = build_fixture();
            let opened = drop_repository(cx, &app, window, at, hit, target.path());
            if compact && !opened {
                // Compare the existing popup's ordinary settled row before
                // attributing its portal-level routing to retained content.
                cx.simulate_event(window, FileDropEvent::Exited);
                let settled = t + Duration::from_millis(500);
                drawn(cx, &app, window, settled);
                let regular =
                    e2e::control_bounds(window.window_id(), "sidebar-tag-z-live").unwrap();
                assert!(!drop_repository(cx,&app,window,settled,regular.center(),target.path()),"compact ghost and ordinary popup row must preserve the same ancestor-drop policy");
                eprintln!("[gui-e2e] BASELINE compact popup rejects ancestor ExternalPaths on both real settled and visible ghost rows");
            } else {
                assert!(opened,"ancestor ExternalPaths must remain reachable at visible normal-sidebar ghost pixels");
            }
        }
        _ => unreachable!(),
    }
    assert_eq!(
        support::Witness::read(repo),
        witness,
        "ghost input cannot mutate source repository"
    );
    unmount(cx, app, window);
}

fn fringe(cx: &mut VisualTestAppContext, compact: bool, failures: &mut Vec<String>) {
    let fixture = build_fixture();
    let repo = fixture.path();
    for index in 0..80 {
        git(repo, &["tag", &format!("row-{index:02}"), "HEAD~1"]);
    }
    let (app, window, t) = mount(cx, repo, compact, 3);
    let baseline = app.read_with(cx, |app, _| app.ui().selected);
    for index in 0..3 {
        git(repo, &["tag", &format!("aaa-{index}"), "HEAD~1"]);
    }
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| app.reload(cx));
    drawn(cx, &app, window, t);
    for index in 0..80 {
        e2e::clear_control_bounds(window.window_id(), &format!("sidebar-tag-row-{index:02}"));
    }
    let at = t + Duration::from_millis(37);
    drawn(cx, &app, window, at);
    let header = e2e::control_bounds(window.window_id(), "tags").unwrap();
    let bottom = app.read_with(cx, |app, _| px(app.sidebar.pane_geom[3].get().1));
    let (name, bounds, hit) = (0..80)
        .find_map(|index| {
            let name = format!("row-{index:02}");
            if sidebar::sidebar_motion_for_e2e(3, &format!("tag:{name}"), "row").is_some() {
                return None;
            }
            let bounds = e2e::control_bounds(window.window_id(), &format!("sidebar-tag-{name}"))?;
            let top = bounds.top().max(header.bottom());
            let end = bounds.bottom().min(bottom);
            (end - top > px(2.)).then(|| {
                (
                    name,
                    bounds,
                    point(bounds.left() + px(32.), (top + end) * 0.5),
                )
            })
        })
        .expect("an off-range row is actually painted into the viewport at 37ms");
    // Fringe shares the survivors' offset and only meets the final real row's
    // edge. Assert the safe geometry rather than inventing an overlapped menu.
    for index in 0..80 {
        let key = format!("tag:row-{index:02}");
        if sidebar::sidebar_motion_for_e2e(3, &key, "row").is_none() {
            continue;
        }
        let real = e2e::control_bounds(window.window_id(), &format!("sidebar-tag-row-{index:02}"))
            .unwrap();
        assert!(
            hit.y < real.top() || hit.y >= real.bottom(),
            "fringe point is not covered by a later real row: fringe={bounds:?}, real={real:?}"
        );
    }
    assert!(
        !recorded_list("sidebar-tags")
            .unwrap()
            .rows
            .values()
            .any(|(label, _)| label.contains(&name)),
        "fringe has no native logical TreeItem"
    );
    let label = format!("compact={compact}/fringe {name}");
    let witness = support::Witness::read(repo);
    focus(cx, &app, window, 3, "tag:row-00");
    pointer(cx, window, hit, MouseButton::Right);
    let menu = app.read_with(cx, |app, _| {
        app.tag_menu.as_ref().map(|menu| menu.name.clone())
    });
    if menu.is_some() {
        failures.push(format!("{label}: wrong menu {menu:?}"));
    }
    root_focus(cx, &app, window, &label, failures);
    app.update(cx, |app, cx| {
        app.tag_menu = None;
        cx.notify();
    });
    drawn(cx, &app, window, at);
    pointer(cx, window, hit, MouseButton::Left);
    let actual = app.read_with(cx, |app, _| app.ui().selected);
    if actual != baseline {
        failures.push(format!(
            "{label}: inert fringe selected {actual:?}, expected {baseline:?}"
        ));
    }
    drawn(cx, &app, window, at);
    if compact && e2e::control_bounds(window.window_id(), POPUP).is_none() {
        failures.push(format!("{label}: inert fringe dismissed popup"));
    } else {
        let target = build_fixture();
        let opened = drop_repository(cx, &app, window, at, hit, target.path());
        assert_eq!(
            opened, !compact,
            "fringe preserves the normal-sidebar / existing compact-popup ancestor-drop policy"
        );
    }
    assert_eq!(support::Witness::read(repo), witness);
    eprintln!("[gui-e2e] SAFE sidebar fringe compact={compact}: measured visible {name} {bounds:?} without a later real-row overlap");
    unmount(cx, app, window);
}

pub(super) fn presentation_rows(cx: &mut VisualTestAppContext, failures: &mut Vec<String>) {
    for compact in [false, true] {
        for event in 0..3 {
            ghost(cx, compact, event, failures);
        }
        fringe(cx, compact, failures);
    }
}
