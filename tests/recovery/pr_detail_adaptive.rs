//! #1118: companion panes may not spend the PR reader's minimum width.
use crate::macos::{build_fixture, git, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::pr_viewed::{pr_at, push_pr_head, wait_loaded};
use crate::recovery_layout::{contained, draw, GlobalSettings};
use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, VisualTestAppContext};
use kagi::ui::{e2e, pr_mode::PrView, KagiApp};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, rc::Rc};

fn bounds(win: AnyWindowHandle, name: &str) -> Bounds<gpui::Pixels> {
    e2e::control_bounds(win.window_id(), name).unwrap_or_else(|| panic!("missing {name}"))
}

pub(crate) fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    let wanted = size(px(dimensions.0), px(dimensions.1));
    cx.update_window(win, |_, w, _| w.resize(wanted)).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        // MacWindow::resize uses the native executor, not TestDispatcher.
        crate::macos::drain_native_events();
        cx.run_until_parked();
        let observed = cx.update_window(win, |_, w, _| w.viewport_size()).unwrap();
        if observed == wanted {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native resize did not reach {wanted:?}; actual viewport is {observed:?}"
        );
    }
}
fn geometry(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
    feed: bool,
) {
    let mut names = vec![
        "pr-mode-center-pane",
        "pr-view-overview",
        "pr-view-review",
        "pr-view-diff",
        "pr-mode-body",
        "pr-mode-home",
    ];
    if feed {
        names.push("pr-mode-composer");
    }
    let compact = kagi_domain::compact::is_compact(
        dimensions.0,
        theme::rem_size_px(),
        theme::compact_threshold_rem(),
    );
    names.push("pr-swimlane-toggle");
    e2e::clear_control_bounds(win.window_id(), "sidebar-toggle");
    if !compact {
        names.push("sidebar-toggle");
    }
    for name in &names {
        e2e::clear_control_bounds(win.window_id(), name);
    }
    for name in [
        "pr-mode-lane-pane",
        "pr-companion-drawer",
        "pr-companion-swimlane",
        "pr-companion-navigator",
        "pr-mode-left-pane",
    ] {
        e2e::clear_control_bounds(win.window_id(), name);
    }
    for _ in 0..3 {
        cx.run_until_parked();
        draw(cx, win, dimensions);
    }
    let viewport = Bounds::new(
        point(px(0.), px(0.)),
        size(px(dimensions.0), px(dimensions.1)),
    );
    for name in names {
        let b = bounds(win, name);
        assert!(
            b.size.width > px(0.) && b.size.height > px(0.),
            "empty {name}: {b:?}"
        );
        contained(viewport, b, name);
        if matches!(
            name,
            "pr-mode-center-pane" | "pr-mode-body" | "pr-mode-composer"
        ) {
            assert!(
                b.size.width + px(1.) >= theme::scaled_px(420.),
                "{name} below scaled reader minimum: {b:?}"
            );
        }
    }
}

fn companion_ax(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    use i18n::Msg;
    let (pane, drawer, show, hide) = if id == "pr-swimlane-toggle" {
        (
            "pr-mode-lane-pane",
            "pr-companion-swimlane",
            Msg::PrShowGraph,
            Msg::PrHideGraph,
        )
    } else {
        (
            "pr-mode-left-pane",
            "pr-companion-navigator",
            Msg::MenuShowSidebar,
            Msg::MenuHideSidebar,
        )
    };
    let visible = e2e::control_bounds(win.window_id(), pane).is_some()
        || e2e::control_bounds(win.window_id(), drawer).is_some();
    let node = crate::native_ax::node(cx, win, id.to_owned().into());
    assert_eq!(node["aria"]["role"], "Button");
    assert_eq!(
        node["aria"]["label"],
        if visible { hide.t() } else { show.t() }
    );
    assert_eq!(
        node["aria"]["toggled"],
        if visible { "True" } else { "False" }
    );
}
fn open(
    cx: &mut VisualTestAppContext,
    repo: &std::path::Path,
    dimensions: (f32, f32),
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let out = captured.clone();
    let win = crate::macos::open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |w, cx| e2e::mount_root(state, w, cx, &out),
    );
    let app = captured.borrow().clone().unwrap();
    cx.run_until_parked();
    (app, win.into())
}

