//! Regression coverage for #1481 compact suspension stealing plan-card Tab focus.
use crate::macos::{build_fixture, git, open_offscreen, repo_fingerprint, unmount};
use gpui::{px, size, AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{button_style, e2e, KagiApp};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, path::Path, rc::Rc};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

struct Presentation {
    language: i18n::Lang,
    zoom: f32,
    palette: String,
    _saved: crate::gui_isolation::SavedKeys,
}
impl Presentation {
    fn keep() -> Self {
        Self {
            language: i18n::lang(),
            zoom: theme::zoom(),
            palette: theme::theme().slug.to_string(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]),
        }
    }
}
impl Drop for Presentation {
    fn drop(&mut self) {
        i18n::set_lang(self.language);
        theme::set_zoom(self.zoom);
        theme::set_active(&self.palette);
    }
}
fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn each_presentation(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    mut check: impl FnMut(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    for dimensions in [(940., 660.), (1392., 883.)] {
        for language in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(language);
            for palette in ["apple-light", "apple-dark"] {
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    let state = e2e::app_state(repo).unwrap();
                    let capture = Rc::new(RefCell::new(None));
                    let out = capture.clone();
                    let window = open_offscreen(
                        cx,
                        size(px(dimensions.0), px(dimensions.1)),
                        move |window, cx| e2e::mount_root(state, window, cx, &out),
                    );
                    let app = capture.borrow().clone().unwrap();
                    cx.run_until_parked();
                    app.update(cx, |app, cx| app.set_theme(palette, cx));
                    check(cx, &app, window.into());
                    unmount(cx, app, window.into());
                }
            }
        }
    }
}
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
    checkout_and_ref_only_preview(cx);
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

/// #1696: quiet plans retain their safety guarantees in AFTER and Copy all.
fn checkout_and_ref_only_preview(cx: &mut VisualTestAppContext) {
    use kagi::ui::{
        dialog_a11y,
        modals::{BranchPlanKind, BranchPlanModal},
    };
    use std::sync::Arc;

    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "preview-side"]);
    git(repo, &["branch", "--set-upstream-to=main", "preview-side"]);
    git(
        repo,
        &["commit", "--allow-empty", "-qm", "advance upstream"],
    );
    std::fs::write(repo.join("README.md"), "local retained change\n").unwrap();
    git(repo, &["add", "README.md"]);
    std::fs::write(repo.join("preview-untracked.txt"), "retained\n").unwrap();
    let before = repo_fingerprint(repo);
    each_presentation(cx, repo, |cx, app, window| {
        for ref_only in [false, true] {
            app.update(cx, |app, cx| {
                if ref_only {
                    let plan = kagi_git::Backend::open(repo)
                        .unwrap()
                        .plan(&kagi_git::Operation::PullBranchFf {
                            branch_name: "preview-side".into(),
                        })
                        .unwrap();
                    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
                    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
                    app.set_branch_plan_modal(BranchPlanModal {
                        kind: BranchPlanKind::PullFfOnly,
                        branch_name: "preview-side".into(),
                        plan: Arc::new(plan),
                        error: None,
                        confirm_armed: false,
                        fetch_owner: None,
                        dirty_digest: None,
                        fetched_refs: None,
                    });
                } else {
                    app.open_plan_modal("preview-side", cx);
                    let plan = &app.plan_modal().unwrap().plan;
                    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
                    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
                }
                cx.notify();
            });
            paint(cx, window);
            let expected = match (ref_only, i18n::lang()) {
                (false, i18n::Lang::En) => "Local changes and untracked files remain after switching branches.",
                (false, i18n::Lang::Ja) => "ローカルの変更と未追跡ファイルは branch を切り替えた後も保持されます。",
                (true, i18n::Lang::En) => "Only the selected branch reference is updated; the current working tree and index are unchanged.",
                (true, i18n::Lang::Ja) => "選択した branch の参照のみを更新します。現在の working tree と index は変更しません。",
            };
            let (_, after) = dialog_a11y::recorded_note("plan-state-after").unwrap();
            assert!(
                after.contains(expected),
                "neutral AFTER context missing: {after}"
            );
            assert!(
                !button_style::recorded_modal_button("plan-confirm")
                    .unwrap()
                    .disabled
            );
            let copy = e2e::control_bounds(window.window_id(), "plan-card-copy").unwrap();
            cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            let copied = cx.read_from_clipboard().unwrap().text().unwrap();
            assert!(
                copied.contains(expected),
                "Copy all lost neutral context: {copied}"
            );
            assert!(
                !copied.contains("\nwarning:"),
                "quiet plans must not manufacture cautions"
            );
            assert_eq!(
                repo_fingerprint(repo),
                before,
                "preview/copy must not write"
            );
            crate::keyboard_nav::keys(cx, window, "escape");
        }
    });
}
