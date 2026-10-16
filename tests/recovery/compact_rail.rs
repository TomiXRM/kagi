//! #1491–#1494/#1502: native keys, AccessKit nodes and real scroll owners.
#[path = "compact_rail/escape.rs"]
pub mod escape;
#[path = "compact_rail/scroll.rs"]
pub mod scroll;

use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::contained;
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Focusable, Modifiers, Pixels, ScrollDelta,
    ScrollWheelEvent, TouchPhase, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Msg},
    settings, theme,
};
use serde_json::Value;
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

const TRIGGERS: [&str; 7] = [
    "compact-rail-local",
    "compact-rail-remote",
    "compact-rail-worktrees",
    "compact-rail-tags",
    "compact-rail-stashes",
    "compact-rail-prs",
    "compact-rail-issues",
];
const LABELS: [Msg; 7] = [
    Msg::A11ySidebarLocal,
    Msg::A11ySidebarRemote,
    Msg::A11ySidebarWorktrees,
    Msg::A11ySidebarTags,
    Msg::A11ySidebarStashes,
    Msg::WorkspacePrs,
    Msg::WorkspaceIssues,
];
const POPUP: &str = "compact-sidebar-popover";

#[derive(Clone, Copy, Debug)]
struct Case {
    width: f32,
    height: f32,
    zoom: f32,
    locale: &'static str,
    preset: &'static str,
}
impl Case {
    fn viewport(self) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(self.width), px(self.height)))
    }
}
fn matrix(mut run: impl FnMut(Case)) {
    for zoom in [1., 1.67] {
        for locale in ["en", "ja"] {
            for preset in ["apple-light", "apple-dark"] {
                run(Case {
                    width: 940.,
                    height: 660.,
                    zoom,
                    locale,
                    preset,
                });
            }
        }
    }
}
fn observed() -> Vec<String> {
    let mut ids = [
        "compact-rail",
        "compact-rail-scroll",
        POPUP,
        "sidebar-filter",
        "sidebar-local-main",
        "sidebar-local-rail-target",
        "pr-sidebar-list-viewport",
        "compact-pr-list-scrollbar",
        "list-filter-text",
        "pr-mode-card-7",
        "pr-mode-card-39",
        "issue-mode-card-1",
        "issue-mode-card-2",
        "issue-mode-card-3",
        "issue-mode-card-4",
    ]
    .map(str::to_owned)
    .to_vec();
    ids.extend(TRIGGERS.map(str::to_owned));
    for n in 0..4 {
        ids.push(format!("pr-mode-section-{n}"));
        ids.push(format!("issue-filter-tab-{n}"));
    }
    ids
}
fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    for _ in 0..4 {
        for id in observed() {
            e2e::clear_control_bounds(win.window_id(), &id);
        }
        e2e::compact_rail::clear(win.window_id());
        cx.run_until_parked();
        cx.update_window(win, |_, w, cx| {
            w.refresh();
            w.draw(cx).clear();
        })
        .unwrap();
    }
}
fn bounds(win: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("missing rendered {id}"))
}
fn press(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
    settle(cx, win);
}
fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let target = bounds(win, id);
    cx.simulate_mouse_move(win, target.center(), None, Modifiers::none());
    cx.simulate_click(win, target.center(), Modifiers::none());
    settle(cx, win);
}
fn root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, w, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        focus.focus(w, cx);
    })
    .unwrap();
    settle(cx, win);
}
fn wheel(cx: &mut VisualTestAppContext, win: AnyWindowHandle, area: Bounds<Pixels>, dy: f32) {
    cx.simulate_event(
        win,
        ScrollWheelEvent {
            position: area.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        },
    );
    settle(cx, win);
}
fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, case: Case) {
    let wanted = size(px(case.width), px(case.height));
    cx.update_window(win, |_, w, _| w.resize(wanted)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx.update_window(win, |_, w, _| w.viewport_size()).unwrap() == wanted {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native resize reaches {wanted:?}"
        );
    }
    settle(cx, win);
}
use crate::native_ax::{activate as activate_ax, tree};
fn ax_element(id: &str) -> gpui::ElementId {
    if let Some(index) = id.strip_prefix("pr-mode-section-") {
        ("pr-mode-section", index.parse::<usize>().unwrap()).into()
    } else if let Some(index) = id.strip_prefix("issue-filter-tab-") {
        ("issue-filter-tab", index.parse::<usize>().unwrap()).into()
    } else {
        id.to_owned().into()
    }
}
fn ax_identity(id: &str) -> String {
    format!("{:?}", ax_element(id))
}
fn node(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) -> Value {
    crate::native_ax::node(cx, win, ax_element(id))
}
fn focused(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) -> bool {
    let tree = tree(cx, win);
    let Some(key) = tree["gpui_focus"].as_str() else {
        return false;
    };
    tree["nodes"][key]["element_id"].as_str() == Some(ax_identity(id).as_str())
}
fn tab_to(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str, reverse: bool) {
    for _ in 0..160 {
        press(cx, win, if reverse { "shift-tab" } else { "tab" });
        if focused(cx, win, id) {
            return;
        }
    }
    panic!(
        "actual {} cannot reach {id}",
        if reverse { "Shift-Tab" } else { "Tab" }
    );
}
fn focus_ring(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let area = bounds(win, id);
    cx.update_window(win, |_, w, _| {
        let image = w.render_to_image().expect("native keyboard focus ring");
        let scale = w.scale_factor();
        let accent = theme::theme().ui_accent();
        let expected = [(accent >> 16) as u8, (accent >> 8) as u8, accent as u8];
        // measure_inside starts at the parent's inner border edge. Inspect
        // the fixed 2px boundary, not a single content-column pixel.
        let left = ((f32::from(area.left()) - 2.).max(0.) * scale).floor() as u32;
        let right = ((f32::from(area.left()) + 2.) * scale).ceil() as u32;
        let top = ((f32::from(area.top()) + 3.) * scale).ceil() as u32;
        let bottom = ((f32::from(area.bottom()) - 3.) * scale).floor() as u32;
        let painted = (top..bottom)
            .filter(|&y| {
                (left..right).any(|x| {
                    image.get_pixel(x, y).0[..3]
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(expected) <= 10)
                })
            })
            .count();
        assert!(
            painted >= 3,
            "{id}: visible native accent border, found {painted} pixels"
        );
    })
    .unwrap();
}
fn mount(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    case: Case,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    std::env::set_var("KAGI_LANG", case.locale);
    i18n::init_lang();
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).unwrap();
    theme::set_zoom(case.zoom);
    theme::set_active(case.preset);
    cx.update(theme::sync_gpui_component_theme);
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let win =
        crate::macos::open_offscreen(cx, size(px(case.width), px(case.height)), move |w, cx| {
            e2e::mount_root(state, w, cx, &output)
        })
        .into();
    let app = captured.borrow_mut().take().unwrap();
    activate_ax(cx);
    settle(cx, win);
    contained(
        case.viewport(),
        bounds(win, "compact-rail"),
        "real rail viewport",
    );
    assert!((theme::zoom() - case.zoom).abs() < 0.001);
    assert_eq!(theme::theme().slug.as_ref(), case.preset);
    (app, win)
}
fn closed(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    assert!(
        e2e::control_bounds(win.window_id(), POPUP).is_none(),
        "no hidden navigator"
    );
    for (id, label) in TRIGGERS.into_iter().zip(LABELS) {
        let aria = node(cx, win, id)["aria"].clone();
        assert_eq!(aria["role"], "Button");
        assert_eq!(aria["label"], label.t());
        assert_eq!(aria["expanded"], false, "{id}: closed native AX state");
    }
}
fn one_open(cx: &mut VisualTestAppContext, win: AnyWindowHandle, active: usize, case: Case) {
    contained(case.viewport(), bounds(win, POPUP), "popover containment");
    for (index, id) in TRIGGERS.into_iter().enumerate() {
        assert_eq!(
            node(cx, win, id)["aria"]["expanded"],
            index == active,
            "{id}: exactly one actual expanded node"
        );
    }
}
fn seed_prs(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, count: u64) {
    app.update(cx, |app, cx| {
        app.github_host_logins
            .insert(Some("github.com".into()), "alice".into());
        let ui = app.ui_mut().unwrap();
        ui.github_prs = (7..7 + count)
            .map(|n| {
                let mut pr = crate::evidence_support::pull_request(
                    n,
                    &format!("rail PR {n}"),
                    &format!("rail-{n}"),
                );
                // Mine, without attention: Inbox must be empty in this fixture.
                pr.review = kagi_domain::github::ReviewState::None;
                pr.mergeable = kagi_domain::github::Mergeable::Unknown;
                pr
            })
            .collect();
        ui.github_prs_loaded = true;
        ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
        for number in 7..7 + count {
            app.seed_pr_status_fresh_for_e2e(number, cx);
        }
        cx.notify();
    });
}

