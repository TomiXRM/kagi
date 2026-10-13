//! #1043 / #1383: the folder button uses production's async completion path.
//! Deferred answers must remain owned by the requesting form and tab visit.
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_domain::plan_note::{CommonNote, PlanNote};
use kagi_git::CommitId;
use kagi_ui_core::{
    i18n::{Lang, Msg},
    theme,
};

use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as output;

const BUTTON: &str = "create-worktree-choose-folder";
const INPUT: &str = "create-worktree-choose-folder-input";

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn settle(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    what: &str,
    predicate: impl Fn(&KagiApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        paint(cx, window);
        cx.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        if cx.read(|cx| predicate(app.read(cx))) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn path_input(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (String, bool) {
    cx.read(|cx| {
        let modal = app.read(cx).create_worktree_modal().expect("card open");
        (modal.path_input.clone(), modal.path_touched)
    })
}

fn click_button(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), BUTTON)
        .expect("the folder button is drawn");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn form_snapshot(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> (String, bool, String, u64) {
    cx.read(|cx| {
        let app = app.read(cx);
        let modal = app.create_worktree_modal().expect("card open");
        assert_eq!(
            modal.path_state.as_ref().unwrap().read(cx).value().as_str(),
            modal.path_input,
            "the path entity and form agree"
        );
        (
            modal.path_input.clone(),
            modal.path_touched,
            format!("{:?}", modal.plan),
            app.modal_replan_gen,
        )
    })
}

fn plan_count() -> usize {
    kagi_ui_core::klog::tail()
        .iter()
        .filter(|line| line.starts_with("[kagi] plan: create-worktree '"))
        .count()
}

fn reopen(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    head: &CommitId,
) {
    app.update(cx, |app, cx| {
        app.cancel_create_worktree_modal();
        app.open_create_worktree_modal_prefilled(head.clone(), "picked/one".into(), false, cx);
        cx.notify();
    });
    settle(cx, app, window, "replacement form", |app| {
        app.create_worktree_modal()
            .is_some_and(|m| m.path_state.is_some() && m.plan.plan().is_some())
    });
}

fn stale_answers(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    head: &CommitId,
    chosen: &std::path::Path,
) {
    // A and B deliberately use the same branch name: matching input text is
    // not matching the form which opened the system chooser.
    let (task, answer) = deferred::<Option<std::path::PathBuf>>(cx);
    kagi::ui::e2e::queue_folder_answer(task);
    click_button(cx, window);
    assert_eq!(kagi::ui::e2e::take_folder_prompts().len(), 1);
    reopen(cx, app, window, head);
    let before = form_snapshot(cx, app);
    let plans = plan_count();
    answer.send(Some(chosen.to_path_buf()));
    cx.run_until_parked();
    paint(cx, window);
    cx.advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(
        form_snapshot(cx, app), before,
        "late chooser A must not change replacement B's path, touched flag, plan or replan generation"
    );
    assert_eq!(plan_count(), plans, "a stale answer must not replan B");

    // Reuse the original Input entity after departure/return to independently
    // exercise attachment visit ownership rather than form replacement.
    let original = cx.read(|cx| app.read(cx).create_worktree_modal().unwrap().clone());
    let (task, answer) = deferred::<Option<std::path::PathBuf>>(cx);
    kagi::ui::e2e::queue_folder_answer(task);
    click_button(cx, window);
    assert_eq!(kagi::ui::e2e::take_folder_prompts().len(), 1);
    let other = build_fixture();
    assert!(app.update(cx, |app, cx| app
        .open_repository(other.path().to_path_buf(), cx)));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.set_create_worktree_modal(original);
        cx.notify();
    });
    paint(cx, window);
    let before = form_snapshot(cx, app);
    let plans = plan_count();
    answer.send(Some(chosen.to_path_buf()));
    cx.run_until_parked();
    paint(cx, window);
    cx.advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    assert_eq!(
        form_snapshot(cx, app),
        before,
        "tab departure/return must retire chooser A"
    );
    assert_eq!(plan_count(), plans, "a retired visit must not replan");
    app.update(cx, |app, cx| app.close_tab(1, cx));
    cx.run_until_parked();
}

pub fn scenario_worktree_folder_picker(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let chosen_dir = tempfile::tempdir().unwrap();
    let chosen = chosen_dir.path().canonicalize().unwrap();
    let state = kagi::ui::e2e::app_state(&repo).unwrap();
    let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
    let out = captured.clone();
    let window = crate::macos::open_offscreen(
        cx,
        gpui::size(gpui::px(940.), gpui::px(660.)),
        move |window, cx| kagi::ui::e2e::mount_root(state, window, cx, &out),
    );
    let app = captured.borrow().clone().unwrap();
    drop(captured);
    let window: AnyWindowHandle = window.into();
    cx.run_until_parked();
    theme::set_zoom(1.67);

    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_create_worktree_modal(head.clone(), cx);
        });
        settle(cx, &app, window, "the opening plan", |app| {
            app.create_worktree_modal()
                .is_some_and(|m| m.branch_state.is_some() && m.path_state.is_some())
        });
        // Type the branch name as the user would.
        let branch_state = cx.read(|cx| {
            app.read(cx)
                .create_worktree_modal()
                .and_then(|m| m.branch_state.clone())
                .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            branch_state.update(cx, |input, cx| input.set_value("picked/one", window, cx));
        })
        .unwrap();
        settle(cx, &app, window, "the typed branch's plan", |app| {
            app.create_worktree_modal().is_some_and(|m| {
                m.branch_input == "picked/one"
                    && m.path_input.ends_with("/picked-one")
                    && m.plan.plan().is_some()
            })
        });

        // Drawn left of the input, a scaled 24px square named for AT.
        assert_eq!(
            kagi::ui::button_style::recorded_modal_button(BUTTON),
            Some(kagi::ui::button_style::ModalButtonA11y {
                role: Role::Button,
                label: Msg::InputChooseFolder.t().to_owned(),
                description: None,
                disabled: false,
            }),
            "{lang:?}: the folder button is a named, enabled button"
        );
        let button = kagi::ui::e2e::control_bounds(window.window_id(), BUTTON).unwrap();
        let input = kagi::ui::e2e::control_bounds(window.window_id(), INPUT)
            .expect("the path input is drawn");
        let side = theme::scaled_px(24.);
        assert!(
            f32::from(button.size.width - side).abs() <= 0.5
                && f32::from(button.size.height - side).abs() <= 0.5,
            "{lang:?}: the folder button is a scaled 24px square: {button:?}"
        );
        assert!(
            button.right() <= input.left(),
            "{lang:?}: the folder button sits left of the input: {button:?} / {input:?}"
        );
        assert!(
            f32::from(input.size.height - theme::scaled_px(32.)).abs() <= 0.5,
            "{lang:?}: the input keeps its 32px height: {input:?}"
        );
        assert!(
            f32::from((button.center().y - input.center().y).abs()) <= 1.0,
            "{lang:?}: the button is centred on the input row"
        );
        // The input takes the rest of the row: it ends where the card's
        // full-width comparison ends, one small gap after the button.
        let comparison = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-comparison")
            .expect("the comparison is drawn");
        assert!(
            f32::from((input.right() - comparison.right()).abs()) <= 1.0
                && f32::from(input.left() - button.right()) <= f32::from(theme::scaled_px(4.)) + 1.0,
            "{lang:?}: the input gives up only the button's width: {button:?} / {input:?} / {comparison:?}"
        );

        stale_answers(cx, &app, window, &head, &chosen);

        // Cancel: the request is made, nothing changes.
        let (default_path, touched) = path_input(cx, &app);
        assert!(!touched, "the default path follows the branch name");
        let unchanged = form_snapshot(cx, &app);
        let plans = plan_count();
        kagi::ui::e2e::answer_folder_prompt(None);
        click_button(cx, window);
        assert_eq!(
            kagi::ui::e2e::take_folder_prompts(),
            vec![Msg::InputChooseFolder.t().to_owned()],
            "{lang:?}: a click asks for one folder"
        );
        paint(cx, window);
        cx.advance_clock(Duration::from_millis(600));
        cx.run_until_parked();
        assert_eq!(
            form_snapshot(cx, &app),
            unchanged,
            "chooser cancel changes no form state"
        );
        assert_eq!(plan_count(), plans, "chooser cancel does not replan");
        assert_eq!(
            path_input(cx, &app),
            (default_path, false),
            "{lang:?}: cancelling the dialog leaves the path alone"
        );

        // Choose: the field becomes a new folder for the branch inside the
        // chosen one and is planned as typed input.
        let (task, answer) = deferred::<Option<std::path::PathBuf>>(cx);
        kagi::ui::e2e::queue_folder_answer(task);
        let before = form_snapshot(cx, &app);
        let plans = plan_count();
        click_button(cx, window);
        assert_eq!(kagi::ui::e2e::take_folder_prompts().len(), 1);
        assert_eq!(
            form_snapshot(cx, &app),
            before,
            "pending chooser changes nothing"
        );
        answer.send(Some(chosen.clone()));
        cx.run_until_parked();
        let expected = chosen.join("picked-one").display().to_string();
        settle(cx, &app, window, "the chosen folder's plan", |app| {
            app.create_worktree_modal().is_some_and(|m| {
                m.plan.plan().is_some_and(|p| {
                    p.recovery
                        .as_ref()
                        .is_some_and(|r| r.commands.iter().any(|c| c.contains(&expected)))
                })
            })
        });
        assert_eq!(
            path_input(cx, &app),
            (expected.clone(), true),
            "{lang:?}: the chosen folder is the user's path"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).modal_replan_gen),
            before.3 + 1,
            "the same form receives one path edit"
        );
        assert_eq!(
            plan_count(),
            plans + 1,
            "the same form replans exactly once"
        );
        let blockers = cx.read(|cx| {
            app.read(cx)
                .create_worktree_modal()
                .and_then(|m| m.plan.plan().map(|p| p.blockers.clone()))
                .unwrap()
        });
        assert!(
            !blockers.iter().any(|b| matches!(
                b,
                PlanNote::Common(
                    CommonNote::WorktreePathErrorKeyed(_) | CommonNote::GitErrorPassthrough { .. }
                )
            )),
            "{lang:?}: a folder inside the chosen one is a valid new path: {blockers:?}"
        );
        assert!(
            !std::path::Path::new(&expected).exists(),
            "choosing a folder writes nothing"
        );
        assert!(
            output(
                &repo,
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/heads/picked/one"
                ]
            )
            .is_empty(),
            "folder selection creates no branch ref"
        );

        app.update(cx, |app, cx| {
            app.cancel_create_worktree_modal();
            cx.notify();
        });
        paint(cx, window);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_folder_picker");
}

