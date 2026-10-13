//! Deferred native picker ownership and non-stealing clone completion (#1402/#1405).
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{Lang, Msg},
    theme, KagiApp,
};
use kagi_git::github_repos::RepoListing;

fn listing(name: &str) -> RepoListing {
    RepoListing {
        name_with_owner: format!("acme/{name}"),
        host: "github.com".into(),
        updated_at: String::new(),
        description: String::new(),
        is_private: false,
        is_fork: false,
    }
}
fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn unselected(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    assert!(
        cx.read(|cx| app.read(cx).clone_modal().unwrap().target.is_none()),
        "stale clone picker changed the replacement card's target"
    );
    draw(cx, window);
    assert!(
        kagi::ui::button_style::recorded_modal_button("clone-confirm")
            .unwrap()
            .disabled,
        "replacement card must not enable Clone without its own folder"
    );
}

pub fn scenario_clone_picker_owner(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let (app, window) = mount(cx, fixture.path());
    for same_listing in [false, true] {
        let (task, reply) = deferred(cx);
        e2e::queue_folder_answer(task);
        app.update(cx, |app, cx| {
            app.open_clone_card(listing("alpha"), cx);
            app.change_clone_parent(cx);
            app.cancel_clone();
            app.open_clone_card(listing(if same_listing { "alpha" } else { "beta" }), cx);
        });
        cx.run_until_parked();
        reply.send(Ok(Some(parent.path().to_owned())));
        cx.run_until_parked();
        unselected(cx, &app, window);
    }
    let (task, reply) = deferred(cx);
    e2e::queue_folder_answer(task);
    app.update(cx, |app, cx| app.change_clone_parent(cx));
    cx.run_until_parked();
    reply.send(Ok(Some(parent.path().to_owned())));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .clone_modal()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .request
            .dest
            .clone()),
        parent.path().join("alpha"),
        "the current card accepts its own folder"
    );
    assert_eq!(e2e::take_folder_prompts().len(), 3);
    picker_failures(cx, &app, window, fixture.path());
    home_picker_failure(cx, &app, window);
    app.update(cx, |app, _| app.cancel_clone());
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS clone_picker_owner");
}

fn picker_failures(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    use kagi_git::oplog::read_oplog_tail_for_repo;
    let before = read_oplog_tail_for_repo(repo, 100).len();
    let toasts = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .len()
    });
    e2e::answer_folder_prompt(None);
    app.update(cx, |app, cx| app.change_clone_parent(cx));
    cx.run_until_parked();
    assert_eq!(
        read_oplog_tail_for_repo(repo, 100).len(),
        before,
        "Cancel is not an operation"
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .len()),
        toasts,
        "Cancel is silent"
    );
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        for error in [
            "native folder dialog failed",
            "folder dialog channel disconnected",
        ] {
            picker_error(cx, app, window, repo, error);
        }
    }
    let replacement = tempfile::tempdir().unwrap();
    e2e::answer_folder_prompt(Some(replacement.path().to_owned()));
    app.update(cx, |app, cx| app.change_clone_parent(cx));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .clone_modal()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .request
            .dest
            .clone()),
        replacement.path().join("alpha"),
        "picker remains usable after failure"
    );
    e2e::take_folder_prompts();
}

fn picker_error(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
    error: &str,
) {
    use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
    let target = cx.read(|cx| {
        app.read(cx)
            .clone_modal()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .request
            .clone()
    });
    let (task, reply) = deferred(cx);
    e2e::queue_folder_answer(task);
    app.update(cx, |app, cx| app.change_clone_parent(cx));
    cx.run_until_parked();
    reply.send(Err(error.to_owned()));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .clone_modal()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .request
            .clone()),
        target,
        "failed picker preserves the card"
    );
    let receipts = read_oplog_tail_for_repo(repo, 100);
    assert!(receipts
        .iter()
        .any(|entry| entry.op == "clone-folder-picker"
            && matches!(&entry.outcome, OpOutcome::Failed { error: detail } if detail == error)));
    assert!(cx.read(|cx| {
        let state = app.read(cx);
        let toast = state
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap();
        toast.kind == kagi::ui::ToastKind::Error
            && toast.message.contains(error)
            && toast.message.contains(Msg::CloneChooseFolder.t())
    }));
    draw(cx, window);
}

fn home_picker_failure(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.cancel_clone();
            app.open_home_tab(window, cx);
        });
    })
    .unwrap();
    for close_repo in [false, true] {
        app.update(cx, |app, cx| {
            app.cancel_clone();
            if close_repo {
                app.close_tab(0, cx);
            }
            assert!(app.home_in_front());
            app.open_clone_card(listing("alpha"), cx);
        });
        let before = kagi_git::oplog::read_oplog_tail(100).len();
        let (task, reply) = deferred(cx);
        e2e::queue_folder_answer(task);
        app.update(cx, |app, cx| app.change_clone_parent(cx));
        cx.run_until_parked();
        reply.send(Err("Home folder dialog unavailable".into()));
        cx.run_until_parked();
        assert_eq!(
            kagi_git::oplog::read_oplog_tail(100).len(),
            before,
            "Home does not invent or attribute a background-repository receipt"
        );
        unselected(cx, app, window);
        assert!(cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.contains("Home folder dialog unavailable"))));
        e2e::take_folder_prompts();
    }
}

