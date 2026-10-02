//! Native geometry regression for the bottom panel inside nested workspaces.
//! PR/Issues own a navigator, and Editor owns a file tree and hunks pane;
//! none of those side panes may stop above the host's bottom panel.

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, theme};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for name in [
        "bottom-panel",
        "pr-mode-left-pane",
        "pr-mode-center-pane",
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "editor-workspace-slot",
    ] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .expect("draw nested workspace");
}

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name)
        .unwrap_or_else(|| panic!("{name} was not painted"))
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 2.0
}

fn right(rect: Bounds<Pixels>) -> f32 {
    f32::from(rect.origin.x + rect.size.width)
}

fn bottom(rect: Bounds<Pixels>) -> f32 {
    f32::from(rect.origin.y + rect.size.height)
}

fn assert_nested_center(
    window: AnyWindowHandle,
    left_name: &str,
    center_name: &str,
) -> Bounds<Pixels> {
    let panel = bounds(window, "bottom-panel");
    let left = bounds(window, left_name);
    let center = bounds(window, center_name);
    assert!(
        near(bottom(left), bottom(panel)),
        "{left_name} stops above the bottom panel: {left:?} {panel:?}"
    );
    assert!(
        near(f32::from(panel.origin.x), f32::from(center.origin.x))
            && near(right(panel), right(center)),
        "panel is not beneath {center_name} alone: {panel:?} {center:?}"
    );
    assert!(
        bottom(center) <= f32::from(panel.origin.y) + 2.0,
        "{center_name} overlaps panel: {center:?} {panel:?}"
    );
    assert!(
        f32::from(panel.origin.x) >= right(left) - 2.0,
        "panel extends beneath {left_name}: {panel:?} {left:?}"
    );
    panel
}

pub fn scenario_bottom_panel_nested(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);

    // Opening PR mode closes the panel by default; explicitly reopen it to
    // exercise the center/left boundary even on the empty dashboard.
    app.update(cx, |app, cx| {
        app.show_pr_mode(cx);
        app.bottom_panel_open = true;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, window);
    assert_nested_center(window, "pr-mode-left-pane", "pr-mode-center-pane");

    app.update(cx, |app, cx| app.show_issues_mode(cx));
    cx.run_until_parked();
    draw(cx, window);
    assert_nested_center(window, "issue-mode-left-pane", "issue-mode-center-pane");

    app.update(cx, |app, cx| app.show_editor_mode(cx));
    cx.run_until_parked();
    draw(cx, window);
    let panel = bounds(window, "bottom-panel");
    let editor_root = bounds(window, "editor-workspace-slot");
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("Editor workspace opened");
    let (tree_width, hunks_width) = cx.read(|cx| {
        let view = editor.read(cx);
        (view.tree_w, view.hunks_w)
    });
    assert!(
        near(bottom(panel), bottom(editor_root)),
        "Editor pane should span the bottom panel: {panel:?} {editor_root:?}"
    );
    assert!(
        f32::from(panel.origin.x)
            >= f32::from(editor_root.origin.x) + tree_width * theme::zoom() - 2.0,
        "bottom panel extends under Editor file tree: {panel:?} {editor_root:?}"
    );
    assert!(
        right(panel) <= right(editor_root) - hunks_width * theme::zoom() + 2.0,
        "bottom panel extends under Editor hunks: {panel:?} {editor_root:?}"
    );

    // An Editor-only notification must rebuild the parent-provided panel;
    // the changed tree width should move its left edge, not make it vanish.
    editor.update(cx, |view, cx| {
        view.tree_w += 24.0;
        cx.notify();
    });
    draw(cx, window);
    let moved = bounds(window, "bottom-panel");
    assert!(
        f32::from(moved.origin.x) >= f32::from(panel.origin.x) + 24.0 * theme::zoom() - 2.0,
        "Editor-only rerender lost or misplaced the panel: {panel:?} {moved:?}"
    );
    assert!(
        near(right(moved), right(panel)) && near(bottom(moved), bottom(editor_root)),
        "Editor hunks and footer must stay beside the panel after resize: {moved:?}"
    );

    assert_eq!(
        before,
        repo_fingerprint(&repo),
        "layout scenario wrote to repo"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS bottom_panel_nested PR/Issues/Editor");
}
