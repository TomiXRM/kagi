//! #1225: A″ compact surfaces share the real commit-panel entity and Git state.
//! Every entry point first requires compact-rail: baseline fails on the missing
//! layout, not a future Rust API. Popup flip near a bottom anchor is covered by
//! the production placement helper's unit test; the current WIP starts at top.
use crate::macos::{build_fixture, git, unmount};
use crate::recovery_layout::contained;
use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Focusable, Modifiers, Pixels, ScrollDelta,
    ScrollWheelEvent, TouchPhase, VisualTestAppContext,
};
use kagi::ui::{commit_panel::CommitPanelView, e2e, KagiApp};
use kagi_ui_core::{i18n, theme};
use std::{cell::RefCell, path::Path, process::Command, rc::Rc, time::Duration};

#[path = "compact_layout/changes.rs"]
pub mod changes;
#[path = "compact_layout/inspector.rs"]
pub mod inspector;
#[path = "compact_layout/lifecycle.rs"]
pub mod lifecycle;
#[path = "compact_layout/popup.rs"]
pub mod popup;
#[path = "compact_layout/queue_history.rs"]
pub mod queue_history;
#[path = "compact_layout/rail.rs"]
pub mod rail;
#[path = "compact_layout/state.rs"]
pub mod state;

fn press_key(cx: &mut VisualTestAppContext, win: AnyWindowHandle, key: &str) {
    settle(cx, win);
    let keystroke = gpui::Keystroke::parse(key).unwrap();
    cx.dispatch_keystroke(win, keystroke.clone());
    cx.simulate_event(win, gpui::KeyUpEvent { keystroke });
    settle(cx, win);
}

fn focus_control(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let focus = e2e::commit_panel_labels::focus_handle(id)
        .unwrap_or_else(|| panic!("{id} must expose its production keyboard focus"));
    cx.update_window(win, |_, window, cx| focus.focus(window, cx))
        .unwrap();
    settle(cx, win);
}

fn assert_control_focus(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    assert!(
        cx.update_window(win, |_, window, _| {
            e2e::commit_panel_labels::focused(id, window)
        })
        .unwrap(),
        "{id} must own actual window focus"
    );
}

fn assert_ax(id: &str, role: gpui::Role, name: &str) {
    assert_eq!(e2e::commit_panel_labels::accessible_role(id), Some(role));
    assert_eq!(
        e2e::commit_panel_labels::accessible_name(id).as_deref(),
        Some(name)
    );
}
const WIDTH: f32 = 940.;
const FILES_PER_SIDE: usize = 72;

/// Desktop-role transitions must clear the current rem threshold at every zoom.
fn desktop_width() -> f32 {
    (theme::compact_threshold_rem() as f32 * theme::rem_size_px())
        .ceil()
        .max(1392.)
}
const RECORDERS: &[&str] = &[
    "compact-rail",
    "plan-heading-title",
    "compact-graph",
    "compact-wip-row",
    "compact-wip-affordance",
    "compact-wip-diffstat",
    "cp-coauthor",
    "status-icon-oplog",
    "compact-sidebar-popover",
    "compact-rail-local",
    "compact-rail-remote",
    "compact-rail-worktrees",
    "compact-rail-tags",
    "compact-rail-stashes",
    "compact-rail-prs",
    "compact-rail-issues",
    "list-filter-text",
    "sidebar-local-main",
    "compact-tab-changes",
    "compact-inspector-details",
    "compact-inspector-files",
    "compact-composer",
    "compact-popup-close",
    "compact-changes-unstaged",
    "compact-changes-staged",
    "cp-commit-action",
    "cp-row-stage",
    "cp-row-unstage",
    "cp-first-row-stage",
    "cp-first-row-unstage",
    "cp-view-list",
    "cp-view-tree",
    "cp-staged-view-list",
    "cp-staged-view-tree",
    "cp-stage-all",
    "cp-unstage-all",
];

#[derive(Clone, Copy, Debug)]
struct Case {
    height: f32,
    zoom: f32,
    locale: &'static str,
    preset: &'static str,
}

impl Case {
    fn dimensions(self) -> (f32, f32) {
        (WIDTH, self.height)
    }
}

fn matrix(mut run: impl FnMut(Case)) {
    for height in [660., 1000.] {
        for zoom in [1., 1.67] {
            for locale in ["en", "ja"] {
                for preset in ["apple-light", "apple-dark"] {
                    run(Case {
                        height,
                        zoom,
                        locale,
                        preset,
                    });
                }
            }
        }
    }
}

fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    // Real history makes the current WIP capable of leaving the graph viewport.
    // These commits and files are never replaced by installed panel/graph rows.
    for n in 0..64 {
        git(
            fixture.path(),
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("history {n}"),
            ],
        );
    }
    git(fixture.path(), &["branch", "compact-fixture"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(fixture.path(), &["tag", "compact-fixture"]);
    std::fs::write(fixture.path().join("README.md"), "real stash fixture\n").unwrap();
    git(
        fixture.path(),
        &["stash", "push", "-q", "-m", "compact fixture"],
    );
    for n in 0..FILES_PER_SIDE {
        std::fs::write(
            fixture.path().join(format!("staged-{n:03}.txt")),
            "staged fixture\n",
        )
        .unwrap();
    }
    git(fixture.path(), &["add", "."]);
    for n in 0..FILES_PER_SIDE {
        std::fs::write(
            fixture.path().join(format!("unstaged-{n:03}.txt")),
            "unstaged fixture\n",
        )
        .unwrap();
    }
    fixture
}

fn bounds(win: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(win.window_id(), name).unwrap_or_else(|| panic!("missing {name}"))
}

fn clear(win: AnyWindowHandle) {
    for name in RECORDERS {
        e2e::clear_control_bounds(win.window_id(), name);
    }
}

fn frame(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    clear(win);
    for _ in 0..4 {
        cx.run_until_parked();
        frame(cx, win);
    }
}

fn mount(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    case: Case,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let _theme_env = EnvGuard::set(case.preset);
    std::env::set_var("KAGI_LANG", case.locale);
    i18n::init_lang();
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).unwrap();
    // Startup migrates stored scale values; apply the requested matrix value
    // afterwards so the observed frame, not only its intended case, is tested.
    theme::set_zoom(case.zoom);
    theme::set_compact_threshold_rem(kagi_domain::compact::DEFAULT_THRESHOLD_REM);
    theme::set_active(case.preset);
    cx.update(theme::sync_gpui_component_theme);
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let win =
        crate::macos::open_offscreen(cx, size(px(WIDTH), px(case.height)), move |window, cx| {
            e2e::mount_root(state, window, cx, &output)
        })
        .into();
    let app = captured.borrow_mut().take().unwrap();
    settle(cx, win);
    // FIRST layout assertion in every scenario/matrix cell. Untouched baseline
    // stops here; all code below uses APIs already present on that baseline.
    let rail = bounds(win, "compact-rail");
    assert!(
        rail.size.width > px(0.),
        "{case:?}: missing usable compact-rail"
    );
    assert_eq!(
        theme::theme().slug.as_ref(),
        case.preset,
        "{case:?}: actual mounted theme"
    );
    assert!(
        (theme::zoom() - case.zoom).abs() < 0.001,
        "{case:?}: actual mounted zoom"
    );
    (app, win)
}

fn viewport(dimensions: (f32, f32)) -> Bounds<Pixels> {
    Bounds::new(
        point(px(0.), px(0.)),
        size(px(dimensions.0), px(dimensions.1)),
    )
}

fn near(actual: Pixels, expected: Pixels, reason: &str) {
    assert!(
        (actual - expected).abs() <= px(1.),
        "{reason}: {actual:?} != {expected:?}"
    );
}

fn rail_graph(win: AnyWindowHandle, dimensions: (f32, f32)) {
    let rail = bounds(win, "compact-rail");
    let graph = bounds(win, "compact-graph");
    contained(viewport(dimensions), rail, "compact rail");
    contained(viewport(dimensions), graph, "actual graph outer column");
    near(rail.size.width, theme::scaled_px(44.), "scaled 44px rail");
    near(rail.left(), px(0.), "rail starts at window edge");
    near(
        graph.left(),
        rail.right(),
        "graph begins immediately after rail",
    );
    near(
        graph.right(),
        px(dimensions.0),
        "no right pane takes graph width",
    );
    assert!(graph.size.height > px(0.), "graph remains visible");
}

fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, name: &str) {
    let target = bounds(win, name);
    cx.simulate_mouse_move(win, target.center(), None, Modifiers::none());
    frame(cx, win);
    cx.simulate_click(win, target.center(), Modifiers::none());
    settle(cx, win);
}

