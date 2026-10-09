//! Real gh transport, production detail acceptance, and private native events.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{point, px, AnyWindowHandle, Bounds, Entity, Pixels, Point, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
    theme, KagiApp,
};
use std::{ffi::OsString, os::unix::fs::PermissionsExt, path::Path};

pub const BODY: &str = "issue-thread-body-4-md";
pub const POISON: &str = "PRIVATE_CLIPBOARD_NOT_COPIED";
pub const BASE: &str = "github.com/conversation/owner-a";
pub type NativeApp = Entity<KagiApp>;

pub struct Restore {
    path: Option<OsString>,
    lang: Lang,
    zoom: f32,
    theme: String,
    _keys: crate::gui_isolation::SavedKeys,
}
impl Restore {
    pub fn capture() -> Self {
        Self {
            path: std::env::var_os("PATH"),
            lang: i18n::lang(),
            zoom: theme::zoom(),
            theme: theme::theme().slug.to_string(),
            _keys: crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]),
        }
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        match self.path.take() {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        i18n::set_lang(self.lang);
        theme::set_zoom(self.zoom);
        theme::set_active(&self.theme);
    }
}

pub struct Producer {
    root: tempfile::TempDir,
}
impl Producer {
    pub fn install() -> Self {
        // Do not let this test redefine the process-cached capability probe.
        let _ = kagi_git::github::gh_available();
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("gh");
        let directory = quote(root.path());
        std::fs::write(&script, format!(r#"#!/bin/sh
set -eu
case "$*" in
  'issue view 4 -R github.com/conversation/owner-a --json number,title,state,url,author,assignees,labels,body,comments,createdAt,updatedAt') owner=a ;;
  'issue view 4 -R github.com/conversation/owner-b --json number,title,state,url,author,assignees,labels,body,comments,createdAt,updatedAt') owner=b ;;
  *) printf 'unexpected gh request: %s\n' "$*" >&2; exit 73 ;;
esac
printf '%s\n' "$owner" >> {directory}/requests
if test -f {directory}/fail-$owner; then
  printf 'HTTP 503: conversation-refresh-unavailable\n' >&2
  exit 1
fi
cat {directory}/$owner.json
"#)).unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![root.path().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self { root }
    }
    pub fn publish(&self, owner: &str, body: &str, comments: &[(&str, &str)]) {
        // Opaque GraphQL IDs are delivered by JSON, never constructed as domain
        // comments. Deliberately identical authors/timestamps defeat metadata IDs.
        let comments: Vec<_> = comments
            .iter()
            .map(|(id, body)| {
                serde_json::json!({
                    "id": id, "body": body, "author": {"login": "same-author"},
                    "createdAt": "2026-10-08T00:00:00Z", "updatedAt": "2026-10-08T00:00:00Z"
                })
            })
            .collect();
        let payload = serde_json::json!({"number":4,"title":"Conversation fixture", "state":"OPEN",
            "url":format!("https://github.com/conversation/owner-{owner}/issues/4"),
            "author":{"login":"same-author"}, "assignees":[],"labels":[],
            "body":body,"comments":comments,"createdAt":"2026-10-08T00:00:00Z",
            "updatedAt":"2026-10-08T00:00:00Z"});
        std::fs::write(
            self.root.path().join(format!("{owner}.json")),
            payload.to_string(),
        )
        .unwrap();
    }
    pub fn fail(&self, owner: &str) {
        std::fs::write(self.root.path().join(format!("fail-{owner}")), b"offline").unwrap();
    }
    pub fn requests(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.path().join("requests"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

pub fn fixture(
    cx: &mut VisualTestAppContext,
    base: &str,
) -> (
    tempfile::TempDir,
    NativeApp,
    AnyWindowHandle,
    (String, String),
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount(cx, &repo);
    configure(cx, &app, base);
    (fixture, app, win, before)
}
pub fn configure(cx: &mut VisualTestAppContext, app: &NativeApp, base: &str) {
    app.update(cx, |app, cx| {
        app.set_lang(Lang::En, cx);
        app.seed_issue_composer_for_e2e(cx);
        app.seed_issue_list_for_e2e(base, Vec::new(), cx);
        app.show_issues_mode(cx);
    });
    cx.run_until_parked();
}
pub fn load(cx: &mut VisualTestAppContext, app: &NativeApp, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.load_github_issue_detail(4, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let accepted = cx.read(|cx| app.read(cx).ui().github_issue_details.contains_key(&4));
    assert!(
        accepted,
        "the real gh producer must land through the accepted app detail path"
    );
}
pub fn finish(
    cx: &mut VisualTestAppContext,
    fixture: tempfile::TempDir,
    app: NativeApp,
    win: AnyWindowHandle,
    before: (String, String),
) {
    assert_eq!(
        before,
        repo_fingerprint(&fixture.path().canonicalize().unwrap()),
        "fixture repository changed"
    );
    unmount(cx, app, win);
    drop(fixture);
}
pub fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
) -> Option<Bounds<Pixels>> {
    e2e::clear_control_bounds(win.window_id(), id);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(win.window_id(), id)
}
pub fn pane(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> Bounds<Pixels> {
    measure(cx, win, "issue-mode-center-pane").expect("real Issue scroll viewport")
}
pub fn visible(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
) -> Option<Bounds<Pixels>> {
    let viewport = pane(cx, win);
    measure(cx, win, id)
        .filter(|bounds| bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom())
}
pub fn wheel(cx: &mut VisualTestAppContext, win: AnyWindowHandle, delta: f32) {
    let position = pane(cx, win).center();
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    cx.run_until_parked();
}
pub fn seek(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
    direction: f32,
) -> Bounds<Pixels> {
    for _ in 0..256 {
        if let Some(bounds) = visible(cx, win, id) {
            return bounds;
        }
        wheel(cx, win, direction * 180.);
    }
    panic!("{id} never became fully reachable through the single real scroll viewport");
}
pub fn start(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.left() + px(2.), bounds.top() + px(2.))
}
pub fn end(bounds: Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.right() - px(2.), bounds.bottom() - px(2.))
}
pub fn down(cx: &mut VisualTestAppContext, win: AnyWindowHandle, at: Point<Pixels>) {
    cx.simulate_mouse_down(win, at, gpui::MouseButton::Left, gpui::Modifiers::none());
}
pub fn move_to(cx: &mut VisualTestAppContext, win: AnyWindowHandle, at: Point<Pixels>) {
    cx.simulate_mouse_move(win, at, gpui::MouseButton::Left, gpui::Modifiers::none());
}
pub fn up(cx: &mut VisualTestAppContext, win: AnyWindowHandle, at: Point<Pixels>) {
    cx.simulate_mouse_up(win, at, gpui::MouseButton::Left, gpui::Modifiers::none());
}
pub fn copy(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> String {
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(POISON.into()));
    cx.simulate_keystrokes(win, "secondary-c");
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

/// Deliver a real transition and Copy in one window update using the current
/// rendered focus route. Key dispatch can repaint a dirty window; direct action
/// dispatch instead queues the existing capture/bubble action route. Queue the
/// clipboard observation immediately after that action, before the update's
/// effect drain is allowed to paint dirty windows.
pub fn copy_before_paint(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    transition: impl FnOnce(&mut gpui::Window, &mut gpui::App),
) -> String {
    let result = std::rc::Rc::new(std::cell::RefCell::new(None));
    let observed = result.clone();
    cx.update_window(win, |_, window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(POISON.into()));
        transition(window, cx);
        window.dispatch_action(Box::new(gpui_component::input::Copy), cx);
        cx.defer(move |cx| {
            *observed.borrow_mut() = Some(
                cx.read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default(),
            );
        });
    })
    .unwrap();
    let copied = result
        .borrow_mut()
        .take()
        .expect("deferred real Copy must complete before clipboard observation");
    copied
}

pub fn move_copy_before_paint(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    at: Point<Pixels>,
) -> String {
    copy_before_paint(cx, win, |window, cx| {
        window.dispatch_event(
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position: at,
                pressed_button: Some(gpui::MouseButton::Left),
                modifiers: gpui::Modifiers::none(),
            }),
            cx,
        );
    })
}

pub fn click_copy_before_paint(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    at: Point<Pixels>,
) -> String {
    copy_before_paint(cx, win, |window, cx| {
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                position: at,
                button: gpui::MouseButton::Left,
                modifiers: gpui::Modifiers::none(),
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
        window.dispatch_event(
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                position: at,
                button: gpui::MouseButton::Left,
                modifiers: gpui::Modifiers::none(),
                click_count: 1,
                ..Default::default()
            }),
            cx,
        );
    })
}
pub fn select(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) -> String {
    let bounds = seek(cx, win, id, 1.);
    down(cx, win, start(bounds));
    move_to(cx, win, end(bounds));
    up(cx, win, end(bounds));
    copy(cx, win)
}
pub fn comment(index: usize) -> String {
    format!("issue-thread-comment-4-{index}-md")
}
