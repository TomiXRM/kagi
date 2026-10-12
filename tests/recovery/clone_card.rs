//! Clone confirmation viewport and complete destination identity (#1296/#1297).
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    button_style, e2e,
    home_github::CloneModal,
    i18n::{Lang, Msg},
    theme, KagiApp,
};
use kagi_git::{
    github_repos::RepoListing,
    ops::{plan_clone, CloneRequest},
};

fn listing() -> RepoListing {
    RepoListing {
        name_with_owner: "acme/widgets".into(),
        host: "github.com".into(),
        updated_at: String::new(),
        description: String::new(),
        is_private: false,
        is_fork: true,
    }
}

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, name: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), name).unwrap_or_else(|| panic!("missing {name}"))
}

fn inside(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) {
    let tolerance = gpui::px(1.);
    assert!(
        inner.left() >= outer.left() - tolerance
            && inner.right() <= outer.right() + tolerance
            && inner.top() >= outer.top() - tolerance
            && inner.bottom() <= outer.bottom() + tolerance,
        "must fit: {inner:?} inside {outer:?}"
    );
}

fn body_viewport(window: AnyWindowHandle) -> gpui::Bounds<gpui::Pixels> {
    // The inside probe moves with scroll content; its paint mask is the actual
    // stationary scrolling viewport, not its translated absolute-child bounds.
    e2e::control_paint(window.window_id(), "clone-body")
        .unwrap()
        .mask
}

fn wheel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, dy: f32) {
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: body_viewport(window).center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}

fn install_state(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    ready: &CloneModal,
    blocked: &CloneModal,
    state: &str,
    count: usize,
    lang: Lang,
) -> usize {
    let mut modal = if state == "blocked" {
        blocked.clone()
    } else {
        ready.clone()
    };
    // Long repository identities exercise the header without relying on remote gh.
    modal.listing.name_with_owner = format!(
        "acme/{}",
        if lang == Lang::En {
            "long-repository-name-".repeat(10)
        } else {
            "長い".repeat(100)
        }
    );
    let target = modal.target.as_mut().unwrap();
    target.plan.warnings = vec![target.plan.warnings[0].clone(); count];
    if state == "blocked" {
        target.plan.blockers = vec![target.plan.blockers[0].clone(); count.max(1)];
    }
    let notes = target.plan.warnings.len() + target.plan.blockers.len();
    if state == "empty" {
        modal.target = None;
    }
    if state == "running" {
        modal.started = Some(std::time::Instant::now());
    }
    app.update(cx, |app, _| app.set_clone_modal(modal));
    if state == "empty" {
        0
    } else {
        notes
    }
}

fn check_viewport(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    state: &str,
    notes: usize,
) {
    draw(cx, app, window);
    wheel(cx, window, 100_000.);
    draw(cx, app, window);
    let viewport = cx
        .update_window(window, |_, window, _| {
            gpui::Bounds::new(
                gpui::point(gpui::px(0.), gpui::px(0.)),
                window.viewport_size(),
            )
        })
        .unwrap();
    let card = bounds(window, "clone-card-bounds");
    inside(card, viewport);
    let actions: &[&str] = if state == "running" {
        &["clone-hide"]
    } else {
        &["clone-cancel", "clone-confirm"]
    };
    for action in actions {
        let action_bounds = bounds(window, action);
        inside(action_bounds, viewport);
        inside(
            action_bounds,
            e2e::control_paint(window.window_id(), action).unwrap().mask,
        );
        assert!(body_viewport(window).bottom() <= action_bounds.top());
    }
    if state != "running" {
        assert_eq!(
            button_style::recorded_modal_button("clone-confirm")
                .unwrap()
                .disabled,
            matches!(state, "empty" | "blocked")
        );
    }
    if notes > 0 {
        let last_id = format!("clone-note-{}", notes - 1);
        let pinned = bounds(window, actions[0]);
        wheel(cx, window, -100_000.);
        draw(cx, app, window);
        let last = e2e::control_paint(window.window_id(), &last_id).unwrap();
        inside(last.bounds, last.mask);
        inside(last.bounds, body_viewport(window));
        assert_eq!(
            bounds(window, actions[0]),
            pinned,
            "scroll moves only the body"
        );
    }
}