pub fn scenario_compact_rail_cmd_f(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "rail-target"]);
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        // mount_root provisions an Input for ordinary fixtures; retire it to
        // exercise the production first-shortcut lifecycle as well.
        app.update(cx, |app, cx| {
            app.sidebar.filter = None;
            cx.notify();
        });
        settle(cx, win);
        assert!(
            cx.read(|cx| app.read(cx).sidebar.filter.is_none()),
            "first filter is genuinely lazy"
        );
        root(cx, &app, win);
        let mut identity = None;
        for attempt in 0..2 {
            press(cx, win, "cmd-f");
            contained(
                case.viewport(),
                bounds(win, POPUP),
                "Cmd-F opens visible navigator",
            );
            contained(
                bounds(win, POPUP),
                bounds(win, "sidebar-filter"),
                "drawn filter",
            );
            let filter = cx
                .read(|cx| app.read(cx).sidebar.filter.clone())
                .expect("existing sidebar Input");
            assert!(cx
                .update_window(win, |_, w, cx| filter
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w))
                .unwrap());
            if let Some(previous) = identity {
                assert_eq!(filter.entity_id(), previous);
            }
            identity = Some(filter.entity_id());
            cx.simulate_input(win, "rail-target");
            settle(cx, win);
            bounds(win, "sidebar-local-rail-target");
            assert!(
                e2e::control_bounds(win.window_id(), "sidebar-local-main").is_none(),
                "typing narrows rendered leaves"
            );
            press(cx, win, "escape");
            assert!(e2e::control_bounds(win.window_id(), POPUP).is_none());
            assert!(
                !cx.update_window(win, |_, w, cx| filter
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(w))
                    .unwrap(),
                "Escape cannot retain invisible Input focus"
            );
            if attempt == 0 {
                // Clear through the same rendered field before the closed/reopen leg.
                press(cx, win, "cmd-f");
                cx.simulate_keystrokes(win, "cmd-a backspace");
                settle(cx, win);
                press(cx, win, "escape");
            }
            drop(filter);
        }
        resize(
            cx,
            win,
            Case {
                width: (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
                ..case
            },
        );
        press(cx, win, "cmd-f");
        assert!(e2e::control_bounds(win.window_id(), "compact-rail").is_none());
        let filter = cx.read(|cx| app.read(cx).sidebar.filter.clone()).unwrap();
        assert_eq!(
            Some(filter.entity_id()),
            identity,
            "wide route reuses identical Input entity"
        );
        assert_eq!(
            cx.read(|cx| filter.read(cx).value().to_string()),
            "rail-target"
        );
        assert!(cx
            .update_window(win, |_, w, cx| filter
                .read(cx)
                .focus_handle(cx)
                .is_focused(w))
            .unwrap());
        bounds(win, "sidebar-local-rail-target");
        drop(filter);
        unmount(cx, app, win);
    });
}

