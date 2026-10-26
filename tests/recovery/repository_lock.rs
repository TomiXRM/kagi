//! #1746: repository admission probes must not manufacture durable receipts.
use crate::app_conflict::{content_fixture, wait_idle};
use crate::conflict_draft_revision::RebaseFixture;
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;

#[path = "../support/git_fixture.rs"]
mod git_fixture;

const BUSY: &str = "Another Kagi window is operating on this repository";

/// Hold a separate descriptor on a scoped thread. The backend's
/// conflict_write_lock_test owns the independent-process/death-release oracle;
/// its child entry point is not available in the native runner binary.
pub(crate) struct RepositoryLockOwner {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RepositoryLockOwner {
    pub(crate) fn acquire(repo: &Path) -> Self {
        let repo = repo.to_owned();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(repo.join(".git/kagi-write.lock"))
                .unwrap();
            lock.try_lock().unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv();
            drop(lock);
        });
        ready_rx.recv().unwrap();
        Self {
            release: Some(release),
            thread: Some(thread),
        }
    }
}

impl Drop for RepositoryLockOwner {
    fn drop(&mut self) {
        self.release.take();
        self.thread.take().unwrap().join().unwrap();
    }
}

fn with_repository_lock(repo: &Path, action: impl FnOnce()) {
    let _owner = RepositoryLockOwner::acquire(repo);
    action();
}

fn assert_one_refusal(repo: &Path, before: usize, operation: &str) {
    let entries = read_oplog_tail_for_repo(repo, 100);
    assert_eq!(
        entries.len(),
        before + 1,
        "one operation receipt, no admission receipt"
    );
    let receipt = entries.iter().find(|entry| entry.op == operation).unwrap();
    assert!(
        matches!(&receipt.outcome, OpOutcome::Refused { blockers }
            if blockers.len() == 1 && blockers[0].contains(BUSY)),
        "the operation must name repository contention: {:?}",
        receipt.outcome
    );
    assert!(entries
        .iter()
        .all(|entry| entry.op != "conflict-write-admission"));
}

pub fn scenario_conflict_repository_lock_continue(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_set_file_side(
            Path::new("file.txt"),
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        cx.notify();
    });
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let worktree = std::fs::read(repo.join("file.txt")).unwrap();
    let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
    with_repository_lock(&repo, || {
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
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "merge-continue");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
        assert_eq!(
            std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
            merge_head
        );
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    });
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_conflict_repository_lock_skip(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let worktree = std::fs::read(repo.join("file.txt")).unwrap();
    let head = std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, _| assert!(!view.skip_request_arm()));
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_skip(owner, cx);
        });
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "rebase-skip");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
        assert_eq!(std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap(), head);
        assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    });
    drop(conflict);
    unmount(cx, app, window);
}

pub fn scenario_repository_lock_silent_auto_fetch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::fetch_owner::local_remote_with_change(&repo, remote.path());
    let upstream = remote.path().join("other");
    let upstream_tip = git_fixture::git_output(&upstream, &["rev-parse", "HEAD"]);
    let tracking_before =
        git_fixture::git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]);
    assert_ne!(tracking_before, upstream_tip);
    let (app, window) = mount(cx, &repo);
    wait_idle(cx, &app);
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let toasts = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .map_or(0, |stack| stack.read(cx).toasts().len())
    });
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            assert!(
                !app.fetch_async_for(true, None, cx),
                "busy auto-fetch must not launch"
            );
        });
        wait_idle(cx, &app);
        assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), before);
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state
                    .toast_stack
                    .as_ref()
                    .map_or(0, |stack| stack.read(cx).toasts().len()),
                toasts
            );
            assert!(state.fetch_in_flight.is_none());
            assert!(!state.app_sessions.has_leases());
            assert!(state.app_notice().is_none());
        });
        assert_eq!(
            git_fixture::git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
            tracking_before,
            "a refused quiet fetch must not move tracking refs"
        );
    });
    app.update(cx, |app, cx| {
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(message)
            if message.as_ref() == kagi_ui_core::i18n::Msg::AdviceConflictsRepositoryBusy.t())
        );
        assert!(app.fetch_async_for(true, None, cx));
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(message)
            if message.is_empty())
        );
    });
    wait_idle(cx, &app);
    assert_eq!(
        git_fixture::git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        upstream_tip,
        "the admitted quiet fetch must update its tracking ref"
    );
    exercise_footer_recovery(cx, &app, &repo, &upstream);
    unmount(cx, app, window);
}

