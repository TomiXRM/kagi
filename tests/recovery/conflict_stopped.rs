//! #1699: a real merge/rebase conflict must never announce completion.
use crate::macos::{git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{FooterStatus, KagiApp, ToastKind};
use kagi_ui_core::i18n::{self, Lang};
use std::time::{Duration, Instant};

#[path = "../support/oplog.rs"]
mod oplog;

#[path = "../support/git_fixture.rs"]
mod git_fixture;

fn fixture(count: usize) -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.name", "Kagi Test"]);
    git(repo, &["config", "user.email", "kagi@example.test"]);
    for (branch, content) in [
        ("main", "base"),
        ("feature", "incoming"),
        ("main", "current"),
    ] {
        match content {
            "incoming" => {
                git(repo, &["checkout", "-qb", branch]);
            }
            "current" => {
                git(repo, &["checkout", "-q", branch]);
            }
            _ => {}
        }
        for index in 0..count {
            std::fs::write(
                repo.join(format!("conflict-{index}.txt")),
                format!("{content}\n"),
            )
            .unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", content]);
    }
    fixture
}

fn wait_for(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    ready: impl Fn(&KagiApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| ready(app.read(cx))) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "conflict operation did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn start_conflict(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, rebase: bool) {
    app.update(cx, |app, cx| {
        if rebase {
            app.open_rebase_modal("feature".into(), cx);
        } else {
            app.open_merge_modal("feature".into(), None, cx);
        }
    });
    wait_for(cx, app, |app| app.planning.is_none());
    app.update(cx, |app, cx| {
        if rebase {
            assert!(app
                .rebase_current_onto_modal()
                .unwrap()
                .plan
                .blockers
                .is_empty());
            app.start_rebase(cx);
            if app.rebase_current_onto_modal().is_some() {
                assert!(app.rebase_current_onto_modal().unwrap().confirm_armed);
                app.start_rebase(cx);
            }
        } else {
            assert!(app.merge_modal().unwrap().plan.blockers.is_empty());
            app.start_merge(cx);
        }
    });
}

fn assert_stopped(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    observed: usize,
    recording_failure: bool,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        let conflict = app.ui().conflict.as_ref().unwrap().read(cx);
        let mode = conflict.mode.as_ref().unwrap();
        assert_eq!(mode.conflicted_count(), observed);
        assert!(!mode.can_continue(), "unresolved files disable Continue");
        let stack = app.toast_stack.as_ref().unwrap().read(cx);
        if recording_failure {
            assert!(
                stack.toasts().is_empty(),
                "required recording acknowledgment is the only result presentation"
            );
            assert!(matches!(&app.status_footer,
                FooterStatus::Idle(text) if text.is_empty()));
        } else {
            assert!(
                matches!(&app.status_footer, FooterStatus::Idle(_)),
                "unresolved conflicts must not leave a successful footer"
            );
            let notices: Vec<_> = stack
                .toasts()
                .iter()
                .filter(|toast| toast.kind == ToastKind::Info)
                .collect();
            assert_eq!(notices.len(), 1);
            assert!(
                notices[0].message.contains(&observed.to_string()),
                "the stopped result must report the actual unresolved-file count"
            );
            assert!(
                stack
                    .toasts()
                    .iter()
                    .all(|toast| toast.kind != ToastKind::Success),
                "unresolved conflicts and disabled Continue cannot coexist with green completion"
            );
        }
    });
}

fn assert_recording_failure(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &std::path::Path,
    operation: &str,
) {
    let append_diagnostic = std::fs::TryLockError::WouldBlock.to_string();
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app
            .app_notice()
            .expect("failed append must be immediately visible");
        assert!(notice.message.contains(&repo.display().to_string()));
        assert!(notice.message.contains(&append_diagnostic));
        assert!(app.toast_stack.as_ref().unwrap().read(cx).toasts().is_empty(),
            "a failed append must not add a stopped or error toast beside its acknowledgment");
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let entries: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == operation && std::path::Path::new(&entry.repo) == repo)
            .collect();
        assert_eq!(entries.len(), 1, "no duplicate in-memory receipts");
        assert!(matches!(&entries[0].outcome,
            kagi_git::oplog::OpOutcome::Partial { error, .. } if error.contains(&append_diagnostic)));
    });
}

