//! #1227: captions and AX assignments follow language changes, not user data.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Msg},
    theme,
};
use std::{cell::RefCell, rc::Rc};

const TEMPLATE: &str =
    "# Commit Panel / Unstaged / Staged / List / Tree\n# Stage all / Unstage all\n";

pub fn scenario_commit_panel_i18n(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("localization.txt"), "staged\n").unwrap();
    git(fixture.path(), &["add", "localization.txt"]);
    std::fs::write(fixture.path().join("localization.txt"), "unstaged\n").unwrap();
    std::fs::write(fixture.path().join("message-template"), TEMPLATE).unwrap();
    git(
        fixture.path(),
        &["config", "commit.template", "message-template"],
    );
    for slug in ["apple-light", "apple-dark"] {
        for (width, height, zoom) in [(1392., 883., 1.), (940., 660., 1.67)] {
            theme::set_zoom(zoom);
            let state = e2e::app_state(fixture.path()).unwrap();
            let captured = Rc::new(RefCell::new(None));
            let output = captured.clone();
            let window =
                crate::macos::open_offscreen(cx, size(px(width), px(height)), move |window, cx| {
                    e2e::mount_root(state, window, cx, &output)
                });
            let app = captured.borrow().clone().unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    app.open_commit_panel(window, cx);
                });
            })
            .unwrap();
            check_languages(cx, &app, window.into(), (width, height), zoom);
            unmount(cx, app, window.into());
        }
    }
    eprintln!("[gui-e2e] PASS commit_panel_i18n");
}

fn check_languages(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    zoom: f32,
) {
    for locale in ["ja", "en", "ja", "en"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        panel.update(cx, |_, cx| cx.notify());
        for _ in 0..4 {
            cx.run_until_parked();
            draw(cx, window, dimensions);
        }
        check_captions(locale, zoom);
        let files = check_control_bounds(window, locale, zoom);
        for (id, staged, count) in [
            (
                "cp-unstaged-heading",
                false,
                cx.read(|cx| panel.read(cx).state.unstaged.len()),
            ),
            (
                "cp-staged-heading",
                true,
                cx.read(|cx| panel.read(cx).state.staged.len()),
            ),
        ] {
            assert_eq!(
                e2e::commit_panel_labels::rendered(id).unwrap().as_ref(),
                i18n::commit_panel_section(staged, count)
            );
            contained(
                files,
                e2e::control_bounds(window.window_id(), id).unwrap(),
                id,
            );
        }
        assert_eq!(
            cx.read(|cx| panel.read(cx).commit_template.clone())
                .as_deref(),
            Some(TEMPLATE)
        );
        // Compact captions include the pinned composer, but its Popover covers
        // the separate Changes headers. Dismiss through the real close button
        // before exercising the real List/Tree pointer target.
        if kagi_domain::compact::is_compact(
            dimensions.0,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        ) {
            let close = e2e::control_bounds(window.window_id(), "compact-popup-close").unwrap();
            cx.simulate_mouse_down(
                window,
                close.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::none(),
            );
            cx.simulate_mouse_up(
                window,
                close.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::none(),
            );
            cx.run_until_parked();
            draw(cx, window, dimensions);
        }
        let tree = e2e::control_bounds(window.window_id(), "cp-view-tree").unwrap();
        cx.simulate_mouse_down(
            window,
            tree.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        cx.simulate_mouse_up(
            window,
            tree.center(),
            gpui::MouseButton::Left,
            gpui::Modifiers::none(),
        );
        draw(cx, window, dimensions);
        assert!(cx.read(|cx| panel.read(cx).state.tree_view));
        assert_eq!(
            e2e::commit_panel_labels::accessible_selected("cp-view-tree"),
            Some(true)
        );
        assert_eq!(
            e2e::commit_panel_labels::accessible_selected("cp-view-list"),
            Some(false)
        );
        if kagi_domain::compact::is_compact(
            dimensions.0,
            theme::rem_size_px(),
            theme::compact_threshold_rem(),
        ) {
            // Real staged/unstaged fixture data keeps the WIP anchor reachable;
            // reopen the same owner for the next locale's composer captions.
            cx.simulate_keystrokes(window, "cmd-enter");
            for _ in 0..4 {
                cx.run_until_parked();
                draw(cx, window, dimensions);
            }
        }
        drop(panel);
    }
}

fn check_captions(locale: &str, zoom: f32) {
    for (id, msg) in [
        // Domain operation names intentionally remain identical in EN/JA.
        ("cp-view-list", Msg::CommitPanelList),
        ("cp-view-tree", Msg::CommitPanelTree),
        ("cp-stage-all", Msg::CommitPanelStageAll),
        ("cp-unstage-all", Msg::CommitPanelUnstageAll),
        ("cp-discard-all", Msg::CommitPanelDiscardAll),
    ] {
        let rendered = e2e::commit_panel_labels::rendered(id).unwrap();
        assert_eq!(
            rendered.as_ref(),
            msg.t(),
            "{locale}@{zoom} {id}: rendered label mismatch"
        );
        assert_eq!(
            e2e::commit_panel_labels::accessible_name(id).unwrap(),
            rendered
        );
        assert_eq!(
            e2e::commit_panel_labels::accessible_role(id),
            Some(if id == "cp-view-list" || id == "cp-view-tree" {
                gpui::Role::Tab
            } else {
                gpui::Role::Button
            })
        );
    }
    assert_eq!(
        e2e::commit_panel_labels::rendered("cp-title")
            .unwrap()
            .as_ref(),
        Msg::CommitPanelTitle.t()
    );
    for (id, msg) in [
        ("cp-row-stage", Msg::CommitPanelStage),
        ("cp-row-unstage", Msg::CommitPanelUnstage),
    ] {
        assert_eq!(
            e2e::commit_panel_labels::rendered(id).unwrap().as_ref(),
            msg.t()
        );
    }
}

fn check_control_bounds(
    window: AnyWindowHandle,
    locale: &str,
    zoom: f32,
) -> gpui::Bounds<gpui::Pixels> {
    let files = e2e::control_bounds(window.window_id(), "cp-files").unwrap();
    for id in [
        "cp-view-list",
        "cp-view-tree",
        "cp-stage-all",
        "cp-discard-all",
        "cp-unstage-all",
        "cp-row-stage",
        "cp-row-unstage",
    ] {
        let bounds = e2e::control_bounds(window.window_id(), id).unwrap();
        assert!(
            bounds.size.height >= px(20.),
            "{locale}@{zoom} {id} hit height: {bounds:?}"
        );
        assert!(
            bounds.left() >= files.left() && bounds.right() <= files.right(),
            "{id}: {bounds:?}"
        );
    }
    files
}
