//! #1385: real ruleset and checkout-after warnings remain advisory and visible.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi_domain::ruleset::{Pattern, PatternOp, Ruleset, RulesetStatus};
use kagi_ui_core::i18n::{self, Lang};
#[path = "../support/git_fixture.rs"]
mod git_fixture;

pub fn scenario_create_branch_warnings(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let previous = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        std::fs::write(repo.join("untracked-warning.txt"), "keep me\n").unwrap();
        let head = kagi_git::CommitId(git_fixture::git_output(&repo, &["rev-parse", "HEAD"]));
        let branch = git_fixture::git_output(&repo, &["symbolic-ref", "--short", "HEAD"]);
        let workdir = kagi_git::Backend::open(&repo).unwrap().workdir().unwrap();
        kagi_git::ruleset::seed_ruleset(
            &workdir,
            &branch,
            RulesetStatus::Active(Ruleset {
                branch_name: Some(Pattern {
                    operator: PatternOp::StartsWith,
                    pattern: "feature/".into(),
                    negate: false,
                }),
                ..Ruleset::default()
            }),
        );
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.open_create_branch_modal(head, cx);
            app.create_branch_modal_mut().unwrap().checkout_after = true;
        });
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        let input = cx
            .read(|cx| {
                app.read(cx)
                    .create_branch_modal()
                    .unwrap()
                    .input_state
                    .clone()
            })
            .unwrap();
        cx.update_window(window, |_, window, cx| {
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.draw(cx).clear();
        })
        .unwrap();
        cx.simulate_keystrokes(window, "f e a t");
        assert_warned_plan(cx, &app, window, language);
        check_viewports(cx, &app, window);
        drop(input);
        unmount(cx, app, window);
    }
    i18n::set_lang(previous);
    eprintln!("[gui-e2e] PASS create_branch_warnings");
}

fn check_viewports(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
) {
    let previous_zoom = kagi_ui_core::theme::zoom();
    for dimensions in [(940., 660.), (1392., 883.)] {
        crate::pr_detail_adaptive::resize(cx, window, dimensions);
        for zoom in [1., 1.67] {
            kagi_ui_core::theme::set_zoom(zoom);
            app.update(cx, |_, cx| cx.notify());
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let confirm = kagi::ui::e2e::confirm_bounds(window.window_id()).unwrap();
            let cancel =
                kagi::ui::e2e::control_bounds(window.window_id(), "create-branch-cancel").unwrap();
            for bounds in [confirm, cancel] {
                assert!(bounds.left() >= gpui::px(0.) && bounds.right() <= gpui::px(dimensions.0));
                assert!(
                    bounds.top() >= gpui::px(0.) && bounds.bottom() <= gpui::px(dimensions.1),
                    "footer remains inside viewport at {dimensions:?}/{zoom}"
                );
            }
            for index in 0..2 {
                let warning = kagi::ui::e2e::control_bounds(
                    window.window_id(),
                    &format!("create-branch-warning-{index}"),
                )
                .unwrap();
                assert!(warning.size.height > gpui::px(0.));
                assert!(
                    warning.left() >= gpui::px(0.) && warning.right() <= gpui::px(dimensions.0)
                );
                assert!(
                    warning.bottom() <= confirm.top(),
                    "warnings stay inside the body above the pinned footer"
                );
            }
        }
    }
    kagi_ui_core::theme::set_zoom(previous_zoom);
}

fn assert_warned_plan(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
    language: Lang,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .unwrap()
                .plan
                .plan()
                .is_some_and(|p| p.blockers.is_empty() && p.warnings.len() == 2)
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "real plan must contain both advisory warnings"
        );
    }
    kagi::ui::dialog_a11y::clear_recorded_a11y();
    kagi::ui::button_style::clear_recorded_modal_buttons();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let warnings = cx.read(|cx| {
        app.read(cx)
            .create_branch_modal()
            .unwrap()
            .plan
            .plan()
            .unwrap()
            .warnings
            .clone()
    });
    for (index, warning) in warnings.iter().enumerate() {
        assert_eq!(
            kagi::ui::dialog_a11y::recorded_note(&format!("create-branch-warning-{index}")),
            Some((gpui::Role::Status, i18n::plan_note_text(warning))),
            "{language:?}: warning {index} must be rendered with localized Status role"
        );
        assert!(
            kagi::ui::e2e::control_bounds(
                window.window_id(),
                &format!("create-branch-warning-{index}")
            )
            .is_some(),
            "warning must have rendered bounds"
        );
    }
    assert!(
        !kagi::ui::button_style::recorded_modal_button("create-branch-confirm")
            .unwrap()
            .disabled,
        "warnings must not disable Create"
    );
}