fn short_reach(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    case: Case,
) {
    for reverse in [false, true] {
        root(cx, app, win);
        let indices: Vec<usize> = if reverse {
            (0..7).rev().collect()
        } else {
            (0..7).collect()
        };
        for index in indices {
            tab_to(cx, win, TRIGGERS[index], reverse);
            contained(
                bounds(win, "compact-rail"),
                bounds(win, TRIGGERS[index]),
                "focused trigger reveals within rail",
            );
            contained(
                case.viewport(),
                bounds(win, TRIGGERS[index]),
                "focused trigger reveals within window",
            );
            press(cx, win, if reverse { "space" } else { "enter" });
            contained(
                case.viewport(),
                bounds(win, POPUP),
                "keyboard opens short-window navigator",
            );
            press(cx, win, "escape");
            assert!(
                e2e::control_bounds(win.window_id(), POPUP).is_none(),
                "Escape closes short-window navigator"
            );
        }
    }
    // A focused trigger must not pin subsequent pointer scrolling to itself.
    root(cx, app, win);
    tab_to(cx, win, TRIGGERS[0], false);
    let rail = bounds(win, "compact-rail");
    wheel(cx, win, rail, -10000.);
    contained(
        rail,
        bounds(win, TRIGGERS[6]),
        "wheel reaches Issues while Local retains keyboard focus",
    );
    let local_top = bounds(win, TRIGGERS[0]).top();
    assert!(
        local_top < rail.top(),
        "Local scrolls outside the short rail"
    );
    for _ in 0..3 {
        settle(cx, win);
        assert_eq!(
            bounds(win, TRIGGERS[0]).top(),
            local_top,
            "focused Local does not reset the pointer scroll offset",
        );
    }
    for id in TRIGGERS {
        let rail = bounds(win, "compact-rail");
        // Actual wheel/pointer route, independently of focus-driven reveal.
        wheel(cx, win, rail, 10000.);
        for _ in 0..32 {
            if let Some(target) = e2e::control_bounds(win.window_id(), id) {
                if target.top() >= rail.top() && target.bottom() <= rail.bottom() {
                    break;
                }
            }
            wheel(cx, win, rail, -24.);
        }
        contained(rail, bounds(win, id), "wheel reaches each pointer target");
        click(cx, win, id);
        contained(
            case.viewport(),
            bounds(win, POPUP),
            "pointer opens short-window navigator",
        );
        press(cx, win, "escape");
    }
}
pub fn scenario_compact_rail_short_window(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_size", "window_frame"]);
    let fixture = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    for (width, height, zoom) in [(940., 400., 1.67), (400., 300., 1.)] {
        let case = Case {
            width,
            height,
            zoom,
            locale: "ja",
            preset: "apple-light",
        };
        let (app, win) = mount(cx, fixture.path(), case);
        short_reach(cx, &app, win, case);
        unmount(cx, app, win);
        // Exercise the production legacy-size restore decision, then create a
        // second real native window at the resulting dimensions.
        settings::write_setting("window_frame", None);
        settings::write_setting("window_size", Some(&format!("{width}x{height}")));
        let no_restore = std::env::var_os("KAGI_NO_RESTORE");
        let override_size = std::env::var_os("KAGI_WINDOW");
        std::env::remove_var("KAGI_NO_RESTORE");
        std::env::remove_var("KAGI_WINDOW");
        let options = cx.read(kagi::ui::window_restore::options);
        if let Some(value) = no_restore {
            std::env::set_var("KAGI_NO_RESTORE", value);
        }
        if let Some(value) = override_size {
            std::env::set_var("KAGI_WINDOW", value);
        }
        let gpui::WindowBounds::Windowed(restored) = options.window_bounds.unwrap() else {
            panic!("windowed restore");
        };
        assert_eq!(
            restored.size,
            size(px(width).max(px(940.)), px(height).max(px(660.))),
            "production clamps unsupported short sizes to the repository minimum"
        );
        let restored_case = Case {
            width: f32::from(restored.size.width),
            height: f32::from(restored.size.height),
            ..case
        };
        let (app, win) = mount(cx, fixture.path(), restored_case);
        for id in TRIGGERS {
            contained(
                bounds(win, "compact-rail"),
                bounds(win, id),
                "supported restored rail",
            );
        }
        let threshold = (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil();
        for width in [999., 1000., threshold - 1., threshold, 940.] {
            resize(
                cx,
                win,
                Case {
                    width,
                    height: 660.,
                    zoom,
                    ..case
                },
            );
            assert_eq!(
                e2e::control_bounds(win.window_id(), "compact-rail").is_some(),
                kagi_domain::compact::is_compact(
                    width,
                    theme::rem_size_px(),
                    theme::compact_threshold_rem()
                )
            );
        }
        unmount(cx, app, win);
    }
}

fn section_ax(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
    label: &str,
    expanded: bool,
    selected: bool,
) {
    let aria = node(cx, win, id)["aria"].clone();
    assert_eq!(aria["role"], "Button");
    assert_eq!(aria["label"], label);
    assert_eq!(aria["expanded"], expanded);
    if id.starts_with("issue-") {
        assert_eq!(aria["selected"], selected);
    }
}
pub fn scenario_compact_popover_sections(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        seed_prs(cx, &app, 1);
        click(cx, win, "compact-rail-prs");
        for width in [
            940.,
            (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
        ] {
            if width != 940. {
                resize(cx, win, Case { width, ..case });
            }
            root(cx, &app, win);
            tab_to(cx, win, "pr-mode-section-1", false);
            section_ax(
                cx,
                win,
                "pr-mode-section-1",
                Msg::PrSectionMine.t(),
                false,
                false,
            );
            assert!(
                e2e::control_bounds(win.window_id(), "pr-mode-card-7").is_none(),
                "Mine starts folded and Inbox empty"
            );
            press(cx, win, "enter");
            section_ax(
                cx,
                win,
                "pr-mode-section-1",
                Msg::PrSectionMine.t(),
                true,
                false,
            );
            assert!(
                focused(cx, win, "pr-mode-section-1"),
                "disclosure retains visible focus"
            );
            focus_ring(cx, win, "pr-mode-section-1");
            bounds(win, "pr-mode-card-7");
            assert!(cx.read(|cx| app.read(cx).pr_mode().unwrap().sections_open[1]));
            press(cx, win, "space");
            assert!(!cx.read(|cx| app.read(cx).pr_mode().unwrap().sections_open[1]));
            assert!(e2e::control_bounds(win.window_id(), "pr-mode-card-7").is_none());
        }
        resize(cx, win, case);
        click(cx, win, "compact-rail-prs");
        tab_to(cx, win, "pr-mode-section-1", false);
        press(cx, win, "enter");
        // One Tab from Mine reaches the real compact leaf and its callback.
        press(cx, win, "tab");
        press(cx, win, "enter");
        assert!(
            cx.read(|cx| app
                .read(cx)
                .pr_mode()
                .unwrap()
                .tabs
                .iter()
                .any(|tab| tab.pr.number == 7)),
            "keyboard opens production PR tab"
        );
        assert!(e2e::control_bounds(win.window_id(), POPUP).is_none());
        app.update(cx, |app, cx| app.seed_issue_navigation_for_e2e(cx));
        settle(cx, win);
        click(cx, win, "compact-rail-issues");
        let labels = [
            Msg::IssuesAssignedToMe,
            Msg::IssuesCreatedByMe,
            Msg::IssuesMentioningMe,
            Msg::IssuesRecentlyUpdated,
        ];
        for width in [
            940.,
            (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
        ] {
            if width != 940. {
                resize(cx, win, Case { width, ..case });
            }
            for tab in kagi_domain::github::IssueListTab::ALL {
                let id = format!("issue-filter-tab-{}", tab.index());
                tab_to(cx, win, &id, false);
                press(
                    cx,
                    win,
                    if tab.index() % 2 == 0 {
                        "enter"
                    } else {
                        "space"
                    },
                );
                assert_eq!(cx.read(|cx| app.read(cx).ui().github_issue_tab), tab);
                assert!(
                    focused(cx, win, &id),
                    "category callback retains actual header focus"
                );
                focus_ring(cx, win, &id);
                for other in kagi_domain::github::IssueListTab::ALL {
                    section_ax(
                        cx,
                        win,
                        &format!("issue-filter-tab-{}", other.index()),
                        labels[other.index()].t(),
                        other == tab,
                        other == tab,
                    );
                }
                let expected: &[u64] = match tab.index() {
                    0 => &[1],
                    1 => &[2],
                    2 => &[3],
                    _ => &[1, 2, 3, 4],
                };
                for number in 1..=4 {
                    assert_eq!(
                        e2e::control_bounds(win.window_id(), &format!("issue-mode-card-{number}"))
                            .is_some(),
                        expected.contains(&number),
                        "category renders only matching real rows"
                    );
                }
            }
        }
        unmount(cx, app, win);
    });
}

pub fn scenario_compact_rail_ax(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let second = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        closed(cx, win);
        for (index, id) in TRIGGERS.into_iter().enumerate() {
            for key in ["enter", "space"] {
                root(cx, &app, win);
                tab_to(cx, win, id, false);
                press(cx, win, key);
                one_open(cx, win, index, case);
                tab_to(cx, win, id, true);
                press(cx, win, key);
                closed(cx, win);
            }
            click(cx, win, id);
            one_open(cx, win, index, case);
            press(cx, win, "escape");
            closed(cx, win);
            click(cx, win, id);
            cx.simulate_click(
                win,
                point(px(case.width - 2.), px(case.height / 2.)),
                Modifiers::none(),
            );
            settle(cx, win);
            closed(cx, win);
        }
        click(cx, win, TRIGGERS[0]);
        click(cx, win, TRIGGERS[1]);
        one_open(cx, win, 1, case);
        resize(
            cx,
            win,
            Case {
                width: (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
                ..case
            },
        );
        resize(cx, win, case);
        closed(cx, win);
        click(cx, win, TRIGGERS[0]);
        app.update(cx, |app, cx| {
            app.open_repository(second.path().to_path_buf(), cx)
        });
        settle(cx, win);
        closed(cx, win);
        click(cx, win, TRIGGERS[0]);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.handle_menu_command("branch.new", window, cx)
            });
        })
        .unwrap();
        settle(cx, win);
        assert!(
            cx.read(|cx| app.read(cx).active_modal.is_some()),
            "production new-branch command opens modal"
        );
        assert!(e2e::control_bounds(win.window_id(), POPUP).is_none());
        press(cx, win, "escape");
        closed(cx, win);
        unmount(cx, app, win);
    });
}
