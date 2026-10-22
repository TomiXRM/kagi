//! Frozen remote Pull visits own one result, including departed and inactive UI.
use crate::busy_dismiss::{paint, ready, wait};
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{e2e, i18n::Lang, FooterStatus, ToastAction, ToastKind};
use kagi_git::oplog::{self, OpOutcome};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Current,
    Inactive,
    Away,
    Revisit,
}

pub fn scenario_remote_pull_current_result_owner(cx: &mut VisualTestAppContext) {
    scenarios(cx, Surface::Current);
    eprintln!("[gui-e2e] PASS remote_pull_current_result_owner");
}
pub fn scenario_remote_pull_inactive_result_owner(cx: &mut VisualTestAppContext) {
    scenarios(cx, Surface::Inactive);
    eprintln!("[gui-e2e] PASS remote_pull_inactive_result_owner");
}
pub fn scenario_remote_pull_away_result_owner(cx: &mut VisualTestAppContext) {
    scenarios(cx, Surface::Away);
    eprintln!("[gui-e2e] PASS remote_pull_away_result_owner");
}
pub fn scenario_remote_pull_revisit_result_owner(cx: &mut VisualTestAppContext) {
    scenarios(cx, Surface::Revisit);
    eprintln!("[gui-e2e] PASS remote_pull_revisit_result_owner");
}

fn scenarios(cx: &mut VisualTestAppContext, surface: Surface) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "recent_repos"]);
    for lang in [Lang::En, Lang::Ja] {
        for unknown in [false, true] {
            for recording_failure in [false, true] {
                result_owner(cx, surface, lang, unknown, recording_failure);
            }
        }
    }
}