fn stopped_matrix(cx: &mut VisualTestAppContext, rebase: bool, recording_failure: bool) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        for count in [1, 3] {
            let fixture = fixture(count);
            let repo = fixture.path().canonicalize().unwrap();
            i18n::set_lang(lang);
            let (app, window) = mount(cx, &repo);
            let recording_lock = recording_failure.then(|| {
                let path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                    .join("operations.jsonl.lock");
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                    .unwrap();
                lock.lock().unwrap();
                lock
            });
            start_conflict(cx, &app, rebase);
            wait_for(cx, &app, |app| {
                app.write_busy_op.is_none() && app.ui().conflict.is_some()
            });
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let observed = kagi_git::Backend::open(&repo)
                .unwrap()
                .working_tree_status()
                .unwrap()
                .conflicted
                .len();
            assert_eq!(
                observed, count,
                "fixture leaves real unmerged index entries"
            );
            assert_stopped(cx, &app, observed, recording_failure);
            let operation = if rebase { "rebase" } else { "merge" };
            if recording_failure {
                assert_recording_failure(cx, &app, &repo, operation);
            }
            assert_eq!(
                oplog::records(&repo, operation, 100, true).len(),
                usize::from(!recording_failure),
                "presentation must neither duplicate nor fabricate durable receipts"
            );
            assert!(
                !cx.read(|cx| app.read(cx).app_sessions.has_leases()),
                "the existing known-outcome settlement still releases its lease"
            );
            drop(recording_lock);
            if recording_failure {
                app.update(cx, |app, cx| app.confirm_app_notice(cx));
                cx.run_until_parked();
                assert_stopped(cx, &app, observed, true);
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(state.app_notice().is_none());
                    assert!(!kagi::ui::e2e::queued_notice_contains(
                        state,
                        &repo.display().to_string()
                    ));
                });
            }
            cx.run_until_parked();
            assert_eq!(
                oplog::records(&repo, operation, 100, true).len(),
                usize::from(!recording_failure),
                "presentation must not retry persistence"
            );
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
}

pub fn scenario_conflict_merge_stopped_toast(cx: &mut VisualTestAppContext) {
    stopped_matrix(cx, false, false);
}

pub fn scenario_conflict_rebase_stopped_toast(cx: &mut VisualTestAppContext) {
    stopped_matrix(cx, true, false);
}

pub fn scenario_oplog_append_failure_conflict_stopped(cx: &mut VisualTestAppContext) {
    stopped_matrix(cx, false, true);
    stopped_matrix(cx, true, true);
}

