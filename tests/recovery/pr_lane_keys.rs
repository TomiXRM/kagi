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
pub fn scenario_pr_lane_selected(cx: &mut VisualTestAppContext) {
    matrix_lane(cx, |cx, cell, _| {
        lane(cx, cell);
        let rows = history(cx, cell);
        let commits = cx.read(|cx| cell.app.read(cx).pr_mode().unwrap().tabs[0].commits.clone());
        for commit in commits.iter().take(2) {
            let ix = rows.iter().position(|row| row.id == commit.id).unwrap();
            let id = format!("pr-lane-row-{ix}");
            cx.simulate_click(cell.win, cell.bounds(&id).center(), gpui::Modifiers::none());
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
        cx.simulate_click(cell.win, cell.bounds(&id).center(), gpui::Modifiers::none());
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
