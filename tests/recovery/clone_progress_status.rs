//! Clone status semantics and owner-preserving keyboard Hide (#1341).
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{e2e, home_clone_card::clone_progress, i18n::Lang, theme, KagiApp};
use kagi_git::github_repos::RepoListing;
use std::time::{Duration, Instant};

fn listing() -> RepoListing {
    RepoListing {
        name_with_owner: "acme/widgets".into(),
        host: "github.com".into(),
        updated_at: String::new(),
        description: String::new(),
        is_private: false,
        is_fork: false,
    }
}

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    clone_progress::take();
    e2e::clear_control_bounds(window.window_id(), "clone-progress-status");
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn absent(window: AnyWindowHandle) {
    assert!(
        clone_progress::take().is_none(),
        "only a running clone has elapsed status"
    );
    assert!(e2e::control_bounds(window.window_id(), "clone-progress-status").is_none());
}

fn check_running(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    lang: Lang,
    seconds: u64,
) {
    app.update(cx, |app, _| {
        let mut modal = app.clone_modal().unwrap().clone();
        modal.started = Some(Instant::now() - Duration::from_secs(seconds));
        app.set_clone_modal(modal);
    });
    draw(cx, app, window);
    let rendered = clone_progress::take().expect("running label rendered");
    assert_eq!(rendered.role, Some(Role::Status));
    let expected = match lang {
        Lang::En => format!("Cloning… {} s", rendered.seconds),
        Lang::Ja => format!("clone 中… {} 秒", rendered.seconds),
    };
    assert_eq!(rendered.name.as_deref(), Some(expected.as_str()));
    assert!(rendered.visible.ends_with(&format!(
        "{}:{:02}",
        rendered.seconds / 60,
        rendered.seconds % 60
    )));
    let paint = e2e::control_paint(window.window_id(), "clone-progress-status").unwrap();
    assert!(paint.bounds.size.width > gpui::px(0.));
    assert!(paint.bounds.left() >= paint.mask.left());
    assert!(paint.bounds.right() <= paint.mask.right() + gpui::px(1.));
    assert!(paint.bounds.top() >= paint.mask.top());
    assert!(paint.bounds.bottom() <= paint.mask.bottom() + gpui::px(1.));
    assert!(e2e::control_bounds(window.window_id(), "clone-hide").is_some());
}

fn matrix(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for preset in ["apple-light", "apple-dark"] {
            theme::set_active(preset);
            for lang in [Lang::En, Lang::Ja] {
                app.update(cx, |app, cx| app.set_lang(lang, cx));
                for zoom in [1., 1.67] {
                    theme::set_zoom(zoom);
                    for reduced in [false, true] {
                        theme::set_reduce_motion(reduced);
                        check_running(cx, app, window, lang, 12);
                        check_running(cx, app, window, lang, 65);
                    }
                }
            }
        }
    }
}

fn running_fixture(cx: &mut VisualTestAppContext) {
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.open_clone_card(listing(), cx));
    draw(cx, &app, window);
    absent(window);
    app.update(cx, |app, cx| app.replan_clone(parent.path(), cx));
    draw(cx, &app, window);
    absent(window);
    app.update(cx, |app, _| {
        let mut modal = app.clone_modal().unwrap().clone();
        modal.started = Some(Instant::now());
        app.home_github.cloning = Some(modal.clone());
        app.set_clone_modal(modal);
    });
    matrix(cx, &app, window);
    let owner = cx.read(|cx| app.read(cx).home_github.cloning.clone().unwrap());
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).modal_focus.clone().unwrap(), cx);
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "tab tab enter");
    assert!(
        cx.read(|cx| app.read(cx).clone_modal().is_none()),
        "keyboard Hide dismisses only card"
    );
    cx.read(|cx| {
        let current = app.read(cx).home_github.cloning.as_ref().unwrap();
        assert_eq!(current.started, owner.started);
        assert_eq!(
            current.target.as_ref().unwrap().request.dest,
            owner.target.as_ref().unwrap().request.dest
        );
    });
    draw(cx, &app, window);
    absent(window);
    app.update(cx, |app, _| app.home_github.cloning = None);
    unmount(cx, app, window);
}

fn terminal_run(cx: &mut VisualTestAppContext, success: bool) {
    let fixture = build_fixture();
    let upstream = build_fixture();
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = bare_dir.path().join("widgets.git");
    git(
        upstream.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    let outcome = if success {
        format!("git clone -q '{}' \"$4\" && git -C \"$4\" remote set-url origin https://github.com/acme/widgets.git", bare.display())
    } else {
        "echo 'clone status test failure' >&2; exit 1".into()
    };
    let script = format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n'repo clone') {} ;;\n*) echo '[]' ;;\nesac\n",
        outcome,
    );
    let _gh = OfflineGh::with_script(&script);
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    // This leg exercises unattended completion, independent of the host's
    // native active-window state; current foreground results use the footer.
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    app.update(cx, |app, cx| app.open_clone_card(listing(), cx));
    draw(cx, &app, window);
    absent(window);
    app.update(cx, |app, cx| app.replan_clone(parent.path(), cx));
    draw(cx, &app, window);
    absent(window);
    app.update(cx, |app, cx| app.start_clone(cx));
    crate::home_github::wait_for(cx, &app, "clone terminal result", |app| {
        app.home_github.cloning.is_none()
    });
    draw(cx, &app, window);
    absent(window);
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_none()));
    if success {
        // Repository tabs normalize symlink aliases (e.g. /var -> /private/var).
        let destination = parent.path().join("widgets").canonicalize().unwrap();
        assert!(cx.read(|cx| app.read(cx).tabs.iter().any(|tab| tab.path == destination)));
    } else {
        let receipts = kagi_git::read_oplog_tail_for_repo(&parent.path().join("widgets"), 100);
        let failure = receipts.iter().find(|entry| entry.op == "clone").unwrap();
        assert!(matches!(
            &failure.outcome,
            kagi_git::oplog::OpOutcome::Failed { error, .. }
                if error.contains("clone status test failure")
        ));
        cx.read(|cx| {
            let stack = app.read(cx).toast_stack.as_ref().unwrap().read(cx);
            let errors: Vec<_> = stack
                .toasts()
                .iter()
                .filter(|toast| toast.kind == kagi::ui::ToastKind::Error)
                .collect();
            assert_eq!(errors.len(), 1, "one notice for the actual failed clone");
            assert_eq!(errors[0].subject, kagi::ui::toast_stack::ToastSubject::App);
            assert!(matches!(
                errors[0].action,
                Some(kagi::ui::ToastAction::OpenOperationLog(_))
            ));
        });
    }
    unmount(cx, app, window);
}

pub fn scenario_clone_progress_status(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "lang",
        "ui_zoom",
        "reduce_motion",
        "recent_repos",
    ]);
    let reduced = theme::reduce_motion();
    let preset = theme::theme().slug.clone();
    running_fixture(cx);
    terminal_run(cx, true);
    terminal_run(cx, false);
    theme::set_reduce_motion(reduced);
    theme::set_active(&preset);
}