/// Actual two-commit rebase -> edited first resolution -> reviewed Continue ->
/// a new physical conflict. This is the public handler boundary, not an OS
/// editor-input claim or an assertion about retained editor ownership (#1721).
pub fn scenario_conflict_continue_stopped_result_owner(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        for recording_failure in [false, true] {
            for departed in [false, true] {
                let fixture = fixture(1);
                let repo = fixture.path().canonicalize().unwrap();
                let other_fixture = departed.then(crate::macos::build_fixture);
                let other_repo = other_fixture
                    .as_ref()
                    .map(|fixture| fixture.path().canonicalize().unwrap());
                git(&repo, &["config", "core.editor", "true"]);
                git(&repo, &["checkout", "-q", "feature"]);
                std::fs::write(repo.join("conflict-0.txt"), "incoming second\n").unwrap();
                git(&repo, &["commit", "-qam", "second incoming"]);
                let second = git_fixture::git_output(&repo, &["rev-parse", "HEAD"]);
                assert!(!git_fixture::git_succeeds(&repo, &["rebase", "main"]));
                let before = git_fixture::git_output(&repo, &["rev-parse", "HEAD"]);
                i18n::set_lang(lang);
                let (app, window) = mount(cx, &repo);
                app.update(cx, |app, cx| {
                    // Supported activation inputs, not OS focus: same-screen
                    // results are unattended; the foreign repository is active.
                    app.set_window_active_for_e2e(departed, cx);
                    app.detect_conflict_mode(cx);
                });
                wait_for(cx, &app, |app| app.ui().conflict.is_some());
                app.update(cx, |app, cx| {
                    app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                        view.mode
                            .as_mut()
                            .unwrap()
                            .buffer
                            .set_manual_text(
                                std::path::Path::new("conflict-0.txt"),
                                "resolved in Kagi\n",
                            )
                            .unwrap();
                    });
                });
                cx.update_window(window, |_, window, cx| {
                    app.update(cx, |app, cx| {
                        let owner = app
                            .app_sessions
                            .attachment(app.active_session().unwrap())
                            .unwrap();
                        app.conflict_continue(owner, window, cx);
                    });
                })
                .unwrap();
                wait_for(cx, &app, |app| app.conflict_continue_modal().is_some());
                assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_some()));
                let recording_lock = recording_failure.then(|| {
                    let path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                        .join("operations.jsonl.lock");
                    let lock = std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(false)
                        .write(true)
                        .open(path)
                        .unwrap();
                    lock.lock().unwrap();
                    lock
                });
                let release = departed.then(|| {
                    let (hold, release) = crate::evidence_support::deferred::<()>(cx);
                    KagiApp::hold_next_continue_confirm_for_e2e(hold);
                    release
                });
                app.update(cx, |app, cx| app.confirm_conflict_continue(cx));
                if departed {
                    // Confirm flushes before admitting the held backend write.
                    wait_for(cx, &app, |app| app.write_busy_op.is_some());
                }
                let foreign_before = if let Some(other_repo) = other_repo.as_ref() {
                    app.update(cx, |app, cx| {
                        // The existing Cancel dismisses the running confirmation,
                        // not its admitted backend write.
                        app.cancel_conflict_continue();
                        assert!(app.open_repository(other_repo.clone(), cx));
                    });
                    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
                    wait_for(cx, &app, |app| app.slow_reads_inflight_for_e2e(owner) == 0);
                    // Obtain a meaningful footer from a real read consumer,
                    // rather than assigning a synthetic foreign sentinel.
                    app.update(cx, |app, cx| app.reload_external(cx));
                    wait_for(cx, &app, |app| app.slow_reads_inflight_for_e2e(owner) == 0);
                    Some((
                        crate::macos::repo_fingerprint(other_repo),
                        cx.read(|cx| {
                            let state = app.read(cx);
                            (
                                format!("{:?}", state.status_footer),
                                (state.bottom_panel_open, state.bottom_tab),
                                state.active_session(),
                            )
                        }),
                    ))
                } else {
                    None
                };
                if let Some(release) = release {
                    release.send(());
                }
                wait_for(cx, &app, |app| {
                    app.write_busy_op.is_none() && app.conflict_continue_modal().is_none()
                });
                cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
                let physical = || {
                    (
                        git_fixture::git_output(&repo, &["rev-parse", "HEAD"]),
                        git_fixture::git_output(&repo, &["rev-parse", "REBASE_HEAD"]),
                        git_fixture::git_output(&repo, &["ls-files", "--unmerged"]),
                        git_fixture::git_output(&repo, &["show", "HEAD:conflict-0.txt"]),
                    )
                };
                let after = physical();
                assert_ne!(after.0, before, "only the first replay was committed");
                assert_eq!(after.1, second, "the second replay is the new stop");
                assert_eq!(after.2.lines().count(), 3, "a real new unmerged file");
                assert_eq!(
                    after.3, "resolved in Kagi",
                    "the first edited resolution was committed"
                );
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(!state.app_sessions.has_leases());
                    if !departed {
                        assert!(!matches!(state.status_footer, FooterStatus::Success(_)));
                    }
                    if let Some((_, before)) = foreign_before.as_ref() {
                        assert_eq!(
                            (
                                format!("{:?}", state.status_footer),
                                (state.bottom_panel_open, state.bottom_tab),
                                state.active_session(),
                            ),
                            *before,
                            "A's result must preserve B's actual read-produced footer and panel"
                        );
                    }
                    let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                    if recording_failure {
                        assert!(
                            toasts.is_empty(),
                            "required acknowledgment is the sole result"
                        );
                        let notice = state
                            .app_notice()
                            .expect("failed append retains the stopped result in its acknowledgment");
                        let diagnostic = format!(
                            "git error: oplog retention: {}",
                            std::fs::TryLockError::WouldBlock
                        );
                        let expected = format!(
                            "{}\n\n{}",
                            i18n::op::user_text::operations::recording_failed(
                                repo.display(),
                                "rebase-continue",
                                i18n::op::RecordingOutcome::Partial,
                                &diagnostic,
                            ),
                            i18n::Msg::conflict_stopped(None),
                        );
                        assert_eq!(
                            notice.message, expected,
                            "EN/JA acknowledgment must say Continue stopped at the next conflict, not succeeded"
                        );
                    } else {
                        assert!(state.app_notice().is_none());
                        assert_eq!(toasts.len(), 1);
                        assert_eq!(
                            toasts[0].kind,
                            ToastKind::Info,
                            "a stopped request is not green completion"
                        );
                    }
                });
                let operation = "rebase-continue";
                if recording_failure {
                    assert_recording_failure(cx, &app, &repo, operation);
                }
                let durable = oplog::records(&repo, operation, 100, true);
                assert_eq!(durable.len(), usize::from(!recording_failure));
                if !recording_failure {
                    assert!(
                        matches!(
                            durable[0].outcome,
                            kagi_git::oplog::OpOutcome::Success { .. }
                        ),
                        "known step success is retained in the original receipt"
                    );
                    kagi::ui::e2e::clear_control_bounds(
                        window.window_id(),
                        "operation-result-details",
                    );
                    cx.update_window(window, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let bounds = kagi::ui::e2e::control_bounds(
                        window.window_id(),
                        "operation-result-details",
                    )
                    .expect("the stopped Info result exposes actual receipt Details");
                    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    cx.read(|cx| {
                        let state = app.read(cx);
                        assert!(state.bottom_panel_open);
                        assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                        let panel = state.op_log.as_ref().unwrap().read(cx);
                        let row = &panel.entries()[panel.expanded().unwrap()];
                        let receipt = &durable[0];
                        assert_eq!(
                            (row.id, row.timestamp, &row.op, &row.repo),
                            (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
                        );
                        if departed {
                            assert_eq!(
                                state.repo_path.as_deref(),
                                Some(repo.as_path()),
                                "the explicit Details action follows A's existing owner tab"
                            );
                        }
                    });
                }
                drop(recording_lock);
                if recording_failure {
                    let bounds =
                        kagi::ui::e2e::control_bounds(window.window_id(), "app-notice-confirm")
                            .expect("the required acknowledgment is an actual rendered control");
                    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
                    cx.run_until_parked();
                    assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
                }
                assert_eq!(
                    physical(),
                    after,
                    "acknowledgment must not replay or skip another commit"
                );
                assert_eq!(
                    oplog::records(&repo, operation, 100, true).len(),
                    durable.len(),
                    "acknowledgment must not retry the append"
                );
                if let Some((before, _)) = foreign_before {
                    assert_eq!(
                        crate::macos::repo_fingerprint(other_repo.as_ref().unwrap()),
                        before
                    );
                }
                unmount(cx, app, window);
            }
        }
    }
    i18n::set_lang(language);
}
