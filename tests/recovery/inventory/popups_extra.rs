use super::*;
use gpui::{Modifiers, MouseButton};
use kagi::ui::{e2e, editor_workspace::TreeMenuTarget, types::ToastKind};
use std::time::Duration;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn bounds(window: AnyWindowHandle, id: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} was not rendered"))
}

pub(in crate::inventory) fn commit_plan(
    f: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, f.repo.clone(), cx);
        e2e::set_commit_message(app, "Review blocker before committing", cx);
        let owner = app.active_session().expect("active commit owner");
        app.open_commit_plan_modal(owner, cx);
    });
    assert!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .commit_panel
            .as_ref()
            .is_some_and(|panel| {
                panel
                    .read(cx)
                    .state
                    .plan_modal
                    .as_ref()
                    .is_some_and(|modal| !modal.plan.blockers.is_empty())
            })),
        "conflict markers must retain commit plan instead of immediately committing"
    );
}

pub(in crate::inventory) fn worktree_inspection(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    draw(cx, window);
    app.update(cx, |app, cx| {
        let index = app.sidebar.rows.iter().position(|row| matches!(row, kagi::ui::sidebar::SidebarRow::Worktree { name, .. } if name == "linked"))
            .expect("linked worktree sidebar row");
        let first_leaf = app.sidebar.pane_ranges[2].start + 1;
        app.sidebar.scroll_handles[2].scroll_to_item(index - first_leaf, gpui::ScrollStrategy::Center);
        cx.notify();
    });
    e2e::clear_control_bounds(window.window_id(), "sidebar-worktree-linked");
    draw(cx, window);
    let row = bounds(window, "sidebar-worktree-linked");
    let pointer = point(row.origin.x + px(4.), row.center().y);
    cx.simulate_mouse_move(window, pointer, None, Modifiers::none());
    cx.run_until_parked();
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "worktree-inspection");
    draw(cx, window);
    bounds(window, "worktree-inspection");
}

pub(in crate::inventory) fn toast(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| {
        app.toast_stack
            .as_ref()
            .expect("mounted toast stack")
            .update(cx, |stack, cx| {
                stack.push_notify(ToastKind::Success, "Inventory operation complete", cx);
            });
    });
    assert!(cx.read(|cx| !app
        .read(cx)
        .toast_stack
        .as_ref()
        .unwrap()
        .read(cx)
        .is_empty()));
    e2e::clear_control_bounds(window.window_id(), "toast-stack");
    draw(cx, window);
    bounds(window, "toast-stack");
}

pub(in crate::inventory) fn busy_snackbar(
    f: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    // Same held real snapshot and GPUI-clock ticks as slow_read_explained.
    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
    kagi::ui::KagiApp::hold_next_snapshot_for_e2e(hold);
    f.hold(release);
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    for _ in 0..10 {
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
    }
    assert_eq!(
        cx.read(|cx| app.read(cx).slow_read_shown_for_e2e()),
        Some("ahead-behind")
    );
    e2e::clear_control_bounds(window.window_id(), "busy-snackbar-skip");
    draw(cx, window);
    bounds(window, "busy-snackbar-skip");
}

pub(in crate::inventory) fn editor_tree_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        let editor = app
            .ui()
            .editor_workspace
            .as_ref()
            .expect("open editor")
            .clone();
        editor.update(cx, |view, cx| {
            view.open_tree_menu(TreeMenuTarget::Root, menu_point(), cx)
        });
    });
    assert!(cx.read(|cx| app
        .read(cx)
        .ui()
        .editor_workspace
        .as_ref()
        .unwrap()
        .read(cx)
        .tree_menu
        .is_some()));
    draw(cx, window);
}

pub(in crate::inventory) fn conflict_file_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        let conflict = app.ui().conflict.as_ref().expect("conflict view").clone();
        conflict.update(cx, |view, cx| {
            assert!(view
                .mode
                .as_ref()
                .is_some_and(|mode| !mode.session.files.is_empty()));
            view.file_menu = Some((0, menu_point()));
            cx.notify();
        });
    });
    assert!(cx.read(|cx| app
        .read(cx)
        .ui()
        .conflict
        .as_ref()
        .unwrap()
        .read(cx)
        .file_menu
        .is_some()));
    draw(cx, window);
}

pub(in crate::inventory) fn filter_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    e2e::clear_control_bounds(window.window_id(), "list-filter-state");
    draw(cx, window);
    cx.simulate_click(
        window,
        bounds(window, "list-filter-state").center(),
        Modifiers::none(),
    );
    cx.run_until_parked();
    draw(cx, window);
    bounds(window, "list-filter-option-0-0");
}

pub(in crate::inventory) fn file_menu(
    f: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, f.repo.clone(), cx)
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            e2e::defer_file_menu(app, 0, menu_point(), window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert!(cx.read(|cx| app
        .read(cx)
        .file_menu
        .as_ref()
        .is_some_and(|menu| menu.path == std::path::Path::new("README.md"))));
}

pub(in crate::inventory) fn inspector_file_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "inspector-file-0");
    draw(cx, window);
    let row = bounds(window, "inspector-file-0");
    cx.simulate_mouse_down(window, row.center(), MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(window, row.center(), MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).inspector_file_menu.is_some()));
}

pub(in crate::inventory) fn pr_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    let pr = crate::evidence_support::pull_request(1013, "Inventory PR", "feature");
    app.update(cx, |app, cx| {
        app.show_pr_mode(cx);
        let ui = app.ui_mut().expect("active PR session");
        ui.github_prs = vec![pr.clone()];
        ui.pr_menu = Some((pr, menu_point()));
        cx.notify();
    });
    assert!(cx.read(|cx| app
        .read(cx)
        .ui()
        .pr_menu
        .as_ref()
        .is_some_and(|(pr, _)| pr.number == 1013)));
    draw(cx, window);
}

pub(in crate::inventory) fn coauthor_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .update(cx, |panel, cx| panel.toggle_coauthor_menu(cx));
    });
    assert!(cx.read(|cx| app
        .read(cx)
        .ui()
        .commit_panel
        .as_ref()
        .unwrap()
        .read(cx)
        .coauthor_menu
        .is_some()));
    draw(cx, window);
}