/// Exercise both admission adapters with real refusals, including a language
/// change between refusal and retry. Quiet fetch leaves the admission footer
/// visible, so a later result cannot hide a stale-contention regression.
fn exercise_footer_recovery(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    repo: &Path,
    upstream: &Path,
) {
    use kagi::ui::{
        i18n::{self, Lang, Msg},
        FooterStatus,
    };
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous = i18n::lang();
    for stage in [false, true] {
        for silent in [false, true] {
            for language in [Lang::En, Lang::Ja] {
                app.update(cx, |app, cx| app.set_lang(language, cx));
                wait_idle(cx, app);
                let before = read_oplog_tail_for_repo(repo, 100).len();
                let successes = read_oplog_tail_for_repo(repo, 100)
                    .iter()
                    .filter(|entry| {
                        entry.op == "fetch" && matches!(entry.outcome, OpOutcome::Success { .. })
                    })
                    .count();
                let retry_body = format!("retry {stage} {silent} {language:?}\n");
                let tracking_before =
                    git_fixture::git_output(repo, &["rev-parse", "refs/remotes/origin/main"]);
                let fetched_tip = if stage {
                    None
                } else {
                    std::fs::write(upstream.join("footer-upstream.txt"), &retry_body).unwrap();
                    git(upstream, &["add", "footer-upstream.txt"]);
                    git(upstream, &["commit", "-q", "-m", "footer recovery retry"]);
                    git(upstream, &["push", "-q", "origin", "main"]);
                    Some(git_fixture::git_output(upstream, &["rev-parse", "HEAD"]))
                };
                with_repository_lock(repo, || {
                    app.update(cx, |app, cx| {
                        assert!(!app.fetch_async_for(silent, None, cx));
                        let message = match &app.status_footer {
                            FooterStatus::Idle(message) if silent => message,
                            FooterStatus::Failed(message) if !silent => message,
                            other => panic!("wrong refusal footer: {other:?}"),
                        };
                        assert_eq!(message.as_ref(), Msg::AdviceConflictsRepositoryBusy.t());
                    });
                    assert_eq!(
                        git_fixture::git_output(repo, &["rev-parse", "refs/remotes/origin/main"]),
                        tracking_before,
                        "the contended fetch must not publish the pending upstream change"
                    );
                });
                let refused = read_oplog_tail_for_repo(repo, 100).len();
                assert_eq!(refused, before + usize::from(!silent));
                app.update(cx, |app, cx| {
                    app.set_lang(
                        if language == Lang::En {
                            Lang::Ja
                        } else {
                            Lang::En
                        },
                        cx,
                    );
                });
                wait_idle(cx, app);
                std::fs::write(repo.join("footer-retry.txt"), &retry_body).unwrap();
                app.update(cx, |app, cx| {
                    if stage {
                        app.do_stage_file_by_path(
                            app.active_session().unwrap(),
                            "footer-retry.txt".into(),
                            cx,
                        );
                    } else {
                        assert!(app.fetch_async_for(true, None, cx));
                    }
                    assert!(
                        matches!(&app.status_footer, FooterStatus::Idle(message)
                        if message.is_empty()),
                        "admission must retire the busy claim"
                    );
                });
                wait_idle(cx, app);
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(!state.app_sessions.has_leases());
                    assert!(matches!(&state.status_footer, FooterStatus::Idle(message)
                        if message.is_empty()));
                });
                let entries = read_oplog_tail_for_repo(repo, 100);
                assert!(entries
                    .iter()
                    .all(|entry| entry.op != "conflict-write-admission"));
                assert_eq!(
                    entries.len(),
                    refused + usize::from(!stage),
                    "only fetch completion records; synchronous stage adds no success receipt"
                );
                assert_eq!(
                    entries
                        .iter()
                        .filter(|entry| entry.op == "fetch"
                            && matches!(entry.outcome, OpOutcome::Success { .. }))
                        .count(),
                    successes + usize::from(!stage),
                    "the admitted fetch must finish successfully"
                );
                if stage {
                    assert_eq!(
                        git_fixture::git_output(repo, &["show", ":footer-retry.txt"]).trim_end(),
                        retry_body.trim_end(),
                        "the recovered stage must publish this attempt's actual content"
                    );
                } else {
                    assert_eq!(
                        git_fixture::git_output(repo, &["rev-parse", "refs/remotes/origin/main"]),
                        fetched_tip.unwrap(),
                        "the admitted quiet fetch must publish this attempt's upstream ref"
                    );
                }
            }
        }
        for footer in [
            FooterStatus::Idle("unrelated idle information".into()),
            FooterStatus::Failed("unrelated failure information".into()),
            FooterStatus::Success(Msg::AdviceConflictsRepositoryBusy.t().into()),
        ] {
            let expected = format!("{footer:?}");
            app.update(cx, |app, cx| {
                app.status_footer = footer;
                if stage {
                    app.do_stage_file_by_path(
                        app.active_session().unwrap(),
                        "footer-retry.txt".into(),
                        cx,
                    );
                } else {
                    assert!(app.fetch_async_for(true, None, cx));
                }
                assert_eq!(format!("{:?}", app.status_footer), expected);
            });
            wait_idle(cx, app);
        }
    }
    app.update(cx, |app, cx| app.set_lang(previous, cx));
}