pub fn scenario_clone_card_viewport(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    let ready = app.update(cx, |app, cx| {
        app.open_clone_card(listing(), cx);
        app.replan_clone(parent.path(), cx);
        app.clone_modal().unwrap().clone()
    });
    let mut blocked = ready.clone();
    blocked.target.as_mut().unwrap().plan.blockers = plan_clone(&CloneRequest {
        source: "github.com/acme/widgets".into(),
        dest: fixture.path().to_owned(),
        is_fork: true,
    })
    .blockers;
    for dimensions in [(600., 660.), (940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| app.set_lang(lang, cx));
                    for state in ["empty", "ready", "blocked", "running"] {
                        for count in [0, 1, 20] {
                            let notes =
                                install_state(cx, &app, &ready, &blocked, state, count, lang);
                            check_viewport(cx, &app, window, state, notes);
                        }
                    }
                }
            }
        }
    }
    app.update(cx, |app, _| app.set_clone_modal(ready));
    draw(cx, &app, window);
    crate::app_conflict::click_control(cx, window, "clone-cancel");
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_none()));
    unmount(cx, app, window);
}

fn check_destination(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    chosen: &std::path::Path,
) {
    let dest = chosen.join("widgets").display().to_string();
    app.update(cx, |app, cx| app.replan_clone(chosen, cx));
    draw(cx, app, window);
    let presentation = e2e::clone_destination_presentation().unwrap();
    assert_eq!(
        presentation.displayed.as_deref(),
        Some(dest.as_str()),
        "show resolved destination, not only its parent"
    );
    assert_eq!(presentation.accessibility.as_deref(), Some(dest.as_str()));
    assert_eq!(presentation.tooltip.as_deref(), Some(dest.as_str()));
    assert_eq!(
        presentation.summary,
        Some(Msg::CloneWillCreate.t().replace("{}", &dest))
    );
    assert!(
        e2e::control_paint(window.window_id(), "clone-destination")
            .unwrap()
            .middle_ellipsis
    );
    let card = bounds(window, "clone-card-bounds");
    for action in [
        "clone-change-folder",
        "clone-confirm",
        "clone-copy-location",
    ] {
        inside(bounds(window, action), card);
    }
    assert_eq!(
        button_style::recorded_modal_button("clone-change-folder")
            .unwrap()
            .label,
        Msg::CloneChangeFolder.t()
    );
    assert_eq!(
        button_style::recorded_modal_button("clone-copy-location")
            .unwrap()
            .label,
        Msg::CloneCopyLocation.t()
    );
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).modal_focus.clone().unwrap();
        window.focus(&focus, cx);
    })
    .unwrap();
    e2e::take_clone_copy_activations();
    // Tab to Change, then Copy Location. Button's own keyboard-click path is used.
    crate::keyboard_nav::keys(cx, window, "tab tab enter");
    assert_eq!(
        button_style::modal_focus_probe(),
        Some("clone-copy-location")
    );
    assert_eq!(e2e::take_clone_copy_activations(), 1);
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(dest.clone())
    );
    assert!(cx.read(|cx| app.read(cx).clone_modal().unwrap().started.is_none()));
    crate::keyboard_nav::keys(cx, window, "space");
    assert_eq!(e2e::take_clone_copy_activations(), 1);
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(dest)
    );
}

pub fn scenario_clone_card_destination(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.open_clone_card(listing(), cx));
    let paths = [
        "normal".to_string(),
        format!("{}甲📁", "same-prefix-".repeat(18)),
        format!("{}乙📁", "same-prefix-".repeat(18)),
    ]
    .map(|s| parent.path().join(s));
    for path in &paths {
        std::fs::create_dir_all(path).unwrap();
    }
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| app.set_lang(lang, cx));
                    for path in &paths {
                        check_destination(cx, &app, window, path);
                    }
                }
            }
        }
    }
    unmount(cx, app, window);
}
