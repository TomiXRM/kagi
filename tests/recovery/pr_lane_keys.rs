//! PR lane keyboard, selected-state and truthful-count regressions.
use crate::pr_detail_a11y::support::{matrix_commit_count, Cell};
use gpui::{Role, VisualTestAppContext};
use kagi::ui::{e2e, list_a11y};
use kagi_ui_core::{i18n::Lang, theme};
fn lane(cx: &mut VisualTestAppContext, cell: &Cell) -> list_a11y::RecordedList {
    e2e::clear_control_bounds(cell.win.window_id(), "pr-lane-body");
    cell.paint(cx);
    if e2e::control_bounds(cell.win.window_id(), "pr-lane-body").is_none() {
        let bounds = cell.bounds("pr-swimlane-toggle");
        cx.simulate_click(cell.win, bounds.center(), gpui::Modifiers::none());
    }
    list_a11y::clear_recorded_lists();
    cell.paint(cx);
    list_a11y::recorded_list("pr-lane-body")
        .expect("PR lane must expose named keyboard-selectable rows")
}
fn matrix_lane(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Cell, &std::path::Path),
) {
    matrix_commit_count(cx, &[Lang::En, Lang::Ja], 10, check);
}
fn focused(cx: &mut VisualTestAppContext, cell: &Cell) -> Option<String> {
    cx.update_window(cell.win, |_, window, cx| {
        cell.app
            .read(cx)
            .list_row_focused_for_e2e("pr-lane-body", window)
    })
    .unwrap()
}
fn history(cx: &mut VisualTestAppContext, cell: &Cell) -> Vec<kagi::ui::commit_list::CommitRow> {
    cx.read(|cx| cell.app.read(cx).pr_lane_history_for_e2e())
}
fn selected(cx: &mut VisualTestAppContext, cell: &Cell) -> Option<String> {
    cx.read(|cx| {
        let tab = &cell.app.read(cx).pr_mode().unwrap().tabs[0];
        tab.selected_commit.map(|ix| tab.commits[ix].id.0.clone())
    })
}
fn wheel(cx: &mut VisualTestAppContext, cell: &Cell, dy: f32) {
    cx.simulate_event(
        cell.win,
        gpui::ScrollWheelEvent {
            position: cell.bounds("pr-lane-body").center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cell.paint(cx);
}
fn assert_extent(cx: &mut VisualTestAppContext, cell: &Cell, count: usize) {
    let (viewport, maximum) = cx.read(|cx| {
        cell.app
            .read(cx)
            .pr_lane_geometry_for_e2e()
            .expect("painted uniform list")
    });
    let expected = (count as f32 * f32::from(theme::scaled_px(27.)) - viewport).max(0.);
    assert!(
        (maximum - expected).abs() <= 1.,
        "live native extent {maximum} must account for all {count} slots, expected {expected}"
    );
}
pub fn scenario_pr_lane_keyboard(cx: &mut VisualTestAppContext) {
    matrix_lane(cx, |cx, cell, _| {
        let list = lane(cx, cell);
        assert_eq!(list.role, Some(Role::ListBox));
        assert_eq!(list.size, 10);
        for _ in 0..200 {
            cx.update_window(cell.win, |_, window, cx| window.focus_next(cx))
                .unwrap();
            cell.paint(cx);
            if focused(cx, cell).is_some() {
                break;
            }
        }
        assert!(focused(cx, cell).is_some(), "Tab reaches lane");
        let old = focused(cx, cell);
        crate::keyboard_nav::keys(cx, cell.win, "down");
        cell.paint(cx);
        assert_ne!(focused(cx, cell), old, "Down moves to another PR commit");
        assert_eq!(selected(cx, cell), None, "focus movement does not activate");
        let commits = cx.read(|cx| cell.app.read(cx).pr_mode().unwrap().tabs[0].commits.clone());
        let history_rows = history(cx, cell);
        let mut ordered: Vec<_> = commits
            .iter()
            .map(|commit| {
                (
                    history_rows
                        .iter()
                        .position(|row| row.id == commit.id)
                        .unwrap(),
                    commit.id.0.clone(),
                )
            })
            .collect();
        ordered.sort_by_key(|(index, _)| *index);
        assert_extent(cx, cell, history_rows.len());
        let mut wide = history_rows.clone();
        for (index, row) in wide.iter_mut().enumerate() {
            row.lane = index;
        }
        cell.app
            .update(cx, |app, cx| app.set_pr_lane_history_for_e2e(wide, cx));
        cell.paint(cx);
        let vertical = cx.read(|cx| cell.app.read(cx).pr_lane_vertical_offset_for_e2e().unwrap());
        for modifiers in [
            gpui::Modifiers::none(),
            gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
        ] {
            cx.simulate_event(
                cell.win,
                gpui::ScrollWheelEvent {
                    position: cell.bounds("pr-lane-body").center(),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(-100.), gpui::px(0.))),
                    modifiers,
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
            cell.paint(cx);
            assert_eq!(
                cx.read(|cx| cell.app.read(cx).pr_lane_vertical_offset_for_e2e().unwrap()),
                vertical,
                "horizontal wheel must not move the vertical PR list"
            );
            assert!(
                cx.read(|cx| cell
                    .app
                    .read(cx)
                    .pr_mode()
                    .unwrap()
                    .lane_scroll_x
                    .unwrap_or(0.))
                    > 0.,
                "horizontal wheel still reaches the overflowing lane rail"
            );
        }
        cell.app.update(cx, |app, cx| {
            app.set_pr_lane_history_for_e2e(history_rows.clone(), cx)
        });
        cell.paint(cx);
        wheel(cx, cell, -1_000_000.);
        let viewport = cell.bounds("pr-lane-body");
        let tail = cell.bounds(&format!("pr-lane-row-{}", history_rows.len() - 1));
        assert!(
            tail.top() >= viewport.top() && tail.bottom() <= viewport.bottom(),
            "one wheel reaches the actual final context/base row"
        );
        wheel(cx, cell, 1_000_000.);
        let first = cell.bounds("pr-lane-row-0");
        assert!(
            (f32::from(first.top() - viewport.top())).abs() <= 1.,
            "reverse wheel reaches the first painted row"
        );
        for (key, expected) in [("end", &ordered.last().unwrap().1), ("home", &ordered[0].1)] {
            crate::keyboard_nav::keys(cx, cell.win, key);
            cell.paint(cx);
            assert_eq!(focused(cx, cell).as_ref(), Some(expected));
            let index = history_rows
                .iter()
                .position(|row| &row.id.0 == expected)
                .unwrap();
            let bounds = cell.bounds(&format!("pr-lane-row-{index}"));
            let viewport = cell.bounds("pr-lane-body");
            assert!(
                bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
                "{key} reveals its actual painted row: {bounds:?} in {viewport:?}"
            );
        }
        crate::keyboard_nav::keys(cx, cell.win, "pagedown");
        cell.paint(cx);
        assert_ne!(focused(cx, cell).as_ref(), Some(&ordered[0].1));
        crate::keyboard_nav::keys(cx, cell.win, "pageup");
        cell.paint(cx);
        assert_eq!(focused(cx, cell).as_ref(), Some(&ordered[0].1));
        for key in ["home", "end", "up", "home", "enter"] {
            crate::keyboard_nav::keys(cx, cell.win, key);
            cell.paint(cx);
        }
        assert_eq!(
            selected(cx, cell),
            focused(cx, cell),
            "Enter selects exact focused SHA"
        );
        let selected_sha = selected(cx, cell).unwrap();
        let selected_row = list_a11y::recorded_list("pr-lane-body").unwrap();
        let name = &selected_row
            .rows
            .values()
            .find(|(_, selected)| *selected)
            .unwrap()
            .0;
        assert!(name.contains(&selected_sha) && name.contains("PR #7"));
        let commit = history(cx, cell)
            .into_iter()
            .find(|r| r.id.0 == selected_sha)
            .unwrap();
        assert!(name.contains(commit.summary.as_ref()));
        let owner = focused(cx, cell);
        crate::keyboard_nav::keys(cx, cell.win, "tab");
        cell.paint(cx);
        assert_eq!(focused(cx, cell), None, "one Tab exits the row owner");
        assert!(owner.is_some());
    });
}
fn native_fill(cx: &mut VisualTestAppContext, cell: &Cell, id: &str) -> [u8; 4] {
    let bounds = cell.bounds(id);
    cx.update_window(cell.win, |_, window, _| {
        let image = window.render_to_image().expect("native lane paint");
        let scale = window.scale_factor();
        image
            .get_pixel(
                ((f32::from(bounds.right()) - 8.) * scale) as u32,
                ((f32::from(bounds.top()) + 6.) * scale) as u32,
            )
            .0
    })
    .unwrap()
}
fn reveal_row(cx: &mut VisualTestAppContext, cell: &Cell, id: &str) -> gpui::Bounds<gpui::Pixels> {
    for _ in 0..32 {
        e2e::clear_control_bounds(cell.win.window_id(), id);
        cell.paint(cx);
        let viewport = cell.bounds("pr-lane-body");
        let bounds = e2e::control_bounds(cell.win.window_id(), id);
        if let Some(bounds) = bounds {
            if bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom() {
                return bounds;
            }
        }
        let dy = bounds
            .map(|bounds| f32::from(viewport.center().y - bounds.center().y))
            .unwrap_or(-80.);
        cx.simulate_event(
            cell.win,
            gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
    }
    panic!("{id} must be reachable through the native lane viewport");
}
pub fn scenario_pr_lane_selected(cx: &mut VisualTestAppContext) {
    matrix_lane(cx, |cx, cell, _| {
        lane(cx, cell);
        let rows = history(cx, cell);
        let commits = cx.read(|cx| cell.app.read(cx).pr_mode().unwrap().tabs[0].commits.clone());
        for commit in commits.iter().take(2) {
            let ix = rows.iter().position(|row| row.id == commit.id).unwrap();
            let id = format!("pr-lane-row-{ix}");
            let bounds = reveal_row(cx, cell, &id);
            cx.simulate_click(cell.win, bounds.center(), gpui::Modifiers::none());
            cell.paint(cx);
            assert_eq!(selected(cx, cell).as_deref(), Some(commit.id.0.as_str()));
            let list = list_a11y::recorded_list("pr-lane-body").unwrap();
            assert_eq!(
                list.rows.values().filter(|(_, selected)| *selected).count(),
                1
            );
            for active in [true, false] {
                cell.app
                    .update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
                cell.paint(cx);
                let expected = if active {
                    if matches!(&*theme::theme().slug, "apple-light" | "apple-dark") {
                        theme::theme().ui_accent()
                    } else {
                        theme::theme().selected
                    }
                } else {
                    theme::theme().inactive_selected
                };
                let expected = [
                    (expected >> 16) as u8,
                    (expected >> 8) as u8,
                    expected as u8,
                    255,
                ];
                assert_eq!(native_fill(cx, cell, &id), expected, "shared selected fill");
                assert_eq!(
                    e2e::control_paint(cell.win.window_id(), &format!("pr-lane-label-{ix}"))
                        .unwrap()
                        .text_color,
                    theme::theme().selection_foreground(active),
                    "actual selected lane subject foreground follows activation"
                );
                cx.simulate_event(
                    cell.win,
                    gpui::MouseMoveEvent {
                        position: cell.bounds(&id).center(),
                        modifiers: gpui::Modifiers::none(),
                        pressed_button: None,
                    },
                );
                cell.paint(cx);
                assert_eq!(
                    native_fill(cx, cell, &id),
                    expected,
                    "hover retains selected fill"
                );
            }
        }
        let before = selected(cx, cell);
        let context = rows
            .iter()
            .enumerate()
            .find(|(_, row)| !commits.iter().any(|c| c.id == row.id))
            .unwrap();
        let id = format!("pr-lane-row-{}", context.0);
        let bounds = reveal_row(cx, cell, &id);
        cx.simulate_click(cell.win, bounds.center(), gpui::Modifiers::none());
        cell.paint(cx);
        assert_eq!(
            selected(cx, cell),
            before,
            "context cannot activate a PR commit"
        );
        cell.app
            .update(cx, |app, cx| app.pr_mode_select_commit(None, cx));
        assert!(lane(cx, cell).rows.values().all(|(_, selected)| !selected));
    });
}
pub fn scenario_pr_lane_count(cx: &mut VisualTestAppContext) {
    matrix_lane(cx, |cx, cell, _| {
        lane(cx, cell);
        let all = history(cx, cell);
        let commits = cx.read(|cx| cell.app.read(cx).pr_mode().unwrap().tabs[0].commits.clone());
        let partial = all
            .iter()
            .filter(|row| !commits.iter().skip(3).any(|c| c.id == row.id))
            .cloned()
            .collect();
        cell.app.update(cx, |app, cx| {
            app.set_pr_lane_total_for_e2e(Some(10), cx);
            app.set_pr_lane_history_for_e2e(partial, cx);
        });
        let partial_label = lane(cx, cell).label;
        let partial_count = history(cx, cell).len();
        assert_extent(cx, cell, partial_count);
        assert!(
            partial_label.contains("10"),
            "three loaded never masquerades as the PR total: {partial_label}"
        );
        cell.app.update(cx, |app, cx| {
            app.set_pr_lane_history_for_e2e(all.clone(), cx)
        });
        assert_eq!(
            lane(cx, cell).label,
            partial_label,
            "authoritative total survives history changes"
        );
        cell.app.update(cx, |app, cx| {
            app.set_pr_lane_total_for_e2e(None, cx);
            app.set_pr_lane_history_for_e2e(
                all.iter()
                    .filter(|row| !commits.iter().skip(3).any(|c| c.id == row.id))
                    .cloned()
                    .collect(),
                cx,
            );
        });
        let unknown = lane(cx, cell).label;
        let expected = kagi_ui_core::i18n::Msg::PrLaneLoaded
            .t()
            .replacen("{}", "3", 1);
        assert!(
            unknown.contains(&expected),
            "unknown PR total explicitly says loaded: {unknown}"
        );
        cell.app.update(cx, |app, cx| {
            app.set_pr_lane_history_for_e2e(all.clone(), cx)
        });
    });
}
