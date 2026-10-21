//! #1443: Cmd-W closes the current document, otherwise its guarded host.
use gpui::{px, size, AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn mount(
    cx: &mut VisualTestAppContext,
    state: KagiApp,
    width: f32,
    height: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    let out = Rc::new(RefCell::new(None));
    let capture = out.clone();
    let window = crate::macos::open_offscreen(cx, size(px(width), px(height)), move |w, cx| {
        e2e::mount_root(state, w, cx, &capture)
    });
    let app = out.borrow_mut().take().unwrap();
    cx.run_until_parked();
    draw(cx, window.into());
    (app, window.into())
}

pub(crate) fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, w, cx| {
        w.refresh();
        w.draw(cx).clear();
    })
    .unwrap();
}
fn open(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> bool {
    cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == window.window_id())
    })
}
fn close(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    draw(cx, window);
    cx.simulate_keystrokes(window, "cmd-w");
    cx.run_until_parked();
}

pub fn scenario_close_window_initial_home(cx: &mut VisualTestAppContext) {
    check_initial_home(cx);
    check_clean_repository(cx);
    check_home_takes_window(cx);
    check_documents(cx);
    check_busy_home(cx);
}

fn check_initial_home(cx: &mut VisualTestAppContext) {
    let (app, window) = mount(cx, KagiApp::with_error(""), 940., 660.);
    assert!(app.read_with(cx, |app, _| app.tabs.is_empty() && app.home.is_none()));
    close(cx, window);
    assert!(
        !open(cx, window),
        "Cmd-W on initial Home must request host close"
    );
    drop(app);
    cx.run_until_parked();
}

fn check_documents(cx: &mut VisualTestAppContext) {
    let fixture = crate::macos::build_fixture();
    let (app, window) = mount(cx, e2e::app_state(fixture.path()).unwrap(), 940., 660.);
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |a, cx| a.open_home_tab(w, cx))
    })
    .unwrap();
    close(cx, window);
    assert!(open(cx, window));
    assert!(
        app.read_with(cx, |a, _| a.home.is_none() && a.tabs.len() == 1),
        "Cmd-W closes only explicit Home when repositories coexist"
    );
    let editor = dirty_editor(cx, &app, window);
    close(cx, window);
    assert!(app.read_with(cx, |a, _| a.tabs.len() == 1
        && a.editor_dirty_guard_modal().is_some()));
    app.update(cx, |a, _| a.cancel_editor_dirty_guard());
    assert!(
        editor.read_with(cx, |e, _| e.any_dirty()),
        "Cancel preserves the draft"
    );
    close(cx, window);
    app.update(cx, |a, cx| a.confirm_editor_dirty_guard(cx));
    drop(editor);
    assert!(
        open(cx, window) && app.read_with(cx, |a, _| a.tabs.is_empty()),
        "closing the final repository returns Home without closing the window"
    );
    close(cx, window);
    assert!(
        !open(cx, window),
        "next Cmd-W closes the now document-free host"
    );
    drop(app);
    cx.run_until_parked();
}

fn check_busy_home(cx: &mut VisualTestAppContext) {
    let (app, window) = mount(cx, KagiApp::with_error(""), 940., 660.);
    app.update(cx, |a, cx| {
        a.home_github.cloning = Some(kagi::ui::home_github::CloneModal {
            listing: kagi_git::github_repos::RepoListing {
                name_with_owner: "audit/repo".into(),
                host: "github.com".into(),
                is_fork: false,
                is_private: false,
                description: String::new(),
                updated_at: String::new(),
            },
            target: None,
            started: Some(std::time::Instant::now()),
        });
        cx.notify();
    });
    close(cx, window);
    assert!(
        open(cx, window)
            && app.read_with(cx, |a, _| !a.may_close_host() && a.app_notice().is_some()),
        "contextual host close cannot bypass the running clone hold"
    );
    app.update(cx, |a, cx| {
        a.home_github.cloning = None;
        cx.notify();
    });
    draw(cx, window);
    assert!(app.read_with(cx, |a, _| a.app_notice().is_none()));
    assert!(
        open(cx, window),
        "finishing the clone must not resume a refused close"
    );
    crate::macos::unmount(cx, app, window);
}

fn dirty_editor(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Entity<kagi_ui_editor::EditorWorkspaceView> {
    use gpui::Focusable;
    app.update(cx, |a, cx| a.open_editor_workspace(cx));
    let editor = app.read_with(cx, |a, _| a.ui().editor_workspace.clone().unwrap());
    editor.update(cx, |e, cx| {
        e.open_tab("README.md".into(), cx);
        e.set_markdown_preview(false, cx);
    });
    cx.run_until_parked();
    draw(cx, window);
    let input = editor.read_with(cx, |e, _| e.editor.clone().unwrap());
    let before = input.read_with(cx, |input, _| input.value().to_string());
    cx.update_window(window, |_, w, cx| {
        w.focus(&input.read(cx).focus_handle(cx), cx);
        w.draw(cx).clear();
        assert!(
            input.read(cx).focus_handle(cx).is_focused(w),
            "the editable document owns text input"
        );
    })
    .unwrap();
    cx.simulate_input(window, "unsaved close guard draft\n");
    cx.run_until_parked();
    assert!(input.read_with(cx, |input, _| {
        input.value().as_str() != before && input.value().contains("unsaved close guard draft")
    }));
    assert!(editor.read_with(cx, |e, _| e.any_dirty()));
    editor
}

fn check_clean_repository(cx: &mut VisualTestAppContext) {
    let fixture = crate::macos::build_fixture();
    let (app, window) = mount(cx, e2e::app_state(fixture.path()).unwrap(), 940., 660.);
    close(cx, window);
    assert!(open(cx, window));
    assert!(app.read_with(cx, |a, _| a.tabs.is_empty() && a.home_in_front()));
    crate::macos::unmount(cx, app, window);
}

fn check_home_takes_window(cx: &mut VisualTestAppContext) {
    let fixture = crate::macos::build_fixture();
    let (app, window) = mount(cx, e2e::app_state(fixture.path()).unwrap(), 940., 660.);
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |a, cx| a.open_home_tab(w, cx));
    })
    .unwrap();
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |a, cx| {
            a.handle_menu_command("window.previousTab", w, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert!(app.read_with(cx, |a, _| a.home.is_some() && !a.home_in_front()));
    close(cx, window);
    assert!(open(cx, window));
    assert!(app.read_with(cx, |a, _| a.tabs.is_empty()
        && a.home.is_some()
        && a.home_in_front()));
    close(cx, window);
    assert!(
        !open(cx, window),
        "one Cmd-W closes document-free Home after a retained Home tab takes the window"
    );
    drop(app);
    cx.run_until_parked();
}