fn key(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle, stroke: &str) {
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |a, cx| a.root_focus.as_ref().unwrap().focus(w, cx))
    })
    .unwrap();
    cx.simulate_keystrokes(win, stroke);
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    pr: kagi_domain::github::PrKey,
    file: Option<usize>,
    typed: String,
    parked: String,
    thread_path: String,
    opened_threads: Vec<usize>,
    top_item: usize,
    top_offset: gpui::Pixels,
    input: gpui::EntityId,
}

fn snapshot(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Snapshot {
    cx.read(|cx| {
        let a = app.read(cx);
        let m = a.pr_mode().unwrap();
        let t = &m.tabs[m.active.unwrap()];
        let (path, opened) = t.threads.opened_rows_for_e2e();
        let top = t.diff_scroll.logical_scroll_top();
        let input = a.pr_comment_input.as_ref().unwrap();
        Snapshot {
            pr: t.pr.key(),
            file: t.selected_file,
            typed: input.read(cx).value().to_string(),
            parked: t.comment_draft.clone(),
            thread_path: path.to_owned(),
            opened_threads: opened.iter().copied().collect(),
            top_item: top.item_ix,
            top_offset: top.offset_in_item,
            input: input.entity_id(),
        }
    })
}

fn retained(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
) {
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |a, cx| {
            let input = a.pr_comment_input.clone().unwrap();
            input.update(cx, |input, cx| {
                input.replace("unsent 日本語 draft".to_owned(), window, cx)
            });
        })
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |a, cx| a.pr_mode_toggle_thread(1, cx));
    geometry(cx, win, dimensions, false);
    assert!(bounds(win, "pr-thread-1-0").size.height > px(0.));
    let viewport = cx.read(|cx| {
        app.read(cx).pr_mode().unwrap().tabs[0]
            .diff_scroll
            .viewport_bounds()
    });
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-600.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    geometry(cx, win, dimensions, false);
    let before = snapshot(cx, app);
    assert!(
        !before.opened_threads.is_empty() && before.top_item > 0,
        "a real thread and nonzero scroll must be retained"
    );
    let compact = kagi_domain::compact::is_compact(
        dimensions.0,
        theme::rem_size_px(),
        theme::compact_threshold_rem(),
    );
    let toggles: &[&str] = if compact {
        &["pr-swimlane-toggle", "pr-swimlane-toggle"]
    } else {
        &[
            "pr-swimlane-toggle",
            "pr-swimlane-toggle",
            "sidebar-toggle",
            "sidebar-toggle",
        ]
    };
    crate::native_ax::activate(cx);
    geometry(cx, win, dimensions, false);
    for &id in toggles {
        companion_ax(cx, win, id);
        e2e::clear_control_bounds(win.window_id(), "pr-companion-drawer");
        let button = bounds(win, id);
        cx.simulate_click(win, button.center(), gpui::Modifiers::none());
        geometry(cx, win, dimensions, false);
        if let Some(drawer) = e2e::control_bounds(win.window_id(), "pr-companion-drawer") {
            contained(
                Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(dimensions.0), px(dimensions.1)),
                ),
                drawer,
                "revealed companion",
            );
        }
        assert_eq!(snapshot(cx, app), before, "toggle {id} resets PR state");
        companion_ax(cx, win, id);
    }
    if compact {
        assert!(
            e2e::control_bounds(win.window_id(), "sidebar-toggle").is_none(),
            "compact uses one navigator launcher, not a competing chrome toggle"
        );
        let launcher = bounds(win, "compact-rail-prs");
        cx.simulate_click(win, launcher.center(), gpui::Modifiers::none());
        geometry(cx, win, dimensions, false);
        bounds(win, "compact-sidebar-popover");
        cx.simulate_keystrokes(win, "escape");
        e2e::clear_control_bounds(win.window_id(), "compact-sidebar-popover");
        geometry(cx, win, dimensions, false);
        assert!(e2e::control_bounds(win.window_id(), "compact-sidebar-popover").is_none());
        assert_eq!(snapshot(cx, app), before, "rail reveal resets PR state");
    }
    // Restore the View preference through its real registered action.
    if !cx.read(|cx| app.read(cx).sidebar.visible) {
        cx.dispatch_action(win, kagi::ui::commands::ToggleSidebar);
        geometry(cx, win, dimensions, false);
    }
    if e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_some() {
        key(cx, app, win, "escape");
        geometry(cx, win, dimensions, false);
        assert!(e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_none());
        assert!(cx.read(|cx| app.read(cx).pr_mode().unwrap().active.is_some()));
        assert_eq!(
            snapshot(cx, app),
            before,
            "Escape closing a drawer resets PR state"
        );
    }
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |a, cx| {
            a.root_focus
                .as_ref()
                .expect("mounted root focus")
                .focus(w, cx)
        })
    })
    .unwrap();
    if dimensions.0 == 1392. && theme::zoom() < 1.1 {
        assert!(e2e::control_bounds(win.window_id(), "pr-mode-lane-pane").is_some());
        assert!(e2e::control_bounds(win.window_id(), "pr-mode-left-pane").is_some());
    }
    assert!(
        e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_none(),
        "keyboard reveal must start with no drawer"
    );
    let lane_before = e2e::control_bounds(win.window_id(), "pr-mode-lane-pane").is_some();
    e2e::clear_control_bounds(win.window_id(), "pr-mode-lane-pane");
    e2e::clear_control_bounds(win.window_id(), "pr-companion-drawer");
    cx.simulate_keystrokes(win, "cmd-alt-l");
    geometry(cx, win, dimensions, false);
    assert!(
        e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_some()
            || (lane_before && e2e::control_bounds(win.window_id(), "pr-mode-lane-pane").is_none()),
        "registered keyboard shortcut must actually toggle the companion"
    );
    if !lane_before {
        let drawer = bounds(win, "pr-companion-swimlane");
        assert!(
            e2e::control_bounds(win.window_id(), "pr-companion-navigator").is_none(),
            "keyboard opens the Swimlane, not Navigator"
        );
        contained(
            bounds(win, "pr-mode-center-pane"),
            drawer,
            "swimlane drawer anchored to center",
        );
        key(cx, app, win, "escape");
        geometry(cx, win, dimensions, false);
        assert!(e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_none());
        assert!(cx.read(|cx| app.read(cx).pr_mode().unwrap().active.is_some()));
        assert_eq!(
            snapshot(cx, app),
            before,
            "Escape dismisses only the drawer"
        );
        key(cx, app, win, "cmd-alt-l");
        geometry(cx, win, dimensions, false);
        assert!(e2e::control_bounds(win.window_id(), "pr-companion-swimlane").is_some());
    }
    assert_eq!(snapshot(cx, app), before, "keyboard reveal resets state");
    cx.simulate_keystrokes(win, "cmd-alt-l");
    geometry(cx, win, dimensions, false);
    assert_eq!(snapshot(cx, app), before, "keyboard close resets state");
    // Native resize, not a reconstructed entity or an injected layout flag.
    let resized = if kagi_domain::compact::is_compact(
        dimensions.0,
        theme::rem_size_px(),
        theme::compact_threshold_rem(),
    ) {
        (
            1392_f32.max((theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil()),
            883.,
        )
    } else {
        (940., 660.)
    };
    resize(cx, win, resized);
    geometry(cx, win, resized, false);
    assert_eq!(
        snapshot(cx, app),
        before,
        "native resize resets selection, draft, threads or scroll"
    );
    resize(cx, win, dimensions);
    geometry(cx, win, dimensions, false);
    assert_eq!(
        snapshot(cx, app),
        before,
        "resize round trip resets PR state"
    );
    // Preserve the original narrow desktop drawer pressure with a supported
    // threshold, without inventing an unreachable navigator preference.
    theme::set_zoom(1.1);
    let threshold = theme::compact_threshold_rem();
    theme::set_compact_threshold_rem(40);
    let desktop = (1000., 720.);
    resize(cx, win, desktop);
    geometry(cx, win, desktop, false);
    key(cx, app, win, "cmd-alt-l");
    geometry(cx, win, desktop, false);
    assert!(e2e::control_bounds(win.window_id(), "pr-companion-swimlane").is_some());
    resize(cx, win, (1392., 883.));
    geometry(cx, win, (1392., 883.), false);
    assert!(e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_none());
    resize(cx, win, desktop);
    geometry(cx, win, desktop, false);
    assert!(
        e2e::control_bounds(win.window_id(), "pr-companion-drawer").is_none(),
        "narrowing must not resurrect an inline reveal"
    );
    resize(cx, win, (1392., 883.));
    geometry(cx, win, (1392., 883.), false);
    theme::set_compact_threshold_rem(threshold);
}
pub fn scenario_pr_detail_adaptive(cx: &mut VisualTestAppContext) {
    run(cx, false);
}