fn changes(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
) -> Entity<CommitPanelView> {
    // PRs intentionally closes the existing bottom panel on entry. Reopen it
    // through its real shortcut after navigator coverage returns to Graph.
    if !cx.read(|cx| app.read(cx).bottom_panel_open) {
        focus_root(cx, app, win);
        cx.simulate_keystrokes(win, "cmd-j");
        settle(cx, win);
    }
    click(cx, win, "compact-tab-changes");
    assert!(
        e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "Changes tab alone must not open the composer"
    );
    cx.read(|cx| app.read(cx).ui().commit_panel.clone())
        .expect("Changes shares commit-panel entity")
}

fn columns(win: AnyWindowHandle, dimensions: (f32, f32)) {
    let unstaged = bounds(win, "compact-changes-unstaged");
    let staged = bounds(win, "compact-changes-staged");
    for (name, b) in [("unstaged", unstaged), ("staged", staged)] {
        contained(viewport(dimensions), b, name);
        assert!(
            b.size.width > px(0.) && b.size.height > px(0.),
            "{name} must be usable"
        );
    }
    near(
        unstaged.left(),
        bounds(win, "compact-rail").right(),
        "Changes starts after rail",
    );
    near(
        staged.right(),
        px(dimensions.0),
        "Changes uses full trailing width",
    );
    near(unstaged.top(), staged.top(), "Changes columns share top");
    near(
        unstaged.bottom(),
        staged.bottom(),
        "Changes columns share bottom",
    );
    assert!(
        unstaged.right() <= staged.left() + px(1.),
        "Changes must not stack or overlap"
    );
    assert!(
        (staged.left() - unstaged.right()).abs() <= theme::scaled_px(16.),
        "bounded divider/gap"
    );
    let midpoint = (unstaged.left() + staged.right()) / 2.;
    assert!(
        (unstaged.right() - midpoint).abs() <= theme::scaled_px(8.) + px(1.),
        "Changes divider must be approximately halfway: {unstaged:?} / {staged:?}"
    );
    assert!(
        (unstaged.size.width - staged.size.width).abs() <= theme::scaled_px(16.) + px(1.),
        "Changes columns share available width"
    );
}

fn commit_columns(win: AnyWindowHandle, dimensions: (f32, f32)) {
    let details = bounds(win, "compact-inspector-details");
    let files = bounds(win, "compact-inspector-files");
    for (name, column) in [("commit details", details), ("changed files", files)] {
        contained(viewport(dimensions), column, name);
        contained(bounds(win, "bottom-panel"), column, name);
        assert!(
            column.size.width > px(0.) && column.size.height > px(0.),
            "{name} must be usable"
        );
    }
    near(details.top(), files.top(), "Inspector columns share top");
    near(
        details.bottom(),
        files.bottom(),
        "Inspector columns share bottom",
    );
    assert!(
        details.right() <= files.left() + px(1.),
        "Inspector metadata is left of files, not stacked or overlapping"
    );
    assert!(
        (details.size.width - files.size.width).abs() <= theme::scaled_px(16.) + px(1.),
        "Inspector columns share available width"
    );
    contained(
        files,
        bounds(win, "inspector-files-scroll"),
        "shared file RowList",
    );
}

fn wheel(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    position: gpui::Point<Pixels>,
    dy: f32,
) {
    cx.simulate_event(
        win,
        ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        },
    );
    settle(cx, win);
}

