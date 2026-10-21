//! #1564: a branch-origin new-worktree form must not propose an existing branch.
use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_ui_core::i18n::{Lang, Msg};

use crate::macos::{build_fixture, mount, unmount};
#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as output;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn open_from_menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    item: usize,
) {
    app.update(cx, |app, cx| {
        app.open_local_branch_menu(
            "prefill/topic".into(),
            gpui::point(gpui::px(400.), gpui::px(40.)),
        );
        cx.notify();
    });
    paint(cx, window);
    crate::context_menu_keys::focus_menu_control(cx, app, window, "branch-menu-item-3-1");
    crate::keyboard_nav::keys(cx, window, "right");
    paint(cx, window);
    let id = format!("branch-menu-item-3-1-submenu-1-{item}");
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), &id)
        .expect("the production Worktree submenu action is drawn");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    paint(cx, window);
}

pub fn scenario_worktree_create_default_name(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    crate::macos::git(&repo, &["branch", "prefill/topic"]);
    let before = output(&repo, &["show-ref", "--heads"]);
    let (app, window) = mount(cx, &repo);
    crate::native_ax::activate(cx);
    for lang in [Lang::En, Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        open_from_menu(cx, &app, window, 1);
        cx.read(|cx| {
            let modal = app.read(cx).create_worktree_modal().unwrap();
            assert!(
                modal.branch_input.is_empty(),
                "{lang:?}: Create worktree here must request a NEW branch, not prefill the existing source: {}",
                modal.branch_input
            );
            assert!(!modal.allow_existing_branch);
            assert!(modal.branch_state.as_ref().unwrap().read(cx).value().is_empty());
            assert!(modal.path_input.ends_with("/new-branch"));
            assert!(!modal.path_touched);
        });
        let tree = crate::native_ax::tree(cx, window).to_string();
        assert!(!tree.contains("already exists"), "{lang:?}: {tree}");
        assert!(
            tree.contains(Msg::InputNewBranchName.t()),
            "{lang:?}: {tree}"
        );
        assert!(
            kagi::ui::button_style::recorded_modal_button("create-worktree-confirm")
                .unwrap()
                .disabled
        );
        // Return on an unfilled new-name request is neutral, not a refused op.
        let oplog = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
            .join("operations.jsonl");
        let receipts = std::fs::read(&oplog).unwrap_or_default();
        let feedback = |cx: &mut VisualTestAppContext| {
            cx.read(|cx| {
                let state = app.read(cx);
                let toasts = state.toast_stack.as_ref().unwrap().read(cx);
                (
                    format!("{:?}", state.status_footer),
                    toasts
                        .toasts()
                        .iter()
                        .map(|toast| (toast.id, format!("{:?}", toast.kind), toast.message.clone()))
                        .collect::<Vec<_>>(),
                )
            })
        };
        let before_return = feedback(cx);
        crate::keyboard_nav::keys(cx, window, "enter");
        paint(cx, window);
        assert_eq!(
            std::fs::read(&oplog).unwrap_or_default(),
            receipts,
            "{lang:?}: Return on an empty new-branch request must not record Refused"
        );
        assert_eq!(
            feedback(cx),
            before_return,
            "{lang:?}: empty Return must not change footer or toasts"
        );
        assert!(cx.read(|cx| app.read(cx).create_worktree_modal().is_some()));
        let branch_state = cx.read(|cx| {
            app.read(cx)
                .create_worktree_modal()
                .unwrap()
                .branch_state
                .clone()
                .unwrap()
        });
        // Ordinary user input still receives collision validation.
        cx.update_window(window, |_, window, cx| {
            branch_state.update(cx, |input, cx| input.set_value("prefill/topic", window, cx));
        })
        .unwrap();
        paint(cx, window);
        cx.read(|cx| {
            assert!(!app
                .read(cx)
                .create_worktree_modal()
                .unwrap()
                .plan
                .plan()
                .unwrap()
                .blockers
                .is_empty())
        });
        cx.update_window(window, |_, window, cx| {
            branch_state.update(cx, |input, cx| input.set_value("fresh/topic", window, cx));
        })
        .unwrap();
        paint(cx, window);
        cx.read(|cx| {
            let modal = app.read(cx).create_worktree_modal().unwrap();
            assert_eq!(modal.branch_input, "fresh/topic");
            assert!(modal.path_input.ends_with("/fresh-topic"));
            assert!(!modal.path_touched);
            assert!(modal.plan.plan().unwrap().blockers.is_empty());
            assert!(
                modal.plan.plan().unwrap().warnings.is_empty(),
                "plain worktree creation has neutral scope, not a caution"
            );
            assert!(!modal.allow_existing_branch);
        });
        let expected_scope = cx.read(|cx| {
            let modal = app.read(cx).create_worktree_modal().unwrap();
            let plan = modal.plan.plan().unwrap();
            let kagi_domain::plan_note::PlanTitle::Worktree(
                kagi_domain::plan_note::WorktreeTitle::CreateWorktree { branch, start },
            ) = &plan.title
            else {
                panic!("worktree title must capture its branch and start")
            };
            assert_eq!(branch, "fresh/topic");
            assert!(!start.is_empty());
            i18n_scope(plan, branch, start)
        });
        let (_, after) = kagi::ui::dialog_a11y::recorded_note("plan-state-after").unwrap();
        assert!(
            after.contains(&expected_scope),
            "worktree AFTER lost path/branch/start: {after}"
        );
        let primary =
            kagi::ui::button_style::recorded_modal_button("create-worktree-confirm").unwrap();
        assert_eq!(primary.label, Msg::InputCreate.t());
        assert!(
            !primary.disabled,
            "{lang:?}: valid new branch enables Create"
        );
        app.update(cx, |app, cx| {
            app.cancel_create_worktree_modal();
            cx.notify();
        });
        paint(cx, window);
        // The separate existing-branch action deliberately retains its prefill.
        open_from_menu(cx, &app, window, 0);
        cx.read(|cx| {
            let modal = app.read(cx).create_worktree_modal().unwrap();
            assert!(modal.allow_existing_branch);
            assert_eq!(modal.branch_input, "prefill/topic");
            assert!(modal.path_input.ends_with("/prefill-topic"));
            assert!(modal.plan.plan().unwrap().blockers.is_empty());
        });
        app.update(cx, |app, cx| {
            app.cancel_create_worktree_modal();
            cx.notify();
        });
        paint(cx, window);
    }
    assert_eq!(
        output(&repo, &["show-ref", "--heads"]),
        before,
        "opening, editing and cancelling either form writes no refs"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS worktree_create_default_name");
}

fn i18n_scope(plan: &kagi_domain::plan::OperationPlan, branch: &str, start: &str) -> String {
    let Some(kagi_domain::plan_note::RecoveryKind::Worktree(
        kagi_domain::plan_note::WorktreeRecovery::CreateWorktree { path, .. },
    )) = plan.recovery.as_ref().map(|recovery| &recovery.kind)
    else {
        panic!("approved worktree plan must retain its destination")
    };
    assert!(std::path::Path::new(path).ends_with("fresh-topic"));
    let scope = kagi_ui_core::i18n::plan_note_text(&kagi_domain::plan_note::PlanNote::Worktree(
        kagi_domain::plan_note::WorktreeNote::CreatesLinkedWorktree {
            path: path.into(),
            branch: branch.into(),
            start: start.into(),
        },
    ));
    assert!(
        kagi_ui_core::i18n::plan::plan_after_state_detail(plan).contains(&scope),
        "approved worktree scope must survive without warning notes"
    );
    scope
}
