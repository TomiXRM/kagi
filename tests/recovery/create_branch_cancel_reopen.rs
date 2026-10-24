//! #1749: cancelling a branch name must retire the actual editor, not just its label.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_ui_core::i18n::{self, Lang};
#[path = "../support/git_fixture.rs"]
mod git_fixture;

pub fn scenario_create_branch_cancel_reopen(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let head = kagi_git::CommitId(git_fixture::git_output(&repo, &["rev-parse", "HEAD"]));
        let refs_before = git_fixture::git_output(&repo, &["show-ref"]);
        let (app, window) = mount(cx, &repo);
        for cancel_by_escape in [false, true] {
            // This entry intentionally has no Window: creation must stay lazy.
            app.update(cx, |app, cx| {
                app.open_create_branch_modal(head.clone(), cx);
                let modal = app.create_branch_modal().unwrap();
                assert!(modal.input.is_empty());
                assert!(modal.input_state.is_none());
            });
            paint(cx, window);
            let old_input = cx.read(|cx| {
                app.read(cx)
                    .create_branch_modal()
                    .unwrap()
                    .input_state
                    .clone()
                    .unwrap()
            });
            click(cx, window, "input-modal-field");
            cx.simulate_keystrokes(window, "o l d");
            assert_name(cx, &app, window, "old");
            if cancel_by_escape {
                cx.simulate_keystrokes(window, "escape");
                cx.run_until_parked();
            } else {
                click(cx, window, "create-branch-cancel");
            }
            assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
            paint(cx, window);
            app.update(cx, |app, cx| app.open_create_branch_modal(head.clone(), cx));
            paint(cx, window);
            let new_input = cx.read(|cx| {
                app.read(cx)
                    .create_branch_modal()
                    .unwrap()
                    .input_state
                    .clone()
                    .unwrap()
            });
            assert_ne!(
                old_input.entity_id(),
                new_input.entity_id(),
                "reopen retires the cancelled editor"
            );
            assert_name(cx, &app, window, "");
            // Do not refocus from the test: the production reopen must route
            // typing to its new editor, not the cancelled editor still retained above.
            cx.simulate_keystrokes(window, "n e w");
            assert_name(cx, &app, window, "new");
            assert_eq!(cx.read(|cx| old_input.read(cx).value().to_string()), "old");
            click(cx, window, "create-branch-cancel");
            assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
            assert_eq!(git_fixture::git_output(&repo, &["show-ref"]), refs_before);
            assert_eq!(
                git_fixture::git_output(&repo, &["rev-parse", "HEAD"]),
                head.0
            );
            drop(new_input);
            drop(old_input);
        }
        // A windowless caller can seed the persistent model before the first
        // render. Lazy editor creation must not overwrite that name with empty text.
        app.update(cx, |app, cx| {
            app.open_create_branch_modal(head.clone(), cx);
            app.create_branch_modal_mut().unwrap().input = "seeded".into();
            assert!(app.create_branch_modal().unwrap().input_state.is_none());
        });
        assert_name(cx, &app, window, "seeded");
        click(cx, window, "create-branch-cancel");
        assert_eq!(git_fixture::git_output(&repo, &["show-ref"]), refs_before);
        unmount(cx, app, window);
    }
    i18n::set_lang(previous);
    eprintln!("[gui-e2e] PASS create_branch_cancel_reopen");
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let bounds = kagi::ui::e2e::control_bounds(window.window_id(), id).unwrap();
    cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

fn assert_name(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &str,
) {
    paint(cx, window);
    cx.read(|cx| {
        let modal = app.read(cx).create_branch_modal().unwrap();
        assert_eq!(modal.input, expected, "persistent branch name");
        assert_eq!(
            modal
                .input_state
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .as_ref(),
            expected,
            "actual editor text"
        );
    });
}
