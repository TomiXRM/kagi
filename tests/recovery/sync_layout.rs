//! #1646: a long armed action must never hide the safe Cancel choice.
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, Pixels, VisualTestAppContext,
};
use kagi::ui::{e2e, BranchPlanKind, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, git, open_offscreen, repo_fingerprint, unmount};
use crate::recovery_layout::contained;
use crate::recovery_oplog_panel::paint;

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn mount_narrow(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    width: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).expect("fixture state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let out = captured.clone();
    let window = open_offscreen(cx, size(px(width), px(700.)), move |window, cx| {
        e2e::mount_root(state, window, cx, &out)
    });
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, window.into())
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} must be painted"))
}

fn actions(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> [Bounds<Pixels>; 2] {
    for id in ["modal-card", "modal-footer", "plan-cancel", "plan-confirm"] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    paint(cx, window);
    let viewport = cx
        .update_window(window, |_, window, _| {
            Bounds::new(point(px(0.), px(0.)), window.viewport_size())
        })
        .unwrap();
    let card = bounds(window, "modal-card");
    let footer = bounds(window, "modal-footer");
    contained(viewport, card, "sync card");
    contained(card, footer, "pinned sync footer");
    let cancel = bounds(window, "plan-cancel");
    let confirm = bounds(window, "plan-confirm");
    for (id, button) in [("Cancel", cancel), ("Confirm", confirm)] {
        contained(footer, button, id);
        assert!(button.size.width >= theme::scaled_px(24.), "{id} width");
        assert!(button.size.height >= theme::scaled_px(16.), "{id} height");
    }
    assert!(
        cancel.right() <= confirm.left()
            || confirm.right() <= cancel.left()
            || cancel.bottom() <= confirm.top()
            || confirm.bottom() <= cancel.top(),
        "Cancel and Confirm must not overlap: {cancel:?} / {confirm:?}"
    );
    if cancel.top() != confirm.top() {
        assert!(
            confirm.bottom() <= cancel.top(),
            "stacked Cancel stays below Confirm"
        );
    }
    [cancel, confirm]
}

fn expand_recovery(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "plan-recovery-body");
    paint(cx, window);
    if e2e::control_bounds(window.window_id(), "plan-recovery-body").is_none() {
        let header = bounds(window, "plan-recovery");
        cx.simulate_click(window, header.center(), Modifiers::none());
        paint(cx, window);
    }
    assert!(e2e::control_bounds(window.window_id(), "plan-recovery-body").is_some());
}

fn cancel_case(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    armed: bool,
) {
    let before = repo_fingerprint(repo);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let refs = git_fixture::git_output(repo, &["show-ref"]);
    let bytes = std::fs::read(repo.join("README.md")).unwrap();
    app.update(cx, |app, _| {
        app.open_branch_plan_modal("main".into(), BranchPlanKind::SyncToRemote)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).branch_plan_modal().expect("real sync plan");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert!(!modal.confirm_armed);
    });
    actions(cx, window);
    expand_recovery(cx, window);
    let [_, confirm] = actions(cx, window);
    if armed {
        cx.simulate_click(window, confirm.center(), Modifiers::none());
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).branch_plan_modal().unwrap().confirm_armed));
        actions(cx, window);
        expand_recovery(cx, window);
    }
    let [cancel, _] = actions(cx, window);
    assert_eq!(repo_fingerprint(repo), before, "plan/arm must not write");
    cx.simulate_click(window, cancel.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).branch_plan_modal().is_none()),
        "visible Cancel must dismiss"
    );
    assert_eq!(repo_fingerprint(repo), before);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(git_fixture::git_output(repo, &["show-ref"]), refs);
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), bytes);
    assert!(kagi_git::oplog::read_oplog_tail_for_repo(repo, 100)
        .iter()
        .all(|entry| entry.op != "sync-to-remote"));
}

pub fn scenario_sync_footer_cancel_visible(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let old_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
    git(repo, &["branch", "--set-upstream-to=origin/main", "main"]);
    std::fs::write(repo.join("README.md"), "staged fixture\n").unwrap();
    git(repo, &["add", "README.md"]);
    std::fs::write(repo.join("README.md"), "staged then edited fixture\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "preserved work\n").unwrap();
    for slug in ["apple-light", "apple-dark"] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            for zoom in [1., 1.67] {
                for width in [1000., 940.] {
                    let (app, window) = mount_narrow(cx, repo, width);
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                    });
                    theme::set_zoom(zoom);
                    paint(cx, window);
                    for armed in [false, true] {
                        cancel_case(cx, &app, window, repo, armed);
                    }
                    unmount(cx, app, window);
                }
            }
        }
    }
    theme::set_active(&old_theme);
    cx.update(theme::sync_gpui_component_theme);
}
