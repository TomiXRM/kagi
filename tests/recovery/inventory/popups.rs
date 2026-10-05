use gpui::{point, px, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    commands::{BranchPickerMode, MenuOverlay},
    KagiApp,
};

use super::fixtures::{oid, Fixture};

fn command(
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
    id: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(id, window, cx));
    })
    .unwrap();
}
fn menu_point() -> gpui::Point<gpui::Pixels> {
    point(px(650.), px(230.))
}

pub(super) fn branch_picker_checkout(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    command(app, window, cx, "branch.checkout");
    assert!(cx.read(|cx| matches!(
        app.read(cx).menu_overlay,
        Some(MenuOverlay::BranchPicker {
            mode: BranchPickerMode::Checkout,
            ..
        })
    )));
}
pub(super) fn branch_picker_delete(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    command(app, window, cx, "branch.delete");
    assert!(cx.read(|cx| matches!(
        app.read(cx).menu_overlay,
        Some(MenuOverlay::BranchPicker {
            mode: BranchPickerMode::Delete,
            ..
        })
    )));
}
pub(super) fn info_about(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    command(app, window, cx, "app.about");
    assert!(cx.read(|cx| matches!(&app.read(cx).menu_overlay, Some(MenuOverlay::Info { .. }))));
}
pub(super) fn info_shortcuts(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    command(app, window, cx, "help.shortcuts");
    assert!(cx.read(|cx| matches!(&app.read(cx).menu_overlay, Some(MenuOverlay::Info { .. }))));
}
pub(super) fn settings(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    command(app, window, cx, "app.settings");
    assert!(cx.read(|cx| matches!(app.read(cx).menu_overlay, Some(MenuOverlay::Settings))));
}
pub(super) fn command_palette(
    _: &Fixture,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_command_palette(window, cx))
    })
    .unwrap();
    assert!(cx.read(|cx| matches!(app.read(cx).menu_overlay, Some(MenuOverlay::CommandPalette))));
}
pub(super) fn commit_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| app.open_commit_menu(0, menu_point()));
    assert!(cx.read(|cx| app
        .read(cx)
        .commit_menu
        .as_ref()
        .is_some_and(|menu| menu.row_index == 0)));
}
pub(super) fn branch_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        app.open_local_branch_menu("feature".into(), menu_point())
    });
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
}
pub(super) fn remote_branch_menu(
    f: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        app.open_remote_branch_menu("origin/feature".into(), oid(&f.repo, "HEAD"), menu_point())
    });
    assert!(cx.read(|cx| app.read(cx).branch_menu.is_some()));
}
pub(super) fn tag_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        app.open_tag_menu("v1.0.0".into(), menu_point())
    });
    assert!(cx.read(|cx| app.read(cx).tag_menu.is_some()));
}
pub(super) fn stash_menu(
    _: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        app.open_stash_menu(0, "Inventory saved work".into(), menu_point())
    });
    assert!(cx.read(|cx| app.read(cx).stash_menu.is_some()));
}
pub(super) fn worktree_menu(
    f: &Fixture,
    app: &Entity<KagiApp>,
    _: AnyWindowHandle,
    cx: &mut VisualTestAppContext,
) {
    app.update(cx, |app, _| {
        app.open_worktree_menu(
            "linked".into(),
            false,
            false,
            Some(f.repo.join("linked")),
            menu_point(),
        )
    });
    assert!(cx.read(|cx| app.read(cx).worktree_menu.is_some()));
}

#[path = "popups_extra.rs"]
mod extra;
pub(super) use extra::*;