pub fn scenario_repository_lock_stage_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    std::fs::write(repo.join("lock-stage.txt"), "unstaged\n").unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| {
            let owner = app.active_session().unwrap();
            app.do_stage_file_by_path(owner, "lock-stage.txt".into(), cx);
        });
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "stage");
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        let entries = read_oplog_tail_for_repo(&repo, 100);
        let receipt = entries.iter().find(|entry| entry.op == "stage").unwrap();
        assert!(matches!(&receipt.outcome, OpOutcome::Refused { blockers }
            if blockers[0].contains("lock-stage.txt")));
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.app_sessions.has_leases());
            assert!(
                matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message)
                    if kagi_ui_core::i18n::op::user_text::operations::is_repository_busy_message(message)),
                "active staging refusal retains the specific repository-busy cause"
            );
        });
    });
    app.update(cx, |app, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo.clone(), cx);
    });
    wait_idle(cx, &app);
    app.update(cx, |app, cx| {
        app.do_stage_all(app.active_session().unwrap(), cx);
        assert!(
            matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(message)
            if message.is_empty()),
            "successful Stage All must clear the refusal"
        );
    });
    wait_idle(cx, &app);
    assert!(
        !git_fixture::git_output(&repo, &["diff", "--cached", "--", "lock-stage.txt"],).is_empty()
    );
    assert_one_refusal(&repo, before, "stage");
    unmount(cx, app, window);
}