/// #1045: the branch menu's prefill survives lazy InputState creation.
pub fn scenario_worktree_modal_prefill(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    crate::macos::git(&repo, &["branch", "prefill/topic"]);
    let (app, window) = mount(cx, &repo);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.open_local_branch_menu(
                "prefill/topic".into(),
                gpui::point(gpui::px(400.), gpui::px(40.)),
            );
            cx.notify();
        });
        paint(cx, window);
        crate::context_menu_keys::focus_menu_control(cx, &app, window, "branch-menu-item-3-1");
        crate::keyboard_nav::keys(cx, window, "right");
        paint(cx, window);
        let create =
            kagi::ui::e2e::control_bounds(window.window_id(), "branch-menu-item-3-1-submenu-1-1")
                .expect("Create worktree here is drawn in the Worktree submenu");
        cx.simulate_click(window, create.center(), gpui::Modifiers::none());
        settle(cx, &app, window, "prefilled modal inputs", |app| {
            app.create_worktree_modal()
                .is_some_and(|m| m.branch_state.is_some() && m.path_state.is_some())
        });
        cx.read(|cx| {
            let modal = app
                .read(cx)
                .create_worktree_modal()
                .expect("worktree modal open");
            assert_eq!(
                modal
                    .branch_state
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .value()
                    .as_str(),
                "prefill/topic",
                "{lang:?}: branch Input must preserve the context-menu prefill"
            );
            assert_eq!(modal.branch_input, "prefill/topic");
            assert!(
                modal.path_input.ends_with("/prefill-topic"),
                "{lang:?}: {}",
                modal.path_input
            );
            assert_eq!(
                modal.path_state.as_ref().unwrap().read(cx).value().as_str(),
                modal.path_input
            );
            assert!(!modal.path_touched, "the suggestion is not a user edit");
        });
        app.update(cx, |app, cx| {
            app.cancel_create_worktree_modal();
            cx.notify();
        });
        paint(cx, window);
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_modal_prefill");
}
