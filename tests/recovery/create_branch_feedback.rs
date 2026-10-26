//! #1774: branch-name feedback timing, native wrapping and truthful AFTER copy.
use crate::macos::{build_fixture, unmount};
use crate::native_ax;
use crate::presentation_support::{mount, paint, Presentation};
use gpui::{px, AnyWindowHandle, Entity, Focusable, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::plan::BranchNameError;
use kagi_domain::plan_note::{CommonNote, PlanNote};
use kagi_ui_core::{
    i18n::{self, Lang, Msg},
    theme,
};
use std::time::{Duration, Instant};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

#[derive(Clone, Copy, Debug)]
enum Sheet {
    Branch,
    Worktree,
}

impl Sheet {
    fn open(self, cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, head: &kagi_git::CommitId) {
        app.update(cx, |app, cx| match self {
            Self::Branch => app.open_create_branch_modal(head.clone(), cx),
            Self::Worktree => app.open_create_worktree_modal(head.clone(), cx),
        });
    }

    fn confirm(self) -> &'static str {
        match self {
            Self::Branch => "create-branch-confirm",
            Self::Worktree => "create-worktree-confirm",
        }
    }

    fn cancel(self) -> &'static str {
        match self {
            Self::Branch => "create-branch-cancel",
            Self::Worktree => "create-worktree-cancel",
        }
    }

    fn name(self, cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> String {
        cx.read(|cx| match self {
            Self::Branch => app.read(cx).create_branch_modal().unwrap().input.clone(),
            Self::Worktree => app
                .read(cx)
                .create_worktree_modal()
                .unwrap()
                .branch_input
                .clone(),
        })
    }

    fn interacted(self, cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> bool {
        cx.read(|cx| match self {
            Self::Branch => {
                app.read(cx)
                    .create_branch_modal()
                    .unwrap()
                    .branch_name_interacted
            }
            Self::Worktree => {
                app.read(cx)
                    .create_worktree_modal()
                    .unwrap()
                    .branch_name_interacted
            }
        })
    }

    fn focused(
        self,
        cx: &mut VisualTestAppContext,
        app: &Entity<KagiApp>,
        window: AnyWindowHandle,
    ) {
        assert!(
            cx.update_window(window, |_, window, cx| {
                let input = match self {
                    Self::Branch => app
                        .read(cx)
                        .create_branch_modal()
                        .unwrap()
                        .input_state
                        .as_ref()
                        .unwrap(),
                    Self::Worktree => app
                        .read(cx)
                        .create_worktree_modal()
                        .unwrap()
                        .branch_state
                        .as_ref()
                        .unwrap(),
                };
                input.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap(),
            "{self:?}: branch-name input retains focus"
        );
    }
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let bounds = e2e::control_bounds(window.window_id(), id).unwrap();
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
    paint(cx, window);
}

fn wait_plan(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, sheet: Sheet) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        if cx.read(|cx| match sheet {
            Sheet::Branch => {
                let modal = app.read(cx).create_branch_modal().unwrap();
                !modal.plan_stale && modal.plan.plan().is_some()
            }
            Sheet::Worktree => {
                let modal = app.read(cx).create_worktree_modal().unwrap();
                !modal.plan_stale && modal.plan.plan().is_some()
            }
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{sheet:?}: real plan must settle"
        );
    }
}

fn alert(cx: &mut VisualTestAppContext, window: AnyWindowHandle, reason: &str, present: bool) {
    let tree = native_ax::tree(cx, window);
    let found = tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .any(|node| node["aria"]["role"] == "Alert" && node["aria"]["label"] == reason);
    assert_eq!(
        found, present,
        "adjacent full-text alert {reason:?}: {tree}"
    );
}

fn disabled(sheet: Sheet, reason: &str) {
    let button = kagi::ui::button_style::recorded_modal_button(sheet.confirm()).unwrap();
    assert_eq!(button.role, gpui::Role::Button);
    assert_eq!(button.label, Msg::InputCreate.t());
    assert!(button.disabled);
    assert_eq!(button.description.as_deref(), Some(reason));
}

pub fn scenario_create_branch_feedback(cx: &mut VisualTestAppContext) {
    let _restore = Presentation::keep();
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    let refs = git_fixture::git_output(repo, &["show-ref"]);
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for palette in ["apple-light", "apple-dark"] {
            theme::set_zoom(1.);
            let (app, window) = mount(cx, repo, (940., 660.));
            app.update(cx, |app, cx| app.set_theme(palette, cx));
            native_ax::activate(cx);
            for sheet in [Sheet::Branch, Sheet::Worktree] {
                sheet.open(cx, &app, &head);
                wait_plan(cx, &app, sheet);
                paint(cx, window);
                assert!(!sheet.interacted(cx, &app));
                assert_eq!(sheet.name(cx, &app), "");
                alert(cx, window, Msg::BranchNameEmpty.t(), false);
                disabled(sheet, Msg::BranchNameEmpty.t());
                sheet.focused(cx, &app, window);
                if matches!(sheet, Sheet::Worktree) {
                    let previous = cx.read(|cx| {
                        app.read(cx)
                            .create_worktree_modal()
                            .unwrap()
                            .plan
                            .plan()
                            .unwrap()
                            .clone()
                    });
                    cx.update_window(window, |_, window, cx| {
                        let input = app
                            .read(cx)
                            .create_worktree_modal()
                            .unwrap()
                            .path_state
                            .clone()
                            .unwrap();
                        input.update(cx, |input, cx| {
                            input.set_value("../edited-path", window, cx)
                        });
                    })
                    .unwrap();
                    paint(cx, window);
                    cx.read(|cx| {
                        let modal = app.read(cx).create_worktree_modal().unwrap();
                        assert!(
                            modal.plan_stale,
                            "path-only edits stay debounced even with an empty name"
                        );
                        assert!(std::sync::Arc::ptr_eq(
                            &previous,
                            modal.plan.plan().unwrap()
                        ));
                    });
                    wait_plan(cx, &app, sheet);
                    paint(cx, window);
                }
                // Measure both the first keystroke and edits after a ready plan.
                // The prior preview stays mounted throughout the debounce.
                for text in ["feature/demo", "x"] {
                    let card = e2e::control_bounds(window.window_id(), "modal-card").unwrap();
                    let field =
                        e2e::control_bounds(window.window_id(), "input-modal-field").unwrap();
                    cx.simulate_input(window, text);
                    paint(cx, window);
                    assert_eq!(
                        e2e::control_bounds(window.window_id(), "modal-card").unwrap(),
                        card,
                        "{sheet:?}: typing must not shrink or move the card"
                    );
                    assert_eq!(
                        e2e::control_bounds(window.window_id(), "input-modal-field").unwrap(),
                        field,
                        "{sheet:?}: typing must not move the focused field"
                    );
                    disabled(sheet, Msg::InputPlanPending.t());
                    wait_plan(cx, &app, sheet);
                    paint(cx, window);
                }
                click(cx, window, sheet.cancel());
                sheet.open(cx, &app, &head);
                wait_plan(cx, &app, sheet);
                paint(cx, window);

                // No debounce advance or plan wait: Return must explain this frame.
                cx.simulate_keystrokes(window, "enter");
                paint(cx, window);
                assert!(sheet.interacted(cx, &app));
                alert(cx, window, Msg::BranchNameEmpty.t(), true);
                disabled(sheet, Msg::BranchNameEmpty.t());
                sheet.focused(cx, &app, window);
                cx.read(|cx| {
                    assert!(
                        app.read(cx)
                            .toast_stack
                            .as_ref()
                            .unwrap()
                            .read(cx)
                            .toasts()
                            .is_empty(),
                        "Return on an empty field shows no operation toast"
                    );
                });
                assert!(
                    kagi_git::oplog::read_oplog_tail_for_repo(repo, 100).is_empty(),
                    "Return on an empty field is not an operation-log entry"
                );
                let single_line =
                    e2e::control_bounds(window.window_id(), "input-branch-name-error").unwrap();
                assert!(single_line.size.height > px(0.));

                click(cx, window, sheet.cancel());
                sheet.open(cx, &app, &head);
                wait_plan(cx, &app, sheet);
                paint(cx, window);
                assert!(
                    !sheet.interacted(cx, &app),
                    "reopening resets pristine state"
                );
                alert(cx, window, Msg::BranchNameEmpty.t(), false);
                sheet.focused(cx, &app, window);
                cx.simulate_input(window, "feature/demo");
                assert!(sheet.interacted(cx, &app));
                cx.simulate_keystrokes(window, "cmd-a backspace");
                paint(cx, window);
                assert_eq!(sheet.name(cx, &app), "");
                alert(cx, window, Msg::BranchNameEmpty.t(), true);
                disabled(sheet, Msg::BranchNameEmpty.t());
                sheet.focused(cx, &app, window);
                check_invalid(cx, &app, window, sheet, single_line.size.height);
                click(cx, window, sheet.cancel());
            }
            assert_eq!(git_fixture::git_output(repo, &["show-ref"]), refs);
            assert_eq!(
                git_fixture::git_output(repo, &["rev-parse", "HEAD"]),
                head.0
            );
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS create_branch_feedback");
}

fn check_invalid(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    sheet: Sheet,
    single_line: gpui::Pixels,
) {
    for name in [
        " feature/demo".to_owned(),
        format!(" feature/{}", "long-name-".repeat(18)),
    ] {
        cx.simulate_keystrokes(window, "cmd-a");
        cx.simulate_input(window, &name);
        wait_plan(cx, app, sheet);
        paint(cx, window);
        assert_eq!(
            sheet.name(cx, app),
            name,
            "leading space must not be trimmed"
        );
        let expected = BranchNameError::CreateInvalidRef(name.clone());
        cx.read(|cx| {
            let plan = match sheet {
                Sheet::Branch => app
                    .read(cx)
                    .create_branch_modal()
                    .unwrap()
                    .plan
                    .plan()
                    .unwrap(),
                Sheet::Worktree => app
                    .read(cx)
                    .create_worktree_modal()
                    .unwrap()
                    .plan
                    .plan()
                    .unwrap(),
            };
            assert!(plan
                .blockers
                .contains(&PlanNote::Common(CommonNote::BranchNameErrorKeyed(
                    expected.clone()
                ))));
        });
        let reason = i18n::branch_name_error(&expected);
        if name == " feature/demo" {
            assert_eq!(reason, match i18n::lang() {
                Lang::En => "Branch name ' feature/demo' is not a valid git ref name (no spaces, '..', or other invalid characters).",
                Lang::Ja => "branch 名 ' feature/demo' は有効な git ref 名ではありません(空白・'..' などは使えません)。",
            });
        }
        alert(cx, window, &reason, true);
        disabled(sheet, &reason);
        let bounds = e2e::control_bounds(window.window_id(), "input-branch-name-error").unwrap();
        if name == " feature/demo" && matches!(sheet, Sheet::Branch) && i18n::lang() == Lang::En {
            assert!(
                (f32::from(bounds.size.height) - 2. * f32::from(single_line)).abs() < 1.,
                "actual EN CreateInvalidRef must occupy two lines: {bounds:?}"
            );
        }
        if name.len() > 30 {
            assert!(
                bounds.size.height > single_line,
                "long reason wraps rather than truncates"
            );
            theme::set_zoom(1.67);
            app.update(cx, |_, cx| cx.notify());
            paint(cx, window);
            alert(cx, window, &reason, true);
            let enlarged =
                e2e::control_bounds(window.window_id(), "input-branch-name-error").unwrap();
            assert!(
                enlarged.size.height > bounds.size.height,
                "enlarged long reason retains all lines"
            );
            let confirm = e2e::confirm_bounds(window.window_id()).unwrap();
            assert!(
                confirm.top() >= px(0.) && confirm.bottom() <= px(660.),
                "Create stays pinned"
            );
            theme::set_zoom(1.);
            app.update(cx, |_, cx| cx.notify());
            paint(cx, window);
        }
    }
}

pub fn scenario_create_branch_after_unchanged(cx: &mut VisualTestAppContext) {
    let _restore = Presentation::keep();
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("unchanged.txt"), "retain untracked contents\n").unwrap();
    let head = kagi_git::CommitId(git_fixture::git_output(repo, &["rev-parse", "HEAD"]));
    let refs = git_fixture::git_output(repo, &["show-ref"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for palette in ["apple-light", "apple-dark"] {
            theme::set_zoom(1.);
            let (app, window) = mount(cx, repo, (940., 660.));
            app.update(cx, |app, cx| app.set_theme(palette, cx));
            Sheet::Branch.open(cx, &app, &head);
            paint(cx, window);
            cx.simulate_input(window, "feature/after-copy");
            wait_plan(cx, &app, Sheet::Branch);
            paint(cx, window);
            assert!(!cx.read(|cx| app.read(cx).create_branch_modal().unwrap().checkout_after));
            let sentence = match language {
                Lang::En => "HEAD, index and working tree stay unchanged.",
                Lang::Ja => "HEAD、index、working tree は変わりません。",
            };
            assert_eq!(Msg::InputBranchWorkingTreeUnchanged.t(), sentence);
            assert_eq!(e2e::last_plan_status_chip().as_deref(), Some(sentence));
            let bounds =
                e2e::control_bounds(window.window_id(), "create-branch-after-unchanged").unwrap();
            assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
            let (role, ax) = kagi::ui::dialog_a11y::recorded_note("plan-state-after").unwrap();
            assert_eq!(role, gpui::Role::Group);
            assert_eq!(ax, format!("{}: {sentence}", Msg::InputStashAfter.t()));
            // This input sheet has no Copy all button. Exercise the shared
            // production Copy all projection with its actual backend plan.
            let copy = cx.read(|cx| {
                e2e::plan_clipboard_text_for_e2e(
                    app.read(cx)
                        .create_branch_modal()
                        .unwrap()
                        .plan
                        .plan()
                        .unwrap(),
                )
            });
            assert!(
                copy.contains(sentence),
                "Copy all contains the visible consequence: {copy}"
            );
            assert_eq!(
                copy.matches(sentence).count(),
                1,
                "no hidden duplicate AFTER summary"
            );
            let checkbox =
                e2e::control_bounds(window.window_id(), "create-branch-checkout-after").unwrap();
            cx.simulate_click(
                window,
                gpui::point(checkbox.left() + px(20.), checkbox.center().y),
                Modifiers::none(),
            );
            wait_plan(cx, &app, Sheet::Branch);
            paint(cx, window);
            cx.read(|cx| {
                let modal = app.read(cx).create_branch_modal().unwrap();
                assert!(modal.checkout_after);
                let plan = modal.plan.plan().unwrap();
                assert!(plan.blockers.is_empty());
                assert!(plan.predicted.head.contains("feature/after-copy"));
                assert_ne!(plan.predicted.head, plan.current.head);
            });
            let (_, ax) = kagi::ui::dialog_a11y::recorded_note("plan-state-after").unwrap();
            assert!(
                !ax.contains(sentence),
                "checkout-on must not claim HEAD is unchanged"
            );
            assert!(
                ax.contains("feature/after-copy"),
                "checkout-on exposes actual predicted HEAD: {ax}"
            );
            let copy = cx.read(|cx| {
                e2e::plan_clipboard_text_for_e2e(
                    app.read(cx)
                        .create_branch_modal()
                        .unwrap()
                        .plan
                        .plan()
                        .unwrap(),
                )
            });
            assert!(!copy.contains(sentence));
            assert!(copy.contains("feature/after-copy"));
            click(cx, window, Sheet::Branch.cancel());
            assert_eq!(git_fixture::git_output(repo, &["show-ref"]), refs);
            assert_eq!(
                git_fixture::git_output(repo, &["rev-parse", "HEAD"]),
                head.0
            );
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read_to_string(repo.join("unchanged.txt")).unwrap(),
                "retain untracked contents\n"
            );
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS create_branch_after_unchanged");
}