fn independent_scroll(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
) {
    let (unstaged, staged, graph) = cx.read(|cx| {
        let panel = panel.read(cx);
        assert_eq!(
            panel.state.unstaged.len(),
            FILES_PER_SIDE,
            "real unstaged fixture"
        );
        assert_eq!(
            panel.state.staged.len(),
            FILES_PER_SIDE,
            "real staged fixture"
        );
        (
            panel.unstaged_scroll_handle.clone(),
            panel.staged_scroll_handle.clone(),
            app.read(cx).ui().commit_scroll_handle.clone(),
        )
    });
    let first = unstaged.0.borrow().base_handle.offset();
    let second = staged.0.borrow().base_handle.offset();
    let graph_before = graph.0.borrow().base_handle.offset();
    let composer = e2e::control_bounds(win.window_id(), "compact-composer");
    let unstaged_viewport = unstaged.0.borrow().base_handle.bounds();
    contained(
        bounds(win, "compact-changes-unstaged"),
        unstaged_viewport,
        "unstaged scroll owner",
    );
    wheel(cx, win, unstaged_viewport.center(), -300.);
    assert!(
        unstaged.0.borrow().base_handle.offset().y < first.y - px(20.),
        "real unstaged wheel scroll"
    );
    assert_eq!(
        staged.0.borrow().base_handle.offset(),
        second,
        "unstaged wheel must not scroll staged"
    );
    assert_eq!(
        graph.0.borrow().base_handle.offset(),
        graph_before,
        "unstaged wheel must not scroll graph"
    );
    let first_after = unstaged.0.borrow().base_handle.offset();
    let staged_viewport = staged.0.borrow().base_handle.bounds();
    contained(
        bounds(win, "compact-changes-staged"),
        staged_viewport,
        "staged scroll owner",
    );
    wheel(cx, win, staged_viewport.center(), -300.);
    assert!(
        staged.0.borrow().base_handle.offset().y < second.y - px(20.),
        "real staged wheel scroll"
    );
    assert_eq!(
        unstaged.0.borrow().base_handle.offset(),
        first_after,
        "staged wheel must not scroll unstaged"
    );
    assert_eq!(
        graph.0.borrow().base_handle.offset(),
        graph_before,
        "staged wheel must not scroll graph"
    );
    assert_eq!(
        e2e::control_bounds(win.window_id(), "compact-composer"),
        composer,
        "composer is pinned while Changes scroll"
    );
    drop((unstaged, staged, graph));
}

pub(crate) fn open_wip(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    let wip = bounds(win, "compact-wip-row");
    let graph = bounds(win, "compact-graph");
    contained(
        graph,
        wip,
        "current WIP row lies inside actual graph column",
    );
    assert!(wip.size.height > px(0.), "visible current WIP row");
    // The reveal is absent at rest, then appears only after a real hover.
    cx.simulate_mouse_move(
        win,
        bounds(win, "compact-rail").center(),
        None,
        Modifiers::none(),
    );
    settle(cx, win);
    assert!(
        e2e::control_bounds(win.window_id(), "compact-wip-affordance").is_none(),
        "WIP affordance is hidden at rest"
    );
    cx.simulate_mouse_move(win, wip.center(), None, Modifiers::none());
    settle(cx, win);
    let affordance = bounds(win, "compact-wip-affordance");
    contained(wip, affordance, "hover-only WIP reveal");
    if let Some(stat) = e2e::control_bounds(win.window_id(), "compact-wip-diffstat") {
        assert!(
            affordance.right() <= stat.left() + px(1.),
            "Commit… reveal must not overlap actual WIP diffstat: {affordance:?} / {stat:?}"
        );
    }
    assert!(
        e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "hover alone must not open the popup"
    );
    cx.simulate_click(win, affordance.center(), Modifiers::none());
    settle(cx, win);
    bounds(win, "compact-composer");
}

fn popup_geometry(win: AnyWindowHandle, dimensions: (f32, f32)) {
    let composer = bounds(win, "compact-composer");
    contained(viewport(dimensions), composer, "popup clamped to window");
    assert!(
        composer.size.width > px(0.) && composer.size.height > px(0.),
        "usable composer"
    );
    for name in ["cp-commit-action", "compact-popup-close"] {
        let action = bounds(win, name);
        contained(composer, action, name);
        contained(viewport(dimensions), action, name);
        assert!(
            action.size.width > px(0.) && action.size.height > px(0.),
            "reachable {name}"
        );
    }
    let wip = bounds(win, "compact-wip-row");
    let room_below = px(dimensions.1) - wip.bottom();
    let room_above = wip.top();
    if room_below >= composer.size.height + theme::scaled_px(8.) {
        assert!(
            composer.top() >= wip.bottom() - px(1.),
            "popup prefers below when it fits"
        );
    } else if room_above >= composer.size.height + theme::scaled_px(8.) {
        assert!(
            composer.bottom() <= wip.top() + px(1.),
            "popup flips above when below cannot fit"
        );
    }
}

fn set_draft(cx: &mut VisualTestAppContext, panel: &Entity<CommitPanelView>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        let title = panel.read(cx).title_input.clone().unwrap();
        let body = panel.read(cx).body_input.clone().unwrap();
        title.update(cx, |input, cx| {
            input.set_value("compact retained subject 日本語", window, cx)
        });
        body.update(cx, |input, cx| {
            input.set_value("first paragraph\n\nretained body 本文", window, cx)
        });
    })
    .unwrap();
    settle(cx, win);
}