/// A busy head retains one retry, not a repeating admission/replanning loop.
/// Measure only dispatcher work: fixture Git and assertion I/O are excluded.
pub fn scenario_repository_lock_queue_retry(cx: &mut VisualTestAppContext) {
    use crate::evidence_support::deferred;
    use crate::macos::git;
    use kagi::ui::{e2e, KagiApp, ToastKind};
    use std::time::{Duration, Instant};

    const TICK: Duration = Duration::from_millis(250);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for branch in ["a", "b", "c"] {
        git(&repo, &["branch", branch]);
    }
    let (app, window) = mount(cx, &repo);
    wait_idle(cx, &app);
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let head_before = std::fs::read(repo.join(".git/HEAD")).unwrap();
    let index_before = std::fs::read(repo.join(".git/index")).unwrap();
    let plan_count = || {
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| line.contains("queue: plan checkout b "))
            .count()
    };
    let error_ids = |cx: &mut VisualTestAppContext| {
        cx.read(|cx| {
            app.read(cx)
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .filter(|toast| toast.kind == ToastKind::Error)
                .map(|toast| toast.id)
                .collect::<Vec<_>>()
        })
    };
    assert!(error_ids(cx).is_empty());
    let mut queued_ids = Vec::new();
    with_repository_lock(&repo, || {
        // Use the existing queue-entry seam so acquisition precedes admission,
        // without racing an independently completing predecessor.
        app.update(cx, |app, cx| {
            assert!(e2e::enqueue_checkout(app, "b", cx));
            assert!(e2e::enqueue_checkout(app, "c", cx));
        });
        cx.run_until_parked();
        queued_ids = cx.read(|cx| app.read(cx).queue_ids_for_e2e());
        assert_eq!(queued_ids.len(), 2);
        assert_eq!(
            cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().0),
            1,
            "only the RepositoryBusy head retains a retry task"
        );
        let retry_before = cx.read(|cx| app.read(cx).repository_retry_state_for_e2e());
        let now = cx.background_executor.now();
        let strip = cx.read(|cx| app.read(cx).queue_strip_for_e2e(now)).unwrap();
        assert_eq!(
            strip.1[0],
            (
                "checkout b".to_string(),
                kagi_ui_core::i18n::Msg::AdviceConflictsRepositoryBusy
                    .t()
                    .to_string()
            ),
            "the head explicitly waits for repository admission"
        );
        let errors = error_ids(cx);
        assert_eq!(errors.len(), 1, "the busy intent reports exactly one Error");
        let plans = plan_count();
        let mut dispatch_wall = Duration::ZERO;
        for _ in 0..20 {
            let started = Instant::now();
            cx.advance_clock(TICK);
            cx.run_until_parked();
            dispatch_wall += started.elapsed();
            assert_eq!(error_ids(cx), errors, "retry must not repeat its toast");
            assert_eq!(plan_count(), plans, "busy probes must not replan");
            assert_eq!(
                read_oplog_tail_for_repo(&repo, 100).len(),
                before + 1,
                "one user-attempt refusal, never one receipt per probe"
            );
            cx.read(|cx| {
                let state = app.read(cx);
                assert_eq!(state.queue_ids_for_e2e(), queued_ids);
                let retry = state.repository_retry_state_for_e2e();
                assert_eq!(retry.0, 1);
                assert_eq!(
                    retry.2, retry_before.2,
                    "a busy probe never emits availability"
                );
                assert!(!state.app_sessions.has_leases());
                assert!(state.plan_modal().is_none());
                assert!(state.app_notice().is_none());
            });
        }
        assert!(
            dispatch_wall < Duration::from_millis(800),
            "five simulated seconds of isolated ticks blocked the UI for {dispatch_wall:?}"
        );
        let retry_after = cx.read(|cx| app.read(cx).repository_retry_state_for_e2e());
        assert!(
            retry_after.1 >= retry_before.1 + 3,
            "background retries were exercised"
        );
        assert_eq!(std::fs::read(repo.join(".git/HEAD")).unwrap(), head_before);
        assert_eq!(
            std::fs::read(repo.join(".git/index")).unwrap(),
            index_before
        );
    });

    // A second window wins admission after the first successful probe. The
    // old retry must retire before reentrant admission registers its successor.
    let second_owner = std::rc::Rc::new(std::cell::RefCell::new(None));
    let second_slot = second_owner.clone();
    let second_repo = repo.clone();
    KagiApp::before_next_repository_resume_for_e2e(move || {
        *second_slot.borrow_mut() = Some(RepositoryLockOwner::acquire(&second_repo));
    });
    let errors_before_race = error_ids(cx);
    for _ in 0..8 {
        cx.advance_clock(TICK);
        cx.run_until_parked();
        if second_owner.borrow().is_some() {
            break;
        }
    }
    assert!(second_owner.borrow().is_some(), "second owner interleaves");
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.queue_ids_for_e2e(), queued_ids);
        assert_eq!(
            state.repository_retry_state_for_e2e().0,
            1,
            "re-contention registers a fresh retry before the old task returns"
        );
        assert_eq!(state.repository_retry_state_for_e2e().2, 1);
        assert!(!state.app_sessions.has_leases());
    });
    for _ in 0..4 {
        cx.advance_clock(TICK);
        cx.run_until_parked();
    }
    assert_eq!(
        error_ids(cx),
        errors_before_race,
        "same intent still toasts once"
    );
    assert_one_refusal(&repo, before, "checkout");
    drop(second_owner.borrow_mut().take());

    // Freeze the resumed write to inspect the untouched successor before it runs.
    let (hold, release) = deferred::<()>(cx);
    KagiApp::hold_next_run_for_e2e(hold);
    for _ in 0..8 {
        cx.advance_clock(TICK);
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).app_sessions.has_leases()) {
            break;
        }
    }
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.app_sessions.has_leases(),
            "head resumes within the 2 s cap"
        );
        assert_eq!(
            state.queue_ids_for_e2e(),
            queued_ids,
            "tail survives contention"
        );
        let retry = state.repository_retry_state_for_e2e();
        assert_eq!(retry.0, 0);
        assert_eq!(
            retry.2, 2,
            "the second availability resumes the re-contended head"
        );
    });
    release.send(());
    for _ in 0..80 {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).queue_ids_for_e2e().is_empty()) {
            break;
        }
        cx.advance_clock(TICK);
    }
    assert!(cx.read(|cx| app.read(cx).queue_ids_for_e2e().is_empty()));
    assert_eq!(
        std::fs::read_to_string(repo.join(".git/HEAD")).unwrap(),
        "ref: refs/heads/c\n"
    );
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(
        receipts.len(),
        before + 3,
        "one refused attempt and the two real checkouts record"
    );
    assert!(receipts
        .iter()
        .all(|entry| entry.op != "conflict-write-admission"));
    assert_eq!(
        receipts
            .iter()
            .filter(|entry| entry.op == "checkout"
                && matches!(&entry.outcome, OpOutcome::Success { .. }))
            .count(),
        2,
        "both the resumed head and its tail finish successfully"
    );

    // Closing the owner drops its pending timer; releasing the external owner
    // afterwards must never resurrect the abandoned checkout.
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| assert!(e2e::enqueue_checkout(app, "a", cx)));
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().0),
            1
        );
        app.update(cx, |app, cx| app.close_tab(0, cx));
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().0),
            0
        );
    });
    for _ in 0..8 {
        cx.advance_clock(TICK);
        cx.run_until_parked();
    }
    assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), before + 4);
    assert_eq!(
        std::fs::read_to_string(repo.join(".git/HEAD")).unwrap(),
        "ref: refs/heads/c\n"
    );
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert_eq!(
        cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().2),
        2,
        "a closed owner's retry never emits availability"
    );
    unmount(cx, app, window);
    retry_nonblocker_reports_real_error(cx);
}

