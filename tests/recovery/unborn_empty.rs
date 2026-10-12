//! Clean unborn Graph guidance and dirty unborn WIP preservation (#1371).
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
    list_a11y, theme,
};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::unborn::take();
    e2e::clear_control_bounds(window.window_id(), "unborn-open-changes");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn clean(cx: &mut VisualTestAppContext, lang: Lang) {
    let fixture = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    draw(cx, window);
    for name in [
        "unborn-state-icon",
        "unborn-state-title",
        "unborn-state-hint",
        "unborn-open-changes",
    ] {
        let rect = e2e::control_bounds(window.window_id(), name)
            .unwrap_or_else(|| panic!("clean unborn repository must render {name}"));
        cx.update_window(window, |_, window, _| {
            assert!(rect.left() >= gpui::px(0.) && rect.top() >= gpui::px(0.));
            assert!(
                rect.right() <= window.viewport_size().width
                    && rect.bottom() <= window.viewport_size().height,
                "{name} must fit at 167%: {rect:?}"
            );
        })
        .unwrap();
    }
    let state = e2e::unborn::take().expect("clean guidance rendered");
    let (title, hint) = match lang {
        Lang::En => ("No commits yet", "Add files and create the first commit"),
        Lang::Ja => (
            "まだ commit がありません",
            "ファイルを追加して最初の commit を作成してください",
        ),
    };
    assert_eq!(state.title.as_ref(), title);
    assert_eq!(state.hint.as_ref(), hint);
    assert_eq!(state.role, gpui::Role::Button);
    assert_eq!(state.name, "Changes");
    cx.update_window(window, |_, window, cx| {
        let focus = e2e::unborn::take_focus();
        assert!(
            (0..100).any(|_| {
                window.focus_next(cx);
                window.draw(cx).clear();
                focus.is_focused(window)
            }),
            "Changes Button must be reachable by Tab"
        );
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "enter");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui().commit_panel_open),
        "Enter opens Commit Panel"
    );
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "guidance must not write Git"
    );
    unmount(cx, app, window);
    dirty(cx, fixture.path());
}

fn dirty(cx: &mut VisualTestAppContext, repo: &std::path::Path) {
    std::fs::write(repo.join("first.txt"), "first\n").unwrap();
    let (app, window) = mount(cx, repo);
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).view().is_dirty));
    assert!(
        e2e::control_bounds(window.window_id(), "unborn-open-changes").is_none(),
        "dirty unborn keeps WIP, not guidance"
    );
    assert!(cx.read(|cx| app.read(cx).view().rows.is_empty()));
    let rows = list_a11y::recorded_list("commit-list").expect("dirty unborn renders list");
    assert_eq!(rows.size, 1);
    assert!(
        rows.rows.get(&0).unwrap().0.contains("WIP"),
        "dirty unborn renders WIP row"
    );
    unmount(cx, app, window);
}

fn ordinary_empty(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        assert!(!app.view().status_summary.is_unborn);
        app.view_mut().rows.clear(); // A zero-result graph read still owns a born HEAD.
        cx.notify();
    });
    draw(cx, window);
    assert!(
        e2e::unborn::take().is_none(),
        "zero results are not an unborn repository"
    );
    unmount(cx, app, window);
}

fn loading_and_failed_read(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let unborn = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(unborn.path())
        .status()
        .unwrap()
        .success());
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        assert!(app.open_repository(unborn.path().to_owned(), cx));
        assert!(app.loading_tab().is_some());
    });
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "unborn-open-changes").is_none(),
        "loading must not display unborn guidance"
    );
    // Fail the pending first snapshot, not an already-accepted unborn read.
    std::fs::rename(unborn.path().join(".git"), unborn.path().join("failed-git")).unwrap();
    cx.run_until_parked();
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "unborn-open-changes").is_none(),
        "failed first read must not claim no commits"
    );
    assert!(!cx.read(|cx| app.read(cx).view().status_summary.is_unborn));
    unmount(cx, app, window);
}

fn remote_empty(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let unborn = tempfile::tempdir().unwrap();
    assert!(std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(unborn.path())
        .status()
        .unwrap()
        .success());
    let snapshot = kagi_git::Backend::open(unborn.path())
        .unwrap()
        .snapshot(100)
        .unwrap();
    let (app, window) = mount(cx, fixture.path());
    let host = kagi_domain::remote::RemoteHost::parse("example.test").unwrap();
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/unborn".into(), snapshot, cx);
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).remote_view.is_some()));
    assert!(cx.read(|cx| app.read(cx).view().status_summary.is_unborn));
    assert!(cx.read(|cx| app.read(cx).repo_path.is_none()));
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "unborn-open-changes").is_none(),
        "read-only remote unborn view must not offer local Changes"
    );
    assert!(e2e::unborn::take().is_none());
    unmount(cx, app, window);
}

pub fn scenario_unborn_empty_state(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            cx.update(theme::sync_gpui_component_theme);
            clean(cx, lang);
            ordinary_empty(cx);
            loading_and_failed_read(cx);
            remote_empty(cx);
        }
    }
    eprintln!("[gui-e2e] PASS unborn_empty_state");
}
