//! #1719: hidden progress preserves the panel and still delivers real failures.
use gpui::{Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, BottomTab, KagiApp, ToastKind};
use kagi_git::oplog::{self, OpOutcome};

use crate::busy_dismiss::{advance, assert_hidden, close_by_pointer, paint, ready, wait};
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use crate::pull_freshness::SettingsGuard;

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git;

pub fn scenario_busy_hidden_failure_reaches_details(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        for initially_open in [false, true] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let missing = repo.join("missing.git");
            git(
                &repo,
                &["remote", "add", "origin", missing.to_str().unwrap()],
            );
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                app.set_window_active_for_e2e(true, cx);
            });
            ready(cx, &app);
            paint(cx, &app, window);
            if cx.read(|cx| app.read(cx).bottom_panel_open) != initially_open {
                cx.simulate_keystrokes(window, "cmd-j");
                cx.run_until_parked();
            }
            let before_panel = cx.read(|cx| {
                let state = app.read(cx);
                (state.bottom_panel_open, state.bottom_tab)
            });
            assert_eq!(before_panel.0, initially_open);
            let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
            let (read_hold, read_release) = deferred::<()>(cx);
            KagiApp::hold_next_snapshot_for_e2e(read_hold);
            app.update(cx, |app, cx| app.reload_external(cx));
            cx.run_until_parked();
            advance(cx, 10);
            let (hold, release) = deferred::<()>(cx);
            KagiApp::hold_next_fetch_for_e2e(hold);
            app.update(cx, |app, cx| app.fetch_async(false, cx));
            cx.run_until_parked();
            let lease = cx.read(|cx| app.read(cx).app_sessions.running_lease().unwrap().0);
            close_by_pointer(cx, &app, window);
            advance(cx, 20);
            assert_hidden(cx, &app, window);
            cx.read(|cx| {
                let state = app.read(cx);
                assert_eq!((state.bottom_panel_open, state.bottom_tab), before_panel);
                assert_eq!(state.app_sessions.running_lease().unwrap().0, lease);
                assert!(e2e::op_latched(state));
                assert_eq!(state.slow_reads_inflight_for_e2e(owner), 1);
            });
            release.send(());
            wait(cx, &app, "the hidden failing Fetch completes", |state| {
                state.fetch_in_flight.is_none() && !state.app_sessions.has_leases()
            });
            let entries = oplog::read_oplog_tail_for_repo(&repo, 100);
            let receipt = entries.iter().find(|entry| entry.op == "fetch").unwrap();
            assert!(
                matches!(&receipt.outcome, OpOutcome::Failed { error } if error.contains(missing.to_str().unwrap()))
            );
            cx.read(|cx| {
                let state = app.read(cx);
                assert_eq!((state.bottom_panel_open, state.bottom_tab), before_panel);
                let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                assert_eq!(toasts.len(), 1);
                assert_eq!(toasts[0].kind, ToastKind::Error);
                assert!(!toasts[0].message.is_empty());
                assert!(toasts[0].action.is_some());
                assert!(!toasts[0].message.contains(missing.to_str().unwrap()));
            });
            paint(cx, &app, window);
            let details =
                e2e::control_bounds(window.window_id(), "operation-result-details").unwrap();
            cx.simulate_click(window, details.center(), Modifiers::none());
            paint(cx, &app, window);
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.bottom_panel_open);
                assert_eq!(state.bottom_tab, BottomTab::OperationLog);
                let panel = state.op_log.as_ref().unwrap().read(cx);
                let row = &panel.entries()[panel.expanded().unwrap()];
                assert_eq!(
                    (row.id, row.timestamp, &row.op, &row.repo),
                    (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
                );
                assert!(state
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .is_empty());
            });
            read_release.send(());
            ready(cx, &app);
            unmount(cx, app, window);
        }
    }
}
