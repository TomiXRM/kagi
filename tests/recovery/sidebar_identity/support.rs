//! Shared mounting, input, renderer probes and repository-write witnesses.
use std::cell::RefCell;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    point, px, size, AnyWindowHandle, Bounds, Entity, Modifiers, Pixels, VisualTestAppContext,
};
use kagi::ui::list_a11y::clear_recorded_lists;
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n::Lang, theme};

use crate::macos::open_offscreen;
use crate::recovery_layout::GlobalSettings;

pub(super) fn mount_dimensions(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    dimensions: (f32, f32),
) -> (Entity<KagiApp>, AnyWindowHandle) {
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = open_offscreen(
        cx,
        size(px(dimensions.0), px(dimensions.1)),
        move |window, cx| e2e::mount_root(state, window, cx, &output),
    );
    let app = captured.borrow().clone().expect("mounted KagiApp");
    cx.run_until_parked();
    (app, window.into())
}

pub(super) struct Settings {
    slug: String,
    _globals: GlobalSettings,
    _saved: crate::gui_isolation::SavedKeys,
}

impl Settings {
    pub(super) fn capture() -> Self {
        Self {
            slug: theme::theme().slug.to_string(),
            _globals: GlobalSettings::capture(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["theme", "sidebar_panes"]),
        }
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        theme::set_active(&self.slug);
    }
}

pub(super) fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "git {args:?}: {:?}", result.stderr);
    String::from_utf8(result.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

/// Include refs/reflogs, semantic index, tracked and untracked bytes, config,
/// object counts and durable receipts; stat-cache refresh is not a write.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Witness {
    git_state: Vec<String>,
    files: Vec<(String, Vec<u8>)>,
    receipts: usize,
}

impl Witness {
    pub(super) fn read(repo: &Path) -> Self {
        let commands: &[&[&str]] = &[
            &["rev-parse", "HEAD"],
            &["show-ref"],
            &["reflog", "show", "--all"],
            &["ls-files", "--stage"],
            &["diff", "--binary"],
            &["diff", "--cached", "--binary"],
            &["status", "--porcelain"],
            &["config", "--local", "--list"],
            &["count-objects", "-v"],
        ];
        let paths = output(
            repo,
            &["ls-files", "--cached", "--others", "--exclude-standard"],
        );
        Self {
            git_state: commands.iter().map(|args| output(repo, args)).collect(),
            files: paths
                .lines()
                .map(|path| (path.to_owned(), std::fs::read(repo.join(path)).unwrap()))
                .collect(),
            receipts: kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).len(),
        }
    }
}

pub(super) fn paint(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

pub(super) fn appearance(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    language: Lang,
    slug: &str,
    zoom: f32,
    pane: usize,
) {
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_theme(slug, cx);
        app.sidebar.collapsed.clear();
        app.sidebar.pane_weights = [1; 5];
        app.sidebar.pane_weights[pane] = 100;
        cx.notify();
    });
    cx.run_until_parked();
    paint(cx, app, window);
}

pub(super) fn focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    pane: usize,
    key: &str,
) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(pane, key, window, cx)
        });
        window.draw(cx).clear();
    })
    .unwrap();
    assert_eq!(focused(cx, app, window), Some((pane, key.to_owned())));
}

pub(super) fn focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Option<(usize, String)> {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx).sidebar_row_focused_for_e2e(window)
    })
    .unwrap()
}

pub(super) fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("{id} not drawn"))
}

pub(super) fn hover(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &str,
    at: gpui::Point<Pixels>,
) -> String {
    e2e::sidebar_labels::clear();
    cx.simulate_mouse_move(window, point(px(850.), px(600.)), None, Modifiers::none());
    paint(cx, app, window);
    cx.simulate_mouse_move(window, at, None, Modifiers::none());
    for _ in 0..3 {
        cx.advance_clock(Duration::from_millis(400));
        cx.run_until_parked();
        paint(cx, app, window);
    }
    e2e::sidebar_labels::recorded_tooltip(id)
        .unwrap_or_else(|| panic!("{id} hover built no tooltip"))
}
