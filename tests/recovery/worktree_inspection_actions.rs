//! #1338: renderer-fed names/focus, real keys and the inspection card's lifetime.
use crate::{
    macos::{build_fixture, git, open_offscreen, unmount},
    recovery_layout::{contained, draw, GlobalSettings},
};
use e2e::worktree_inspection as probes;
use gpui::{
    px, size, AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton, Pixels, Role,
    VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{
    i18n::{self, Msg},
    theme,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

const REFRESH: &str = "worktree-inspection-refresh";
const URL: &str = "worktree-inspection-port";
const CARD: &str = "worktree-inspection";

#[derive(Clone, Copy)]
struct Case {
    dimensions: (f32, f32),
    zoom: f32,
    theme: &'static str,
    locale: &'static str,
}

fn fixture() -> (tempfile::TempDir, PathBuf, u16) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for name in ["with-port", "no-port"] {
        let path = repo.join(name);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                name,
                path.to_str().unwrap(),
                "HEAD",
            ],
        );
    }
    let port = kagi_git::worktree_ports::assign_block(
        &repo.join("with-port"),
        kagi_domain::worktree_ports::PortRange {
            start: 3000,
            end: 3099,
        },
        10,
    )
    .unwrap();
    (fixture, repo, port)
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle, case: Case) {
    cx.run_until_parked();
    probes::clear_actions();
    for id in [CARD, REFRESH, URL, "sidebar-worktree-keyboard-card"] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    draw(cx, window, case.dimensions);
}

fn press(cx: &mut VisualTestAppContext, window: AnyWindowHandle, case: Case, key: &str) {
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(window, keystroke.clone());
    cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
    paint(cx, window, case);
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} not drawn"))
}

fn assert_focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let probe = probes::action(id).unwrap_or_else(|| panic!("{id} not rendered"));
    assert!(
        cx.update_window(window, |_, window, _| {
            probe
                .focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap(),
        "{id} must have real keyboard focus"
    );
}

fn focus_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    case: Case,
    path: &Path,
) {
    let key = format!("worktree:{}", path.display());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(2, &key, window, cx)
        });
    })
    .unwrap();
    paint(cx, window, case);
    assert_row_focus(cx, app, window, path);
}

fn assert_row_focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    path: &Path,
) {
    assert_eq!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).sidebar_row_focused_for_e2e(window)
        })
        .unwrap(),
        Some((2, format!("worktree:{}", path.display())))
    );
}

