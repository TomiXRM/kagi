//! #1552: unchanged rename prefill is neutral and never actionable.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::i18n::Lang;
use kagi_git::ops::BranchRenameValidation;

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn assert_neutral_reason(language: Lang) {
    let expected = match language {
        Lang::En => "Enter a new name",
        Lang::Ja => "新しい名前を入力",
    };
    let button = kagi::ui::button_style::recorded_modal_button("branch-input-confirm").unwrap();
    assert!(button.disabled);
    assert_eq!(
        button.description.as_deref(),
        Some(expected),
        "neutral is not a running plan"
    );
}

pub fn scenario_branch_rename_neutral_state(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "recent_repos", "ui_zoom"]);
    let fixture = build_fixture();
    git_fixture::git(fixture.path(), &["branch", "occupied"]);
    let config = std::fs::read(fixture.path().join(".git/config")).unwrap();
    for (language, zoom) in [
        (Lang::En, 1.0),
        (Lang::Ja, 1.0),
        (Lang::En, 1.67),
        (Lang::Ja, 1.67),
    ] {
        kagi::ui::theme::set_zoom(zoom);
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            app.set_lang(language, cx);
            app.open_rename_branch_modal("main".into());
            cx.notify();
        });
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        cx.read(|cx| {
            let modal = app.read(cx).rename_branch_modal().unwrap();
            assert_eq!(modal.input, "main");
            assert!(
                modal.plan.plan().is_none(),
                "unchanged prefill must not create an error plan"
            );
            assert!(modal.error.is_none());
        });
        assert_neutral_reason(language);
        assert!(kagi::ui::e2e::control_bounds(window.window_id(), "input-field-error").is_none());
        let confirm = kagi::ui::e2e::confirm_bounds(window.window_id()).unwrap();
        cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).rename_branch_modal().is_some()));
        keys(cx, window, "enter");
        assert!(cx.read(|cx| app.read(cx).rename_branch_modal().is_some()));
        let input = cx.read(|cx| {
            app.read(cx)
                .rename_branch_modal()
                .unwrap()
                .input_state
                .clone()
                .unwrap()
        });
        for name in ["bad..name", "occupied", "new-valid-name", "main"] {
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-plan-blockers");
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value(name, window, cx));
                window.draw(cx).clear();
            })
            .unwrap();
            cx.run_until_parked();
            // Return forces the existing debounced replan before admission.
            if name != "new-valid-name" {
                keys(cx, window, "enter");
            }
            cx.read(|cx| {
                let modal = app.read(cx).rename_branch_modal().unwrap();
                assert_eq!(modal.input, name);
                if name == "main" {
                    assert!(
                        modal.plan.plan().is_none(),
                        "returning to old name retires Ready plan"
                    );
                } else if name == "bad..name" || name == "occupied" {
                    assert!(matches!(
                        modal.validation,
                        BranchRenameValidation::Invalid(_)
                    ));
                    assert!(!modal.plan.plan().unwrap().blockers.is_empty());
                }
            });
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-plan-blockers");
            cx.update_window(window, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            if name == "bad..name" || name == "occupied" {
                assert!(
                    kagi::ui::e2e::control_bounds(window.window_id(), "input-plan-blockers")
                        .is_none(),
                    "{name}: field validation must not repeat as a plan blocker"
                );
            }
            if name == "main" {
                assert_neutral_reason(language);
            }
        }
        keys(cx, window, "escape");
        assert!(cx.read(|cx| app.read(cx).rename_branch_modal().is_none()));
        assert_eq!(
            std::fs::read(fixture.path().join(".git/config")).unwrap(),
            config
        );
        assert_eq!(
            git_fixture::git_output(fixture.path(), &["branch", "--show-current"]),
            "main"
        );
        // A valid edit still goes through the existing rendered plan/approval path.
        app.update(cx, |app, cx| {
            app.open_rename_branch_modal("main".into());
            cx.notify();
        });
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        let accepted_input = cx.read(|cx| {
            app.read(cx)
                .rename_branch_modal()
                .unwrap()
                .input_state
                .clone()
                .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            accepted_input.update(cx, |state, cx| state.set_value("accepted-name", window, cx));
            window.draw(cx).clear();
        })
        .unwrap();
        cx.run_until_parked();
        keys(cx, window, "enter");
        crate::recovery_operations::wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).rename_branch_modal().is_none()));
        assert_eq!(
            git_fixture::git_output(fixture.path(), &["branch", "--show-current"]),
            "accepted-name"
        );
        unmount(cx, app, window);
        git_fixture::git(fixture.path(), &["branch", "-m", "accepted-name", "main"]);
    }
}