fn draft(cx: &VisualTestAppContext, panel: &Entity<CommitPanelView>) -> (String, String, String) {
    cx.read(|cx| {
        let panel = panel.read(cx);
        (
            panel
                .title_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string(),
            panel
                .body_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string(),
            panel.effective_commit_message(cx),
        )
    })
}

fn escape(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        let focus = app.read(cx).root_focus.as_ref().unwrap().clone();
        focus.focus(window, cx);
    })
    .unwrap();
    cx.simulate_keystrokes(win, "escape");
    settle(cx, win);
}

fn resize(cx: &mut VisualTestAppContext, win: AnyWindowHandle, dimensions: (f32, f32)) {
    let wanted = size(px(dimensions.0), px(dimensions.1));
    cx.update_window(win, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(win, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native resize did not reach {wanted:?}"
        );
    }
    settle(cx, win);
}

pub fn scenario_compact_layout_geometry(cx: &mut VisualTestAppContext) {
    geometry_matrix(cx, 660., 1.);
}

pub fn scenario_compact_layout_geometry_zoom167(cx: &mut VisualTestAppContext) {
    geometry_matrix(cx, 660., 1.67);
}

pub fn scenario_compact_layout_geometry_tall(cx: &mut VisualTestAppContext) {
    geometry_matrix(cx, 1000., 1.);
}

pub fn scenario_compact_layout_geometry_tall_zoom167(cx: &mut VisualTestAppContext) {
    geometry_matrix(cx, 1000., 1.67);
}

pub fn scenario_compact_inspector_directory_accordion(cx: &mut VisualTestAppContext) {
    crate::inspector_file_rows::directory_accordion(cx, true);
}

// Each native process retains all EN/JA × Light/Dark cells for one viewport/zoom,
// bounding the expensive rail interactions without extending the 120s deadline.
fn geometry_matrix(cx: &mut VisualTestAppContext, height: f32, zoom: f32) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\necho 'compact fixture: offline GitHub' >&2\nexit 1\n",
    );
    matrix(|case| {
        if case.height != height || case.zoom != zoom {
            return;
        }
        let (app, win) = mount(cx, fixture.path(), case);
        rail_graph(win, case.dimensions());
        rail::rail_interactions(cx, &app, win, fixture.path(), case.dimensions());
        // Selecting Changes for a historical commit does not create a WIP owner.
        if !cx.read(|cx| app.read(cx).bottom_panel_open) {
            focus_root(cx, &app, win);
            cx.simulate_keystrokes(win, "cmd-j");
            settle(cx, win);
        }
        click(cx, win, "compact-tab-changes");
        assert!(
            cx.read(|cx| app.read(cx).ui().selected.is_some()),
            "local navigator selected a real commit"
        );
        commit_columns(win, case.dimensions());
        // Changes follows graph selection. Restore real WIP before asking
        // its existing staging lists to scroll; tab selection alone cannot.
        click(cx, win, "compact-wip-row");
        assert!(cx.read(|cx| app.read(cx).ui().selected.is_none()));
        escape(cx, &app, win);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .expect("real WIP activation creates the shared staging owner");
        columns(win, case.dimensions());
        rail_graph(win, case.dimensions());
        independent_scroll(cx, &app, &panel, win);
        // Existing production entry selects Changes and pins the same composer.
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| app.open_commit_panel(window, cx));
        })
        .unwrap();
        settle(cx, win);
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
            panel.entity_id()
        );
        popup_geometry(win, case.dimensions());
        rail_graph(win, case.dimensions());
        columns(win, case.dimensions());
        drop(panel);
        unmount(cx, app, win);
    });
}

fn index(repo: &Path) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["ls-files", "--stage", "-z"])
        .output()
        .unwrap();
    assert!(output.status.success(), "read real fixture index");
    output.stdout
}

fn focus_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        let focus = app.read(cx).root_focus.as_ref().unwrap().clone();
        focus.focus(window, cx);
    })
    .unwrap();
}

struct EnvGuard(Option<std::ffi::OsString>);

impl EnvGuard {
    fn set(preset: &str) -> Self {
        let previous = std::env::var_os("KAGI_THEME");
        std::env::set_var("KAGI_THEME", preset);
        Self(previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var("KAGI_THEME", previous),
            None => std::env::remove_var("KAGI_THEME"),
        }
    }
}