fn retry_nonblocker_reports_real_error(cx: &mut VisualTestAppContext) {
    use crate::macos::git;
    use kagi::ui::{e2e, ToastKind};
    use std::time::Duration;

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "b"]);
    let (app, window) = mount(cx, &repo);
    wait_idle(cx, &app);
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    let head = std::fs::read(repo.join(".git/HEAD")).unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| assert!(e2e::enqueue_checkout(app, "b", cx)));
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().0),
            1
        );
    });
    // Keep the repository healthy, but make opening the admission sidecar fail
    // with a non-contention I/O error. This is deterministic even as root.
    let sidecar = repo.join(".git/kagi-write.lock");
    let preserved = repo.join(".git/kagi-write.lock.preserved");
    std::fs::rename(&sidecar, &preserved).unwrap();
    std::fs::create_dir(&sidecar).unwrap();
    let error = kagi_git::Backend::check_conflict_write_admission(&repo).unwrap_err();
    assert!(error.blocker().is_none());
    let expected = kagi_ui_core::i18n::op::user_text::diagnostic(
        &kagi::app::AdmissionError::Identity(error.to_string()).to_string(),
    )
    .into_owned();
    for _ in 0..8 {
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).repository_retry_state_for_e2e().2) > 0 {
            break;
        }
    }
    let now = cx.background_executor.now();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state.repository_retry_state_for_e2e().0,
            0,
            "non-blocker errors retire the busy retry instead of waiting forever"
        );
        assert_eq!(state.repository_retry_state_for_e2e().2, 1);
        let strip = state.queue_strip_for_e2e(now).unwrap();
        assert!(
            strip.1.is_empty(),
            "the head no longer waits for repository contention"
        );
        assert_eq!(strip.2.len(), 1, "normal admission cancels the failed head");
        assert!(
            state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| toast.kind == ToastKind::Error && toast.message.as_ref() == expected),
            "normal admission reports the actual non-blocker diagnostic"
        );
        assert!(!state.app_sessions.has_leases());
    });
    let entries = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(
        entries.len(),
        before + 2,
        "contention and the actual terminal failure each record"
    );
    assert!(entries.iter().all(|entry| entry.op == "checkout"));
    assert!(entries.iter().any(|entry| matches!(&entry.outcome,
        OpOutcome::Refused { blockers } if blockers.iter().any(|blocker| blocker.contains(&error.to_string())))));
    assert_eq!(std::fs::read(repo.join(".git/HEAD")).unwrap(), head);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    std::fs::remove_dir(&sidecar).unwrap();
    std::fs::rename(&preserved, &sidecar).unwrap();
    unmount(cx, app, window);
}

