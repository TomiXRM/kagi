//! Hidden clone retains a named, keyboard-reachable progress entrance (#1452).
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{e2e, home_work::HomePane, i18n::Lang, theme, KagiApp};
use kagi_git::github_repos::RepoListing;

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    e2e::take_clone_busy_status();
    for id in ["clone-busy-status", "clone-busy-show"] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn check_status(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    draw(cx, app, window);
    let (role, name) = e2e::take_clone_busy_status()
        .expect("hidden running clone must retain a named progress status");
    assert_eq!(role, Role::Status);
    let expected = match kagi::ui::i18n::lang() {
        Lang::En => "Cloning github.com/acme/widgets…",
        Lang::Ja => "github.com/acme/widgets を clone 中…",
    };
    assert_eq!(
        name, expected,
        "localized status names the exact running source"
    );
    assert!(!name.contains('%'), "clone has no determinate percentage");
    for id in ["clone-busy-status", "clone-busy-show"] {
        let paint = e2e::control_paint(window.window_id(), id).expect("running entry drawn");
        assert!(paint.bounds.size.width > gpui::px(0.));
        assert!(paint.bounds.left() >= paint.mask.left());
        assert!(paint.bounds.right() <= paint.mask.right() + gpui::px(1.));
        assert!(paint.bounds.top() >= paint.mask.top());
        assert!(paint.bounds.bottom() <= paint.mask.bottom() + gpui::px(1.));
    }
    assert!(cx.read(|cx| e2e::op_latched(app.read(cx))));
}

fn show_with_keyboard(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    key: &str,
) {
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            !e2e::active_modal_present(state),
            "hidden clone leaves no modal: notice={:?}, trust={}, clone={}",
            state.app_notice().map(|notice| &notice.message),
            state.trust_repo_modal().is_some(),
            state.clone_modal().is_some()
        );
    });
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        focus.focus(window, cx);
    })
    .unwrap();
    draw(cx, app, window);
    let mut reached = false;
    for _ in 0..120 {
        e2e::take_clone_busy_focus();
        crate::keyboard_nav::keys(cx, window, "tab");
        draw(cx, app, window);
        if e2e::take_clone_busy_focus() {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "Show Progress is reachable through production Tab navigation"
    );
    crate::keyboard_nav::keys(cx, window, key);
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_some()));
}

fn held_clone(cx: &mut VisualTestAppContext, success: bool) {
    let fixture = build_fixture();
    let other = build_fixture();
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
        "echo 'hidden clone fixture failure' >&2; exit 1".into()
    };
    let script = format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n'repo clone') {outcome} ;;\n*) echo '[]' ;;\nesac\n"
    );
    let _gh = OfflineGh::with_script(&script);
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    let (hold, release) = deferred(cx);
    e2e::take_clone_dispatches();
    e2e::queue_clone_run(hold);
    app.update(cx, |app, cx| {
        app.open_clone_card(
            RepoListing {
                name_with_owner: "acme/widgets".into(),
                host: "github.com".into(),
                updated_at: String::new(),
                description: String::new(),
                is_private: false,
                is_fork: false,
            },
            cx,
        );
        app.replan_clone(parent.path(), cx);
        app.start_clone(cx);
        app.cancel_clone();
        assert!(app.open_repository(other.path().to_owned(), cx));
    });
    cx.run_until_parked();
    crate::home_github::wait_for(cx, &app, "repository B idle", |app| {
        app.active_session()
            .is_some_and(|session| !app.reads.is_loading(session))
    });
    let owner = cx.read(|cx| app.read(cx).home_github.cloning.clone().unwrap());
    assert_eq!(e2e::take_clone_dispatches(), 1, "one clone dispatched");
    matrix(cx, &app, window);
    settings_busy_feedback(cx, &app, window);
    reopen_same(cx, &app, window, &owner);
    home_hidden(cx, &app, window);
    release.send(());
    crate::home_github::wait_for(cx, &app, "clone terminal report", |app| {
        app.home_github.cloning.is_none()
    });
    draw(cx, &app, window);
    assert!(
        e2e::take_clone_busy_status().is_none(),
        "terminal report removes status"
    );
    assert!(e2e::control_bounds(window.window_id(), "clone-busy-show").is_none());
    assert!(
        !cx.read(|cx| e2e::op_latched(app.read(cx))),
        "only terminal report releases latch"
    );
    assert_eq!(e2e::take_clone_dispatches(), 0);
    unmount(cx, app, window);
}