fn open(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    case: Case,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    theme::set_zoom(case.zoom);
    std::env::set_var("KAGI_LANG", case.locale);
    i18n::init_lang();
    let state = e2e::app_state(repo).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let window = open_offscreen(
        cx,
        size(px(case.dimensions.0), px(case.dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    )
    .into();
    let app = captured.borrow().clone().unwrap();
    app.update(cx, |app: &mut KagiApp, cx| {
        app.set_theme(case.theme, cx);
        app.sidebar.collapsed.remove("worktrees");
        app.sidebar.pane_weights = [1, 1, 8, 1, 1];
        cx.notify();
    });
    for _ in 0..3 {
        paint(cx, window, case);
    }
    (app, window)
}

fn keyboard_actions(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    case: Case,
    path: &Path,
    port: u16,
) {
    focus_row(cx, app, window, case, path);
    press(cx, window, case, "enter");
    assert_row_focus(cx, app, window, path);
    assert_eq!(
        cx.read(|cx| app.read(cx).sidebar_keyboard_card_for_e2e()),
        Some(path.to_path_buf())
    );
    let card = bounds(window, CARD);
    contained(
        Bounds::new(
            gpui::point(px(0.), px(0.)),
            size(px(case.dimensions.0), px(case.dimensions.1)),
        ),
        card,
        "inspection card",
    );
    let refresh = probes::action(REFRESH).expect("Refresh rendered");
    assert_eq!(refresh.role, Role::Button);
    assert_eq!(refresh.name.as_ref(), Msg::WorktreeInspectionRefresh.t());
    let url = probes::action(URL).expect("URL rendered");
    assert_eq!(url.role, Role::Button);
    let expected_url_name = if case.locale == "ja" {
        format!("http://localhost:{port} をブラウザで開く")
    } else {
        format!("Open http://localhost:{port} in browser")
    };
    assert_eq!(url.name.as_ref(), expected_url_name.as_str());
    contained(card, bounds(window, REFRESH), "Refresh");
    contained(card, bounds(window, URL), "URL");
    press(cx, window, case, "tab");
    assert_focus(cx, window, REFRESH);
    cx.advance_clock(Duration::from_secs(1));
    paint(cx, window, case);
    assert_focus(cx, window, REFRESH);
    for key in ["enter", "space"] {
        probes::take_refresh_activations();
        press(cx, window, case, key);
        assert_eq!(
            probes::take_refresh_activations(),
            1,
            "{key} refreshes exactly once"
        );
        assert_focus(cx, window, REFRESH);
        bounds(window, CARD);
    }
    press(cx, window, case, "tab");
    assert_focus(cx, window, URL);
    for key in ["enter", "space"] {
        e2e::take_opened_urls();
        press(cx, window, case, key);
        assert_eq!(
            e2e::take_opened_urls(),
            [format!("http://localhost:{port}")],
            "{key} opens exactly once"
        );
        assert_focus(cx, window, URL);
        bounds(window, CARD);
    }
    press(cx, window, case, "shift-tab");
    assert_focus(cx, window, REFRESH);
    press(cx, window, case, "shift-tab");
    assert_row_focus(cx, app, window, path);
    bounds(window, CARD);
    press(cx, window, case, "tab");
    assert_focus(cx, window, REFRESH);
    press(cx, window, case, "tab");
    assert_focus(cx, window, URL);
    press(cx, window, case, "escape");
    assert!(e2e::control_bounds(window.window_id(), CARD).is_none());
    assert_row_focus(cx, app, window, path);
}

fn no_port(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    case: Case,
    path: &Path,
) {
    focus_row(cx, app, window, case, path);
    press(cx, window, case, "enter");
    assert!(
        probes::action(URL).is_none(),
        "unassigned worktree has no URL action"
    );
    assert!(e2e::control_bounds(window.window_id(), URL).is_none());
    press(cx, window, case, "tab");
    assert_focus(cx, window, REFRESH);
    press(cx, window, case, "tab");
    assert!(
        e2e::control_bounds(window.window_id(), CARD).is_none(),
        "Tab leaves no-port card after Refresh"
    );
    assert!(e2e::take_opened_urls().is_empty());
}

fn owner_switch(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    case: Case,
    path: &Path,
    other: &Path,
) {
    focus_row(cx, app, window, case, path);
    press(cx, window, case, "enter");
    press(cx, window, case, "tab");
    assert_focus(cx, window, REFRESH);
    let owner = cx.read(|cx| app.read(cx).active_tab);
    assert!(app.update(cx, |app, cx| app.open_repository(other.to_path_buf(), cx)));
    paint(cx, window, case);
    assert!(cx.read(|cx| app.read(cx).sidebar_keyboard_card_for_e2e().is_none()));
    assert!(e2e::control_bounds(window.window_id(), CARD).is_none());
    app.update(cx, |app, cx| app.switch_repo(owner, cx));
    paint(cx, window, case);
    assert!(
        e2e::control_bounds(window.window_id(), CARD).is_none(),
        "returning must not resurrect card"
    );
}

fn hover_actions(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    case: Case,
    port: u16,
) {
    cx.update_window(window, |_, window, cx| {
        let root = app.read(cx).root_focus.clone().unwrap();
        root.focus(window, cx);
    })
    .unwrap();
    paint(cx, window, case);
    let row = bounds(window, "sidebar-worktree-with-port");
    cx.simulate_mouse_move(
        window,
        gpui::point(row.origin.x + px(4.), row.center().y),
        None,
        Modifiers::none(),
    );
    cx.run_until_parked();
    cx.advance_clock(Duration::from_secs(1));
    paint(cx, window, case);
    bounds(window, CARD);
    for id in [REFRESH, URL] {
        let target = bounds(window, id).center();
        cx.simulate_mouse_move(window, target, None, Modifiers::none());
        cx.run_until_parked();
        probes::take_refresh_activations();
        e2e::take_opened_urls();
        cx.simulate_mouse_down(window, target, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(window, target, MouseButton::Left, Modifiers::none());
        paint(cx, window, case);
        if id == REFRESH {
            assert_eq!(probes::take_refresh_activations(), 1);
        } else {
            assert_eq!(
                e2e::take_opened_urls(),
                [format!("http://localhost:{port}")]
            );
        }
    }
}

pub fn scenario(cx: &mut VisualTestAppContext) {
    let _settings = GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "sidebar_panes"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let original_theme = theme::theme().slug.to_string();
    let (_fixture, repo, port) = fixture();
    let other = build_fixture();
    for dimensions in [(940., 660.), (1392., 883.)] {
        for zoom in [1., 1.67] {
            for theme in ["apple-light", "apple-dark"] {
                for locale in ["en", "ja"] {
                    let case = Case {
                        dimensions,
                        zoom,
                        theme,
                        locale,
                    };
                    let (app, window) = open(cx, &repo, case);
                    keyboard_actions(cx, &app, window, case, &repo.join("with-port"), port);
                    no_port(cx, &app, window, case, &repo.join("no-port"));
                    owner_switch(
                        cx,
                        &app,
                        window,
                        case,
                        &repo.join("with-port"),
                        other.path(),
                    );
                    hover_actions(cx, &app, window, case, port);
                    unmount(cx, app, window);
                }
            }
        }
    }
    assert!(theme::set_active(&original_theme));
    eprintln!("[gui-e2e] PASS worktree_inspection_actions: names/focus, row Enter → Tab Refresh → Tab URL, Enter/Space once, retained focus, Escape return, owner switch, no-port, hover; EN/JA 100/167 Apple Light/Dark 940×660/1392×883");
}
