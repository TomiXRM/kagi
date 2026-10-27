//! #1561: real HTTP Basic 401 fetch, actionable summary and complete Copy.
#[path = "../support/git_fixture.rs"]
mod git_fixture;
#[path = "../support/http_basic_401.rs"]
mod http_basic_401;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::{draw, GlobalSettings};
use gpui::VisualTestAppContext;
use kagi::ui::{e2e, FooterStatus, ToastKind};
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
            git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
            let before = git_fixture::git_output(repo, &["show-ref"]);
            let (app, window) = mount(cx, repo);
            crate::pr_detail_adaptive::resize(cx, window, (1000., 720.));
            app.update(cx, |app, cx| {
                app.set_window_active_for_e2e(false, cx);
                app.bottom_panel_open = false;
                assert!(app.fetch_async_for(false, None, cx));
            });
            cx.run_until_parked();
            draw(cx, window, (1000., 720.));
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
                assert_auth_preview(&toast.message, language, repo, &server.url());
                assert!(toast.result_key.is_some(), "the away result is keyed");
                assert!(
                    toast.action.is_some(),
                    "authentication failure offers details"
                );
                let status = kagi::ui::toast_stack::status_for_e2e(
                    window.window_id(),
                    kagi::ui::toast_stack::ToastAudience::Repository,
                )
                .expect("toast rendered");
                assert_eq!(status.role, Some(gpui::Role::Status));
                assert!(
                    status
                        .label
                        .as_ref()
                        .is_some_and(|label| label.contains(toast.message.as_ref())),
                    "the Status announcement includes the displayed safe preview: {status:?}"
                );
                assert!(matches!(&app.status_footer, FooterStatus::Idle(_)));
                assert!(!app.bottom_panel_open, "failure does not auto-open the log");
                let panel = app.op_log.as_ref().unwrap().read(cx);
                let receipt = panel
                    .entries()
                    .iter()
                    .find(|entry| entry.op == "fetch" && entry.repo == repo.display().to_string())
                    .unwrap();
                assert_auth_receipt(receipt);
                assert!(receipt.ref_moves.as_ref().is_some_and(Vec::is_empty));
                assert!(app.app_notice().is_none());
            });
            let action = e2e::control_bounds(window.window_id(), "operation-result-details")
                .expect("authentication result draws its detail action");
            cx.simulate_click(window, action.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            draw(cx, window, (1000., 720.));
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
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.bottom_panel_open);
                assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                assert_eq!(panel.read(cx).expanded(), Some(row));
                assert!(panel
                    .read(cx)
                    .scroll_handle()
                    .bounds_for_item(row)
                    .is_some());
            });
            cx.update(|cx| panel.update(cx, |panel, cx| panel.copy_entry(row, cx)));
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap();
            assert!(
                copied.contains("origin") && copied.contains("terminal prompts disabled"),
                "{copied}"
            );
            drop(panel);
            let toast_ids = cx.read(|cx| {
                app.read(cx)
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .iter()
                    .map(|toast| toast.id)
                    .collect::<Vec<_>>()
            });
            app.update(cx, |app, cx| assert!(app.fetch_async_for(true, None, cx)));
            cx.run_until_parked();
            cx.read(|cx| {
                let app = app.read(cx);
                assert!(
                    app.toast_stack
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .toasts()
                        .iter()
                        .all(|toast| toast_ids.contains(&toast.id)),
                    "quiet failure adds no notification"
                );
                assert!(matches!(&app.status_footer, FooterStatus::Idle(_)));
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
                assert_auth_receipt(receipt);
                assert!(receipt.ref_moves.as_ref().is_some_and(Vec::is_empty));
            }
            // Exercise the real remote-branch menu item, not the repo Fetch command.
            let target = kagi_git::CommitId(
                git_fixture::git_output(repo, &["rev-parse", "HEAD"])
                    .trim()
                    .to_string(),
            );
            app.update(cx, |app, cx| {
                app.open_remote_branch_menu(
                    "origin/main".into(),
                    target,
                    gpui::point(gpui::px(150.), gpui::px(150.)),
                );
                cx.notify();
            });
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "branch-menu-item-1-0");
            draw(cx, window, (1000., 720.));
            let fetch = kagi::ui::e2e::control_bounds(window.window_id(), "branch-menu-item-1-0")
                .expect("Fetch remote branch menu item is drawn");
            cx.simulate_click(window, fetch.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            draw(cx, window, (1000., 720.));
            cx.read(|cx| {
                let app = app.read(cx);
                assert!(app.branch_menu.is_none(), "real menu action dismissed");
                assert!(!app.app_sessions.has_leases());
                let toast = app
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .last()
                    .unwrap();
                assert_eq!(toast.kind, ToastKind::Error);
                assert_auth_preview(&toast.message, language, repo, &server.url());
                let FooterStatus::Failed(preview) = &app.status_footer else {
                    panic!("branch-menu failure keeps its near explanation");
                };
                assert_auth_preview(preview, language, repo, &server.url());
                assert!(app.app_notice().is_none());
            });
            let persisted = kagi_git::oplog::read_oplog_tail_for_repo(repo, 10);
            let branch_attempts: Vec<_> = persisted
                .iter()
                .filter(|entry| entry.op == "fetch-remote-branch")
                .collect();
            assert_eq!(branch_attempts.len(), 1, "one branch-menu receipt");
            let receipt = branch_attempts[0];
            assert_auth_receipt(receipt);
            let panel = cx.read(|cx| app.read(cx).op_log.clone().unwrap());
            let row = cx.read(|cx| {
                panel
                    .read(cx)
                    .entries()
                    .iter()
                    .position(|entry| entry.op == "fetch-remote-branch")
                    .unwrap()
            });
            cx.update(|cx| panel.update(cx, |panel, cx| panel.copy_entry(row, cx)));
            let copied = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap();
            assert!(
                copied.contains("origin") && copied.contains("terminal prompts disabled"),
                "{copied}"
            );
            drop(panel);
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

fn assert_auth_preview(
    preview: &str,
    language: i18n::Lang,
    repo: &std::path::Path,
    remote_url: &str,
) {
    assert!(
        !preview.is_empty() && preview.chars().count() <= 240,
        "{preview}"
    );
    let actionable = match language {
        i18n::Lang::En => {
            (preview.contains("Authentication") || preview.contains("authentication"))
                && (preview.contains("Check") || preview.contains("check"))
        }
        i18n::Lang::Ja => preview.contains("認証") && preview.contains("確認"),
    };
    assert!(
        actionable,
        "the localized preview explains authentication and a next step: {preview}"
    );
    for private in [
        repo.to_str().unwrap(),
        remote_url,
        "127.0.0.1",
        "terminal prompts disabled",
        "fatal:",
        "exit 128",
        "cannot exec",
    ] {
        assert!(!preview.contains(private), "{preview}");
    }
}

fn assert_auth_receipt(receipt: &kagi_git::oplog::OpLogEntry) {
    let kagi_git::oplog::OpOutcome::Failed { error } = &receipt.outcome else {
        panic!(
            "authentication must be durably Failed: {:?}",
            receipt.outcome
        );
    };
    assert!(error.contains("origin") && !error.contains('\n'), "{error}");
    let detail = receipt
        .technical_detail
        .as_ref()
        .expect("durable authentication cause");
    assert!(detail.contains("terminal prompts disabled"), "{detail}");
    assert!(!detail.contains("cannot exec"), "{detail}");
    let copy = kagi::ui::oplog_panel::entry_clipboard_text(receipt);
    assert!(
        copy.contains(detail),
        "Copy retains the complete raw cause: {copy}"
    );
}