fn settings_busy_feedback(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repository: AnyWindowHandle,
) {
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "settings_pane",
        "settings_window_position",
        "settings_window_height",
        "settings_window_frame",
    ]);
    cx.simulate_keystrokes(repository, "cmd-,");
    cx.run_until_parked();
    let settings = cx
        .read(|cx| cx.windows())
        .into_iter()
        .find(|window| window.window_id() != repository.window_id())
        .expect("clone owner opens a separate Settings host");
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        e2e::clear_control_bounds(settings.window_id(), "toast-stack");
        draw(cx, app, settings);
        assert!(
            e2e::control_bounds(settings.window_id(), "clone-busy-status").is_some(),
            "Settings retains the running clone's busy feedback"
        );
        assert!(
            e2e::control_bounds(settings.window_id(), "toast-stack").is_some(),
            "Settings retains toast feedback"
        );
        assert!(
            e2e::control_bounds(settings.window_id(), "clone-busy-show").is_none(),
            "Settings must not expose an action that opens progress behind its host"
        );
        assert!(
            cx.read(|cx| app.read(cx).clone_modal().is_none()),
            "drawing Settings does not reopen the hidden clone card"
        );
        check_status(cx, app, repository);
    }
    cx.update_window(settings, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
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
                    check_status(cx, &app, window);
                }
            }
        }
    }
}

fn reopen_same(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    owner: &kagi::ui::home_github::CloneModal,
) {
    for key in ["enter", "space"] {
        show_with_keyboard(cx, &app, window, key);
        cx.read(|cx| {
            let card = app.read(cx).clone_modal().unwrap();
            assert_eq!(card.started, owner.started);
            assert_eq!(
                card.target.as_ref().unwrap().request,
                owner.target.as_ref().unwrap().request
            );
        });
        app.update(cx, |app, cx| {
            app.start_clone(cx);
            app.cancel_clone();
            assert!(
                app.app_sessions.running_lease().is_none(),
                "no second writer lease"
            );
            app.fetch_async(false, cx);
            assert!(
                app.fetch_in_flight.is_none(),
                "repository B write admission stays blocked"
            );
        });
        assert_eq!(
            e2e::take_clone_dispatches(),
            0,
            "reopen never spawns a second clone"
        );
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).app_notice().unwrap().message,
                kagi::ui::i18n::Msg::OpInProgress.t(),
                "repository B explains its blocked write"
            );
        });
        // The refusal owns the single modal slot until its normal dismissal.
        draw(cx, app, window);
        crate::keyboard_nav::keys(cx, window, "escape");
        draw(cx, app, window);
        assert!(cx.read(|cx| e2e::op_latched(app.read(cx))));
    }
}

fn home_hidden(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    for pane in [HomePane::Prs, HomePane::Issues, HomePane::Repos] {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.set_home_pane(pane, cx);
                if let Some(filter) = app.home_github.filter.clone() {
                    filter.update(cx, |input, cx| {
                        input.set_value("no-matching-clone", window, cx)
                    });
                }
            });
        })
        .unwrap();
        cx.run_until_parked();
        check_status(cx, &app, window);
    }
}

pub fn scenario_clone_hidden_status(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom", "recent_repos"]);
    held_clone(cx, true);
    held_clone(cx, false);
    eprintln!("[gui-e2e] PASS clone_hidden_status");
}
