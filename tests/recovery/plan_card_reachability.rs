//! Regression coverage for #1481 compact suspension stealing plan-card Tab focus.
use crate::macos::{build_fixture, git, repo_fingerprint};
use crate::presentation_support::{each_presentation, paint, Presentation};
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{button_style, e2e, KagiApp};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn covered_rail_is_inert(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let trigger = "compact-rail-prs";
    if e2e::control_bounds(window.window_id(), trigger).is_none() {
        return;
    }
    crate::native_ax::activate(cx);
    paint(cx, window);
    let focused = |cx: &mut VisualTestAppContext| {
        let tree = crate::native_ax::tree(cx, window);
        let key = tree["gpui_focus"].as_str().expect("real GPUI focus");
        tree["nodes"][key]["element_id"].as_str()
            == Some(format!("{:?}", gpui::ElementId::from(trigger)).as_str())
    };
    for _ in 0..80 {
        cx.simulate_keystrokes(window, "tab");
        cx.run_until_parked();
        paint(cx, window);
        if focused(cx) {
            break;
        }
    }
    assert!(focused(cx), "Tab reaches the actual covered rail trigger");
    let mode = app.read_with(cx, |app, _| app.workspace_mode());
    for key in ["enter", "space"] {
        e2e::clear_control_bounds(window.window_id(), "compact-sidebar-popover");
        crate::keyboard_nav::keys(cx, window, key);
        cx.run_until_parked();
        paint(cx, window);
        cx.run_until_parked();
        paint(cx, window);
        assert_eq!(
            app.read_with(cx, |app, _| app.workspace_mode()),
            mode,
            "{key}: a covered rail trigger cannot change workspace mode"
        );
        assert!(
            e2e::control_bounds(window.window_id(), "compact-sidebar-popover").is_none(),
            "{key}: a covered rail trigger cannot open a popover"
        );
        assert!(focused(cx), "{key}: covered activation cannot steal focus");
        assert_eq!(
            crate::native_ax::node(cx, window, trigger.into())["aria"]["expanded"],
            false
        );
    }
}

/// The shared non-danger cards and the bespoke cherry-pick card must survive
/// compact suspension repaints while Tab crosses the rail and card controls.
pub fn scenario_neighbor_plan_labels(cx: &mut VisualTestAppContext) {
    let _presentation = Presentation::keep();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-qb", "side", "HEAD~1"]);
    std::fs::write(repo.join("side.txt"), "side\n").unwrap();
    git(repo, &["add", "side.txt"]);
    git(repo, &["commit", "-qm", "side commit"]);
    let pick = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "-q", "main"]);
    let revert = git_fixture::git_output(repo, &["rev-parse", "HEAD"]);
    let before = repo_fingerprint(repo);
    each_presentation(cx, repo, |cx, app, window| {
        for (operation, cancel, confirm) in [
            ("cherry-pick", "cherry-pick-cancel", "cherry-pick-confirm"),
            ("revert", "plan-cancel", "plan-confirm"),
            ("merge", "plan-cancel", "plan-confirm"),
        ] {
            app.update(cx, |app, cx| {
                match operation {
                    "cherry-pick" => app.open_cherry_pick_modal(kagi_git::CommitId(pick.clone())),
                    "revert" => app.open_revert_modal(kagi_git::CommitId(revert.clone())),
                    "merge" => app.open_merge_modal("side".into(), None, cx),
                    _ => unreachable!(),
                }
                cx.notify();
            });
            crate::recovery_operations::wait_idle(cx, app);
            paint(cx, window);
            let action = button_style::recorded_modal_button(confirm).unwrap();
            assert_eq!(action.role, Role::Button);
            assert!(
                !action.disabled,
                "{operation}: confirmation must be available"
            );
            // Preserve the original root-start driver: forcing modal_focus
            // would hide a production cycle before the card's first stop.
            crate::recovery_operations::press_key(cx, app, window, "f19");
            covered_rail_is_inert(cx, app, window);
            let mut saw_cancel = false;
            let mut reached = false;
            for _ in 0..80 {
                button_style::modal_focus_probe();
                cx.simulate_keystrokes(window, "tab");
                cx.run_until_parked();
                paint(cx, window);
                cx.simulate_keystrokes(window, "f19");
                match button_style::modal_focus_probe() {
                    Some(id) if id == cancel => saw_cancel = true,
                    Some(id) if id == confirm => {
                        reached = true;
                        break;
                    }
                    _ => {}
                }
            }
            assert!(
                saw_cancel && reached,
                "{operation}: Tab must cross Cancel to the real Confirm"
            );
            assert_eq!(
                repo_fingerprint(repo),
                before,
                "Tab does not approve a write"
            );
            crate::keyboard_nav::keys(cx, window, "escape");
            paint(cx, window);
            assert_eq!(repo_fingerprint(repo), before, "Escape is read-only");
        }
    });
}
