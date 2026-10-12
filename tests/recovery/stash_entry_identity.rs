use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::app::PlanState;
use kagi::ui::{e2e, i18n};

fn selected_stash_fixture(duplicate: bool) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let message = format!(
        "selected \u{1b}[31m{}tail",
        "長い stash メッセージ ".repeat(60)
    );
    for (contents, name) in [
        (
            "older\n",
            if duplicate {
                message.as_str()
            } else {
                "older entry"
            },
        ),
        ("selected\n", message.as_str()),
        (
            "newer\n",
            if duplicate {
                message.as_str()
            } else {
                "newer entry"
            },
        ),
    ] {
        std::fs::write(repo.join("README.md"), contents).unwrap();
        git(&repo, &["stash", "push", "-qm", name]);
    }
    let expected = format!(
        "stash@{{1}}: On main: selected \\x1B[31m{}tail",
        "長い stash メッセージ ".repeat(60)
    );
    (fixture, repo, expected)
}

fn approve_and_assert_refused(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
    repo: &std::path::Path,
) {
    crate::recovery_operations::paint(cx, window);
    let confirm = e2e::confirm_bounds(window.window_id()).unwrap();
    assert!(
        confirm.bottom() <= gpui::px(900.),
        "long target must not displace the footer"
    );
    cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let receipts = kagi_git::oplog::read_oplog_tail_for_repo(repo, 10);
    assert!(
        receipts
            .iter()
            .any(|entry| matches!(entry.outcome, kagi_git::oplog::OpOutcome::Refused { .. })),
        "drift must refuse the actual GUI approval"
    );
}

pub fn scenario_stash_entry_identity(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom"]);
    let original = i18n::lang();
    let zoom = kagi::ui::theme::zoom();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        kagi::ui::theme::set_zoom(if lang == i18n::Lang::Ja { 1.67 } else { 1. });
        for action in ["apply", "pop", "drop"] {
            let (_fixture, repo, expected) = selected_stash_fixture(lang == i18n::Lang::Ja);
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| match action {
                "apply" => app.open_stash_apply_modal(1, cx),
                "pop" => app.open_pop_modal(1, cx),
                _ => app.open_stash_drop_modal(1, cx),
            });
            cx.run_until_parked();
            assert!(cx.read(|cx| matches!(
                app.read(cx).app_sessions.plan_state(),
                PlanState::Ready { .. }
            )));
            kagi::ui::take_stash_entry_label_for_e2e();
            crate::recovery_operations::paint(cx, window);
            assert_eq!(
                kagi::ui::take_stash_entry_label_for_e2e().as_deref(),
                Some(expected.as_str()),
                "selected entry must be rendered and supplied to AX"
            );
            assert!(
                e2e::control_paint(window.window_id(), "stash-entry-identity")
                    .unwrap()
                    .middle_ellipsis
            );
            // Replace the selected entry without changing the stack depth.
            git(&repo, &["stash", "drop", "-q", "stash@{1}"]);
            std::fs::write(repo.join("README.md"), "replacement\n").unwrap();
            git(&repo, &["stash", "push", "-qm", "replacement"]);
            let mut backend = git2::Repository::open(&repo).unwrap();
            let plan = cx.read(|cx| match action {
                "apply" => app
                    .read(cx)
                    .stash_apply_modal()
                    .unwrap()
                    .plan
                    .clone()
                    .unwrap(),
                "pop" => app.read(cx).pop_modal().unwrap().plan.clone().unwrap(),
                _ => app
                    .read(cx)
                    .stash_drop_modal()
                    .unwrap()
                    .plan
                    .clone()
                    .unwrap(),
            });
            let error = kagi_git::preflight_check_stash(&mut backend, &plan, 3).unwrap_err();
            let reason = i18n::plan_note_text(error.blocker().unwrap());
            assert!(reason.contains("stash@{1}"));
            approve_and_assert_refused(cx, window, &repo);
            assert_eq!(
                std::fs::read(repo.join("README.md")).unwrap(),
                b"# fixture\nsecond line\n"
            );
            drop(backend);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original);
    kagi::ui::theme::set_zoom(zoom);
}