pub fn scenario_clone_completion_navigation(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang", "ui_zoom"]);
    let upstream = build_fixture();
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = bare_dir.path().join("widgets.git");
    git(
        upstream.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    let script = format!("#!/bin/sh\ncase \"$1 $2\" in\n'repo clone') git clone -q '{}' \"$4\" && git -C \"$4\" remote set-url origin https://github.com/acme/widgets.git ;;\n*) echo '[]' ;;\nesac\n", bare.display());
    let _gh = OfflineGh::with_script(&script);
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for dimensions in [(940., 660.), (1392., 883.)] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    for background in [true, false] {
                        completion(cx, dimensions, lang, zoom, background);
                    }
                }
            }
        }
    }
    eprintln!("[gui-e2e] PASS clone_completion_navigation");
}

fn completion(
    cx: &mut VisualTestAppContext,
    dimensions: (f32, f32),
    lang: Lang,
    zoom: f32,
    background: bool,
) {
    let fixture = build_fixture();
    let other = build_fixture();
    let parent = tempfile::tempdir().unwrap();
    let dest = parent.path().join("widgets");
    let (app, window) = mount(cx, fixture.path());
    crate::pr_detail_adaptive::resize(cx, window, dimensions);
    theme::set_zoom(zoom);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    let (hold, release) = deferred(cx);
    e2e::queue_clone_run(hold);
    app.update(cx, |app, cx| {
        app.open_clone_card(listing("widgets"), cx);
        app.replan_clone(parent.path(), cx);
        app.start_clone(cx);
    });
    cx.run_until_parked();
    if background {
        leave_clone(cx, &app, window, other.path());
    }
    let before = cx.read(|cx| {
        (
            app.read(cx).active_session(),
            app.read(cx).ui().selected,
            app.read(cx).tabs.len(),
        )
    });
    let focus = cx
        .update_window(window, |_, window, cx| window.focused(cx))
        .unwrap();
    release.send(());
    crate::home_github::wait_for(cx, &app, "clone completion", |app| {
        app.home_github.cloning.is_none()
    });
    let dest = dest.canonicalize().unwrap();
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(&dest, 100)
            .iter()
            .filter(|entry| entry.op == "clone"
                && matches!(entry.outcome, kagi_git::oplog::OpOutcome::Success { .. }))
            .count(),
        1,
        "one durable successful clone receipt"
    );
    if background {
        assert_eq!(
            cx.read(|cx| (
                app.read(cx).active_session(),
                app.read(cx).ui().selected,
                app.read(cx).tabs.len()
            )),
            before,
            "background clone completion stole the current repository context"
        );
        assert_eq!(
            cx.update_window(window, |_, window, cx| window.focused(cx))
                .unwrap(),
            focus,
            "completion must not move focus"
        );
        activate_completion(cx, &app, window, dimensions, lang);
    }
    assert_eq!(cx.read(|cx| app.read(cx).repo_path.clone()), Some(dest));
    unmount(cx, app, window);
}

fn leave_clone(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &std::path::Path,
) {
    app.update(cx, |app, cx| {
        app.cancel_clone();
        assert!(app.open_repository(repo.to_owned(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, _| app.select(1));
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        focus.focus(window, cx);
    })
    .unwrap();
}

fn activate_completion(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    dimensions: (f32, f32),
    lang: Lang,
) {
    draw(cx, window);
    let action = e2e::control_bounds(window.window_id(), "clone-completion-open")
        .expect("completed clone offers Open Repository");
    assert!(action.left() >= gpui::px(0.) && action.right() <= gpui::px(dimensions.0));
    assert!(action.top() >= gpui::px(0.) && action.bottom() <= gpui::px(dimensions.1));
    let action_focus = cx.read(|cx| {
        let state = app.read(cx);
        let stack = state.toast_stack.as_ref().unwrap().read(cx);
        let toast = stack.toasts().last().unwrap();
        assert!(toast.action.is_some());
        assert_eq!(toast.kind, kagi::ui::ToastKind::Success);
        let focus = toast
            .action_focus
            .as_ref()
            .unwrap()
            .borrow()
            .iter()
            .find(|(host, _)| *host == window.window_id())
            .unwrap()
            .1
            .clone();
        focus
    });
    if lang == Lang::Ja {
        let mut reached = false;
        for _ in 0..100 {
            crate::keyboard_nav::keys(cx, window, "tab");
            if cx
                .update_window(window, |_, window, _| action_focus.is_focused(window))
                .unwrap()
            {
                reached = true;
                break;
            }
        }
        assert!(reached, "Open Repository is keyboard reachable");
        crate::keyboard_nav::keys(cx, window, "enter");
    } else {
        cx.simulate_event(
            window,
            gpui::MouseDownEvent {
                position: action.center(),
                button: gpui::MouseButton::Left,
                click_count: 1,
                ..Default::default()
            },
        );
        cx.simulate_event(
            window,
            gpui::MouseUpEvent {
                position: action.center(),
                button: gpui::MouseButton::Left,
                click_count: 1,
                ..Default::default()
            },
        );
    }
    cx.run_until_parked();
}
