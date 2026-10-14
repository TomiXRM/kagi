//! #1561: real HTTP Basic 401 fetch, actionable summary and complete Copy.
#[path = "../support/git_fixture.rs"]
mod git_fixture;
#[path = "../support/http_basic_401.rs"]
mod http_basic_401;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::VisualTestAppContext;
use kagi::ui::{FooterStatus, ToastKind};
use kagi_ui_core::{i18n, theme};

pub fn scenario_fetch_auth_failure_toast(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let old_theme = theme::theme().slug.to_string();
    let server = http_basic_401::HttpBasic401::start();
    for preset in ["apple-light", "apple-dark"] {
        theme::set_active(preset);
        for language in [i18n::Lang::En, i18n::Lang::Ja] {
            i18n::set_lang(language);
            let fixture = build_fixture();
            let repo = fixture.path();
            git(repo, &["remote", "add", "origin", &server.url()]);
            git(repo, &["config", "credential.helper", ""]);
            git(repo, &["config", "branch.main.remote", "origin"]);
            git(repo, &["config", "branch.main.merge", "refs/heads/main"]);
            let before = git_fixture::git_output(repo, &["show-ref"]);
            let (app, window) = mount(cx, repo);
            crate::pr_detail_adaptive::resize(cx, window, (1000., 720.));
            app.update(cx, |app, cx| assert!(app.fetch_async_for(false, None, cx)));
            cx.run_until_parked();
            draw(cx, window, (1000., 720.));
            let expected = match language {
                i18n::Lang::En => {
                    "Fetch from origin requires authentication. Check your credentials."
                }
                i18n::Lang::Ja => {
                    "origin からの Fetch には認証が必要です。認証情報を確認してください。"
                }
            };
            cx.read(|cx| {
                let app = app.read(cx);
                assert!(app.fetch_in_flight.is_none());
                assert!(!app.app_sessions.has_leases());
                let toast = app
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .last()
                    .expect("failure toast");
                assert_eq!(toast.kind, ToastKind::Error);
                assert_eq!(
                    toast.message.as_ref(),
                    expected,
                    "only actionable summary is shown"
                );
                let status = kagi::ui::toast_stack::status_for_e2e().expect("toast rendered");
                assert_eq!(status.role, Some(gpui::Role::Status));
                assert!(
                    status
                        .label
                        .as_ref()
                        .is_some_and(|label| label.contains(expected)),
                    "renderer summary: {status:?}"
                );
                assert!(matches!(&app.status_footer, FooterStatus::Failed(text) if text.as_ref() == expected));
                let panel = app.op_log.as_ref().unwrap().read(cx);
                let receipt = panel
                    .entries()
                    .iter()
                    .find(|entry| entry.op == "fetch" && entry.repo == repo.display().to_string())
                    .unwrap();
                let kagi_git::oplog::OpOutcome::Failed { error } = &receipt.outcome else {
                    panic!("authentication must be Failed")
                };
                assert_eq!(error, expected, "single-line receipt summary");
                assert!(receipt.technical_detail.as_ref().is_some_and(|detail| detail.contains("terminal prompts disabled")));
                let copy = kagi::ui::oplog_panel::entry_clipboard_text(receipt);
                assert!(
                    copy.contains("terminal prompts disabled"),
                    "Copy keeps full cause: {copy}"
                );
                assert!(!copy.contains("cannot exec"));
                assert!(receipt.ref_moves.as_ref().is_some_and(Vec::is_empty));
                assert!(app.app_notice().is_none());
            });
            let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
            let row = cx.read(|cx| {
                panel
                    .read(cx)
                    .entries()
                    .iter()
                    .position(|entry| {
                        entry.op == "fetch" && entry.repo == repo.display().to_string()
                    })
                    .unwrap()
            });
            cx.update(|cx| panel.update(cx, |panel, cx| panel.copy_entry(row, cx)));
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap();
            assert!(
                copied.contains(expected) && copied.contains("terminal prompts disabled"),
                "{copied}"
            );
            drop(panel);
            let toast_count = cx.read(|cx| {
                app.read(cx)
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .len()
            });
            app.update(cx, |app, cx| assert!(app.fetch_async_for(true, None, cx)));
            cx.run_until_parked();
            cx.read(|cx| {
                let app = app.read(cx);
                assert_eq!(app.toast_stack.as_ref().unwrap().read(cx).toasts().len(), toast_count, "quiet failure adds no toast");
                assert!(matches!(&app.status_footer, FooterStatus::Failed(text) if text.as_ref() == expected));
            });
            let persisted = kagi_git::oplog::read_oplog_tail_for_repo(repo, 10);
            let attempts: Vec<_> = persisted
                .iter()
                .filter(|entry| entry.op == "fetch")
                .collect();
            assert_eq!(
                attempts.len(),
                2,
                "one durable Failed receipt per manual/quiet attempt"
            );
            for receipt in attempts {
                assert!(
                    matches!(&receipt.outcome, kagi_git::oplog::OpOutcome::Failed { error } if error == expected)
                );
                assert!(receipt
                    .technical_detail
                    .as_ref()
                    .is_some_and(|detail| detail.contains("terminal prompts disabled")
                        && !detail.contains("cannot exec")));
            }
            assert_eq!(
                git_fixture::git_output(repo, &["show-ref"]),
                before,
                "no ref update on authentication failure"
            );
            unmount(cx, app, window);
        }
    }
    theme::set_active(&old_theme);
    eprintln!("[gui-e2e] PASS fetch_auth_failure_toast");
}