/// #1172: dashboard inputs must not survive native resize and PR departure.
pub fn scenario_pr_detail_lifecycle(cx: &mut VisualTestAppContext) {
    let before = cx.read(|cx| cx.leak_detector_snapshot());
    run(cx, true);
    cx.update(|_| {});
    cx.run_until_parked();
    cx.read(|cx| cx.assert_no_new_leaks(&before));
}

fn run(cx: &mut VisualTestAppContext, lifecycle: bool) {
    let _restore = GlobalSettings::capture();
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    git(
        &repo,
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
    let text = "reader data\n".repeat(300);
    let head = push_pr_head(&repo, &remote, "main", &[("adaptive.txt", &text)]);
    for locale in ["en", "ja"] {
        std::env::set_var("KAGI_LANG", locale);
        i18n::init_lang();
        for (dimensions, zoom) in [
            ((940., 660.), 1.),
            ((940., 660.), 1.667),
            ((1392., 883.), 1.),
            ((1392., 883.), 1.667),
        ] {
            theme::set_zoom(zoom);
            let (app, win) = open(cx, &repo, dimensions);
            if lifecycle {
                e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(
                    crate::evidence_support::pr_page(vec![pr_at(&head)], "", None),
                )));
                app.update(cx, |a, cx| a.refresh_github_prs(cx));
                cx.run_until_parked();
                app.update(cx, |a, cx| a.show_pr_mode(cx));
                cx.run_until_parked();
                draw(cx, win, dimensions);
                assert!(bounds(win, "pr-home-row-7").size.height > px(0.));
            }
            e2e::queue_github_pr_conversation(gpui::Task::ready((
                Ok((vec![], vec![])),
                Ok(vec![kagi_domain::review_thread::ReviewThread {
                    path: "adaptive.txt".into(),
                    line: Some(1),
                    original_line: Some(1),
                    diff_side: kagi_domain::review_thread::DiffSide::Right,
                    comments: vec![kagi_domain::github::ReviewComment {
                        author: "alice".into(),
                        body: "Retain this opened thread".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }]),
            )));
            app.update(cx, |a, cx| a.pr_mode_open(&pr_at(&head), cx));
            wait_loaded(cx, &app, &head);
            if dimensions == (1392., 883.) && zoom == 1. {
                crate::toast_stack_budget::assert_nested_feedback_host(
                    cx,
                    &app,
                    win,
                    "pr-mode-center-pane",
                    true,
                );
            }
            for view in [PrView::Overview, PrView::Review, PrView::Diff] {
                app.update(cx, |a, cx| {
                    if view == PrView::Diff {
                        a.pr_mode_select_file(0, cx);
                    } else {
                        a.pr_mode_show(view, cx);
                    }
                });
                geometry(cx, win, dimensions, view != PrView::Diff);
                if dimensions.0 == 1392. && zoom == 1. {
                    assert!(
                        e2e::control_bounds(win.window_id(), "pr-mode-lane-pane").is_some(),
                        "desktop swimlane must be inline before any toggle"
                    );
                    assert!(
                        e2e::control_bounds(win.window_id(), "pr-mode-left-pane").is_some(),
                        "desktop navigator must be inline before any toggle"
                    );
                }
            }
            retained(cx, &app, win, dimensions);
            if lifecycle {
                // Change zoom with the same native window and input entities.
                let mut before = snapshot(cx, &app);
                theme::set_zoom(1.667);
                geometry(cx, win, (1392., 883.), false);
                let after = snapshot(cx, &app);
                // Zoom reflows the pixel offset within the same logical row.
                before.top_offset = after.top_offset;
                assert_eq!(after, before, "zoom resets PR state");
                key(cx, &app, win, "escape");
                cx.run_until_parked();
                draw(cx, win, (1392., 883.));
                assert!(cx.read(|cx| app.read(cx).pr_mode().unwrap().active.is_none()));
                // Home's pointer route is also part of the combined lifecycle.
                app.update(cx, |a, cx| a.pr_mode_open(&pr_at(&head), cx));
                geometry(cx, win, (1392., 883.), false);
                let home = bounds(win, "pr-mode-home");
                cx.simulate_click(win, home.center(), gpui::Modifiers::none());
                cx.run_until_parked();
                draw(cx, win, (1392., 883.));
                assert!(cx.read(|cx| app.read(cx).pr_mode().unwrap().active.is_none()));
            }
            unmount(cx, app, win);
        }
    }
}