fn result_owner(
    cx: &mut VisualTestAppContext,
    surface: Surface,
    lang: Lang,
    unknown: bool,
    recording_failure: bool,
) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::fetch_owner::local_remote_with_change(&repo, remote.path());
    git(&repo, &["fetch", "-q", "origin"]);
    let head = git_fixture::git_output(&repo, &["rev-parse", "HEAD"]);
    let snapshot = kagi_git::Backend::open(&repo)
        .unwrap()
        .snapshot(10_000)
        .unwrap();
    let shim = tempfile::tempdir().unwrap();
    let marker = |name: &str| shim.path().join(name);
    let release = marker("release");
    let calls = marker("calls");
    let fail = marker("fail");
    let probe_release = marker("probe-release");
    // Reuse the existing SSH options / frozen identity / checked-execution
    // protocol fixture. Only transport is substituted, not a completed report.
    crate::recovery_pull::blocking_fake_ssh(
        shim.path(),
        &head,
        &release,
        &calls,
        &fail,
        &probe_release,
        &marker("identity-change"),
        &marker("route-change"),
        &marker("toplevel-change"),
        &marker("branch-change"),
        &marker("oid-change"),
        &marker("upstream-change"),
        &marker("head-read-failure"),
        &marker("remote-url-change"),
        &marker("dirty-change"),
    );
    std::fs::write(&probe_release, "go").unwrap();
    let old_path = std::env::var_os("PATH");
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            shim.path().display(),
            old_path.clone().unwrap_or_default().to_string_lossy()
        ),
    );
    let host = kagi_domain::remote::RemoteHost {
        user: Some("kagi-e2e".into()),
        host: "e2e.invalid".into(),
        port: None,
        identity_file: None,
    };
    let recorded_repo = format!("{}:/srv/repo", host.label());
    let count = || {
        oplog::read_oplog_tail(usize::MAX)
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == recorded_repo)
            .count()
    };
    let before_count = count();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_window_active_for_e2e(surface != Surface::Inactive, cx);
        app.enter_remote_view(host, "/srv/repo".into(), snapshot, cx);
        app.open_pull_modal(cx);
    });
    wait(
        cx,
        &app,
        "remote Pull's real identity plan is ready",
        |app| {
            app.pull_modal().is_some()
                && matches!(
                    app.app_sessions.plan_state(),
                    kagi::app::PlanState::Ready { .. }
                )
        },
    );
    let (owner, remote_index) = cx.read(|cx| {
        let state = app.read(cx);
        (
            state
                .app_sessions
                .attachment(state.active_session().unwrap())
                .unwrap(),
            state.active_tab,
        )
    });
    let before_rows = cx.read(|cx| {
        app.read(cx)
            .op_log
            .as_ref()
            .unwrap()
            .read(cx)
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == recorded_repo)
            .count()
    });
    let (hold, run_release) = deferred(cx);
    kagi::ui::KagiApp::hold_next_run_for_e2e(hold);
    crate::recovery_operations::press_enter(cx, &app, window);
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.pull_modal().is_none());
        assert!(app.app_sessions.has_leases());
        if matches!(surface, Surface::Away | Surface::Revisit) {
            app.switch_repo(0, cx);
            if surface == Surface::Revisit {
                app.switch_repo(remote_index, cx);
                assert_eq!(app.active_session(), Some(owner.session));
                assert_ne!(
                    app.app_sessions.attachment(owner.session).unwrap().visit,
                    owner.visit
                );
            }
        }
    });
    ready(cx, &app);
    let foreign = app.update(cx, |app, _| {
        if matches!(surface, Surface::Away | Surface::Revisit) {
            app.status_footer = FooterStatus::Idle("new visit result owner".into());
        }
        (
            app.active_session(),
            format!("{:?}", app.status_footer),
            app.bottom_panel_open,
            app.bottom_tab,
        )
    });
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl.lock");
    let recording_lock = recording_failure.then(|| {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .unwrap();
        file.lock().unwrap();
        file
    });
    // Keep the automatic read in flight while checking result delivery. This
    // is the existing read transport seam, not a synthetic result or app state.
    let finish_refresh = if matches!(surface, Surface::Current | Surface::Inactive) {
        Some(crate::remote_refresh_owner::defer_refresh(cx))
    } else {
        None
    };
    if unknown {
        std::fs::write(&fail, "unrecognized remote reply").unwrap();
    }
    std::fs::write(&release, "go").unwrap();
    run_release.send(());
    wait(
        cx,
        &app,
        "checked remote Pull settles its real lease",
        |app| {
            if unknown {
                !app.app_sessions.reconcile_ids().is_empty()
            } else {
                !app.app_sessions.has_leases()
            }
        },
    );
    paint(cx, &app, window);
    assert_eq!(
        std::fs::read_to_string(&calls)
            .unwrap()
            .lines()
            .filter(|line| *line == "KAGI-EXEC-GIT-PULL")
            .count(),
        1
    );
    assert_eq!(count(), before_count + usize::from(!recording_failure));
    let selected = cx.read(|cx| {
        let state = app.read(cx);
        let panel = state.op_log.as_ref().unwrap().read(cx);
        let rows: Vec<_> = panel.entries().iter()
            .filter(|entry| entry.op == "pull" && entry.repo == recorded_repo).collect();
        let stack = state.toast_stack.as_ref().unwrap().read(cx);
        eprintln!("[gui-e2e] remote_pull_result surface={surface:?} lang={lang:?} unknown={unknown} recording_failure={recording_failure} frozen={owner:?} live={:?} footer={:?} panel_rows={}->{} toasts={:?}", state.app_sessions.attachment(owner.session), state.status_footer, before_rows, rows.len(), stack.toasts().iter().map(|toast| (toast.id, toast.kind, &toast.action)).collect::<Vec<_>>());
        assert_eq!(rows.len(), before_rows + 1, "one attempted receipt is presented even away");
        let entry = rows[0];
        if recording_failure {
            assert!(matches!(&entry.outcome, OpOutcome::Partial { error, .. }
                if !unknown && error.contains(&std::fs::TryLockError::WouldBlock.to_string()))
                || matches!(&entry.outcome, OpOutcome::Unknown { evidence, .. }
                if unknown && evidence.contains("unexpected remote reply")
                    && evidence.contains(&std::fs::TryLockError::WouldBlock.to_string())));
            assert!(state.app_notice().is_some_and(|notice| notice.message.contains(&std::fs::TryLockError::WouldBlock.to_string())));
        } else {
            assert_eq!(matches!(entry.outcome, OpOutcome::Unknown { .. }), unknown);
            assert_eq!(matches!(entry.outcome, OpOutcome::Success { .. }), !unknown);
        }
        let announce = surface != Surface::Current && !recording_failure;
        assert_eq!(stack.toasts().len(), usize::from(announce));
        if announce {
            let toast = &stack.toasts()[0];
            assert_eq!(toast.kind, if unknown { ToastKind::Error } else { ToastKind::Success });
            if unknown {
                assert!(matches!(toast.action, Some(ToastAction::OpenOperationLog(_))));
            }
        }
        if matches!(surface, Surface::Away | Surface::Revisit) {
            assert_eq!((state.active_session(), format!("{:?}", state.status_footer), state.bottom_panel_open, state.bottom_tab), foreign);
        } else if surface == Surface::Inactive {
            // The read may own Busy; an ordinary result must not add Success.
            assert!(!matches!(state.status_footer, FooterStatus::Success(_)));
        }
        if unknown {
            assert!(!state.app_sessions.may_close_host());
            assert!(state.app_notice().is_some());
        }
        (entry.id, entry.timestamp, entry.op.clone(), entry.repo.clone())
    });
    drop(recording_lock);
    if unknown && !recording_failure && surface != Surface::Current {
        // The required inspection is not bypassed to click a covered toast.
        // Use the existing three explicit Inspect / arm / acknowledge controls
        // before testing the result's actual Details navigation.
        for _ in 0..3 {
            wait(
                cx,
                &app,
                "required remote inspection is actionable",
                |app| app.app_notice().is_some(),
            );
            e2e::clear_control_bounds(window.window_id(), "app-notice-confirm");
            paint(cx, &app, window);
            let inspect = e2e::control_bounds(window.window_id(), "app-notice-confirm").unwrap();
            cx.simulate_click(window, inspect.center(), gpui::Modifiers::none());
            cx.run_until_parked();
        }
        assert!(cx.read(|cx| app.read(cx).app_sessions.may_close_host()));
        e2e::clear_control_bounds(window.window_id(), "operation-result-details");
        paint(cx, &app, window);
        let details = e2e::control_bounds(window.window_id(), "operation-result-details")
            .expect("the remote Unknown still offers its saved receipt");
        cx.simulate_click(window, details.center(), gpui::Modifiers::none());
        paint(cx, &app, window);
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(state.bottom_panel_open);
            assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
            let panel = state.op_log.as_ref().unwrap().read(cx);
            let entry = &panel.entries()[panel.expanded().expect("the actual receipt is expanded")];
            assert_eq!(
                (
                    entry.id,
                    entry.timestamp,
                    entry.op.clone(),
                    entry.repo.clone()
                ),
                selected
            );
        });
    }
    if let Some(finish_refresh) = finish_refresh {
        finish_refresh(Ok(kagi_git::Backend::open(&repo)
            .unwrap()
            .snapshot(10_000)
            .unwrap()));
        cx.run_until_parked();
    }
    assert_eq!(
        count(),
        before_count + usize::from(!recording_failure),
        "presentation never retries the append"
    );
    unmount(cx, app, window);
    match old_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
}