pub fn scenario_repository_lock_user_receipts(cx: &mut VisualTestAppContext) {
    use crate::macos::git;
    use kagi::ui::{e2e, ToastKind};

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote = tempfile::tempdir().unwrap();
    crate::fetch_owner::local_remote_with_change(&repo, remote.path());
    std::fs::write(repo.join("pending.txt"), "user commit\n").unwrap();
    git(&repo, &["add", "pending.txt"]);
    let (app, window) = mount(cx, &repo);
    wait_idle(cx, &app);
    let head = std::fs::read(repo.join(".git/HEAD")).unwrap();
    let oid = std::fs::read(repo.join(".git/refs/heads/main")).unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let pending = std::fs::read(repo.join("pending.txt")).unwrap();
    let before = read_oplog_tail_for_repo(&repo, 100).len();
    with_repository_lock(&repo, || {
        app.update(cx, |app, cx| app.open_pull_modal(cx));
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before, "pull");
        cx.read(|cx| {
            assert!(
                matches!(&app.read(cx).status_footer, kagi::ui::FooterStatus::Failed(message)
                    if message.as_ref() == kagi_ui_core::i18n::Msg::AdviceConflictsRepositoryBusy.t()),
                "Pull must not overwrite repository contention with generic OpInProgress"
            );
        });
        app.update(cx, |app, cx| {
            e2e::open_local_panel_no_inputs(app, repo.clone(), cx);
            e2e::set_commit_message(app, "user admission refusal", cx);
        });
        wait_idle(cx, &app);
        app.update(cx, |app, cx| {
            app.open_commit_plan_modal(app.active_session().unwrap(), cx);
        });
        wait_idle(cx, &app);
        assert_one_refusal(&repo, before + 1, "commit");
        let entries = read_oplog_tail_for_repo(&repo, 100);
        assert_eq!(entries.iter().filter(|entry| entry.op == "pull").count(), 1);
        assert_eq!(
            entries.iter().filter(|entry| entry.op == "commit").count(),
            1
        );
        assert!(entries
            .iter()
            .all(|entry| entry.ref_moves.as_ref().is_some_and(Vec::is_empty)));
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.app_sessions.has_leases());
            assert!(state.fetch_in_flight.is_none());
            let stack = state.toast_stack.as_ref().unwrap().read(cx);
            let mut errors = 0;
            for toast in stack
                .toasts()
                .iter()
                .filter(|toast| toast.kind == ToastKind::Error)
            {
                errors += 1;
                assert!(
                    kagi_ui_core::i18n::op::user_text::operations::is_repository_busy_message(
                        &toast.message
                    )
                );
                assert!(toast.result_key.is_some(), "recorded refusal is keyed");
                assert!(
                    matches!(
                        &toast.action,
                        Some(kagi::ui::ToastAction::OpenOperationLog(_))
                    ),
                    "recorded refusal links directly to its durable receipt"
                );
                assert!(!toast.message.contains(repo.to_str().unwrap()));
            }
            assert_eq!(
                errors, 2,
                "each user attempt gets one bounded error notification"
            );
        });
        assert_eq!(std::fs::read(repo.join(".git/HEAD")).unwrap(), head);
        assert_eq!(
            std::fs::read(repo.join(".git/refs/heads/main")).unwrap(),
            oid
        );
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(repo.join("pending.txt")).unwrap(), pending);
    });
    unmount(cx, app, window);
}

pub fn scenario_repository_lock_admission_recording_failure(cx: &mut VisualTestAppContext) {
    use kagi::ui::{
        i18n::{self, Lang},
        FooterStatus,
    };
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let remote = tempfile::tempdir().unwrap();
        crate::fetch_owner::local_remote_with_change(&repo, remote.path());
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.set_lang(language, cx));
        wait_idle(cx, &app);
        let before = read_oplog_tail_for_repo(&repo, 100).len();
        let log_lock = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
            .join("operations.jsonl.lock");
        let append_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(log_lock)
            .unwrap();
        append_lock.lock().unwrap();
        with_repository_lock(&repo, || {
            app.update(cx, |app, cx| app.fetch_async(false, cx));
            wait_idle(cx, &app);
            cx.read(|cx| {
                let state = app.read(cx);
                let rows = state
                    .op_log
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .entries()
                    .iter()
                    .filter(|entry| entry.op == "fetch" && entry.repo == repo.display().to_string())
                    .collect::<Vec<_>>();
                assert_eq!(rows.len(), 1, "a failed append appears once, not twice");
                assert!(matches!(&rows[0].outcome, OpOutcome::Refused { .. }));
                assert_eq!(rows[0].before.dirty, "unobserved");
                let notice = state
                    .app_notice()
                    .expect("required failed-recording acknowledgment");
                assert!(notice.message.contains(&repo.display().to_string()));
                assert!(notice
                    .message
                    .contains(&std::fs::TryLockError::WouldBlock.to_string()));
                assert!(
                    state
                        .toast_stack
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .toasts()
                        .is_empty(),
                    "no ordinary refusal beside required acknowledgment"
                );
                assert!(
                    matches!(&state.status_footer, FooterStatus::Idle(text) if text.is_empty())
                );
                assert!(state.fetch_in_flight.is_none());
                assert!(!state.app_sessions.has_leases());
            });
            assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), before);
        });
        drop(append_lock);
        unmount(cx, app, window);
    }
    i18n::set_lang(previous);
}
