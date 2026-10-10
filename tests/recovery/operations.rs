//! Real UI → backend → durable-record scenarios; no mocked mutation results.
use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use kagi::ui::{modals::ActiveModal, CheckoutSelected, FooterStatus, KagiApp, ToastKind};
use kagi_domain::branch_cleanup::{CleanupDeleteTarget, MergedBranchStatus};
use kagi_git::oplog::{append_oplog, read_oplog_tail_for_repo, FailureCode, OpLogEntry, OpOutcome};
use kagi_git::{CommitId, OperationKind, StateSummary};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as output;

pub(super) fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).write_busy_op.is_none()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "mutation did not release busy state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn press_key(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    key: &str,
) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        window.focus(&focus, cx);
        // Native offscreen windows do not pump AppKit frames. Paint the real
        // modal and its focus dispatch tree before delivering a raw key.
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, key);
}

pub(super) fn press_enter(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    press_key(cx, app, window, "enter");
}

pub(super) fn dispatch_checkout_selected(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        let focus = app.read(cx).root_focus.clone().unwrap();
        window.focus(&focus, cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.dispatch_action(window, CheckoutSelected);
}

fn records(repo: &Path, op: &str) -> Vec<OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|e| e.op == op)
        .collect()
}

fn recovery_oid(entry: &OpLogEntry, expected: &str) -> String {
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("expected successful durable entry: {:?}", entry.outcome);
    };
    after
        .dirty
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|oid| oid.len() == 40 && *oid == expected)
        .expect("full recovery OID must survive durable recording")
        .to_string()
}

pub fn scenario_stash_drop_persists(cx: &mut VisualTestAppContext) {
    for switch_away in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path();
        std::fs::write(repo.join("README.md"), "recover this stashed work\n").unwrap();
        git(repo, &["stash", "push", "-q", "-m", "recoverable"]);
        let oid = output(repo, &["rev-parse", "refs/stash"]);
        let fingerprint = repo_fingerprint(repo);
        let (app, window) = mount(cx, repo);
        let other = build_fixture();
        let other_before = repo_fingerprint(other.path());
        if switch_away {
            app.update(cx, |app, cx| {
                assert!(app.open_repository(other.path().to_path_buf(), cx));
                app.switch_repo(0, cx);
            });
            cx.run_until_parked();
        }
        app.update(cx, |app, cx| {
            app.open_stash_drop_modal(0, cx);
        });
        cx.run_until_parked();
        app.update(cx, |app, cx| {
            assert!(app
                .stash_drop_modal()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .blockers
                .is_empty());
            app.start_stash_drop(cx);
            if switch_away {
                app.switch_repo(1, cx);
            }
        });
        wait_idle(cx, &app);
        assert!(output(repo, &["stash", "list"]).is_empty());
        assert_eq!(fingerprint, repo_fingerprint(repo));
        assert_eq!(other_before, repo_fingerprint(other.path()));
        let entries = records(repo, "stash-drop");
        assert_eq!(
            entries.len(),
            1,
            "each executed drop must persist once, including stale UI completion"
        );
        let logged_oid = recovery_oid(&entries[0], &oid);
        git(repo, &["stash", "store", "-m", "recovered", &logged_oid]);
        assert_eq!(output(repo, &["rev-parse", "refs/stash"]), oid);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.stash_drop_modal().is_none());
            assert_eq!(app.active_tab, usize::from(switch_away));
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS stash_drop_persists active/stale completion and recovery");
}

pub fn scenario_history_persists(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let before = output(repo, &["rev-parse", "HEAD~1"]);
    let after = output(repo, &["rev-parse", "HEAD"]);
    let text = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").operation_history = Default::default();
        app.record_history(
            OperationKind::Commit,
            "main",
            CommitId(before.clone()),
            CommitId(after.clone()),
            "second commit",
        );
        app.open_history_undo_modal();
        assert!(app.history_modal().unwrap().plan.blockers.is_empty());
        app.confirm_history(cx);
    });
    assert_eq!(output(repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), text);
    let undo = records(repo, "undo-commit");
    assert_eq!(undo.len(), 1, "UI undo must persist exactly once");
    recovery_oid(&undo[0], &before);
    recovery_oid(&undo[0], &after);
    app.update(cx, |app, cx| {
        app.open_history_redo_modal();
        assert!(app.history_modal().unwrap().plan.blockers.is_empty());
        app.confirm_history(cx);
    });
    assert_eq!(output(repo, &["rev-parse", "HEAD"]), after);
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), text);
    let redo = records(repo, "redo-commit");
    assert_eq!(redo.len(), 1);
    recovery_oid(&redo[0], &before);
    recovery_oid(&redo[0], &after);
    cx.read(|cx| assert!(app.read(cx).history_modal().is_none()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS history_persists undo/redo with both recovery OIDs");
}

pub fn scenario_cleanup_stale_tab(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged", "HEAD~1"]);
    let oid = output(repo, &["rev-parse", "merged"]);
    let other = build_fixture();
    let other_before = repo_fingerprint(other.path());
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_branch_cleanup_plan(
            vec![CleanupDeleteTarget {
                name: "merged".into(),
                local_tip: Some(CommitId(oid.clone())),
                remote_tip: None,
                status: MergedBranchStatus::FullyMerged,
            }],
            cx,
        );
        assert!(app.branch_cleanup_modal().unwrap().plan.blockers.is_empty());
        app.confirm_branch_cleanup(cx);
        // No executor yield between confirmation and switch: the callback must
        // observe the changed generation even if the background write was fast.
        app.switch_repo(1, cx);
    });
    wait_idle(cx, &app);
    assert!(output(repo, &["for-each-ref", "refs/heads/merged"]).is_empty());
    assert_eq!(other_before, repo_fingerprint(other.path()));
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_tab, 1);
        assert!(app.branch_cleanup_modal().is_none());
        assert!(!matches!(&app.status_footer, FooterStatus::Success(s) if s.starts_with("branch-cleanup:")),
            "cleanup completion overwrote another tab's footer");
    });
    let entries = records(repo, "branch-cleanup");
    assert_eq!(
        entries.len(),
        1,
        "stale presentation must not discard or duplicate the durable result"
    );
    let logged_oid = recovery_oid(&entries[0], &oid);
    git(repo, &["branch", "merged", &logged_oid]);
    assert_eq!(output(repo, &["rev-parse", "merged"]), oid);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS cleanup_stale_tab durable recovery and UI ownership");
}

pub fn scenario_preflight_presentation(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    use kagi_ui_core::i18n::{self, Lang, Op};
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let fixture = build_fixture();
        let repo = fixture.path();
        std::fs::write(repo.join("README.md"), "first stash\n").unwrap();
        git(repo, &["stash", "push", "-q", "-m", "first"]);
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| {
            app.open_stash_drop_modal(0, cx);
        });
        cx.run_until_parked();
        let plan = cx.read(|cx| {
            app.read(cx)
                .stash_drop_modal()
                .unwrap()
                .plan
                .clone()
                .unwrap()
        });
        std::fs::write(repo.join("README.md"), "second stash\n").unwrap();
        git(repo, &["stash", "push", "-q", "-m", "second"]);
        let before = output(repo, &["stash", "list"]);
        let error = kagi_git::Backend::open(repo)
            .unwrap()
            .preflight_check_stash(&plan, plan.stash_count_at_plan())
            .unwrap_err();
        let blocker = error.blocker().expect("typed stale-stash refusal");
        let expected_blocker = blocker.message_en();
        let detail = i18n::plan_note_text(blocker);
        let expected = i18n::op_failed(Op::Preflight, detail);
        cx.run_until_parked();
        let fingerprint = repo_fingerprint(repo);
        // Local stash now shares the root Enter/button approval boundary.
        app.update(cx, |app, cx| app.start_stash_drop(cx));
        wait_idle(cx, &app);
        let entries = records(repo, "stash-drop");
        assert_eq!(entries.len(), 1);
        let OpOutcome::Refused { blockers } = &entries[0].outcome else {
            panic!("expected a durable refusal, got {:?}", entries[0].outcome);
        };
        assert_eq!(blockers.as_slice(), std::slice::from_ref(&expected_blocker));
        cx.read(|cx| {
            let app = app.read(cx);
            // #747 deliberately replaced dismiss-only notices with oplog + toast.
            assert!(
                app.app_notice().is_none(),
                "a recorded preflight refusal must not open a dismiss-only modal"
            );
            assert!(
                matches!(&app.status_footer, FooterStatus::Failed(message)
                    if message.as_ref() == expected.as_str()),
                "localized preflight detail must reach the footer: {:?}",
                app.status_footer
            );
            let toast = app
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert!(
                matches!(toast.kind, kagi::ui::ToastKind::Error)
                    && toast.message.ends_with(&expected),
                "localized preflight detail must reach the error toast: {}",
                toast.message
            );
            let panel = app.op_log.as_ref().unwrap().read(cx);
            let shown = panel
                .entries()
                .iter()
                .find(|entry| entry.id == entries[0].id)
                .expect("the durable refusal must be available in Operation Log");
            let OpOutcome::Refused { blockers } = &shown.outcome else {
                panic!("the panel lost the refusal: {:?}", shown.outcome);
            };
            assert_eq!(blockers.as_slice(), std::slice::from_ref(&expected_blocker));
        });
        assert_eq!(output(repo, &["stash", "list"]), before);
        assert_eq!(repo_fingerprint(repo), fingerprint);
        unmount(cx, app, window);

        let fixture = build_fixture();
        let repo = fixture.path();
        let before = output(repo, &["rev-parse", "HEAD~1"]);
        let after = output(repo, &["rev-parse", "HEAD"]);
        let (app, window) = mount(cx, repo);
        let plan = app.update(cx, |app, _| {
            app.ui_mut().expect("active session").operation_history = Default::default();
            app.record_history(
                OperationKind::Commit,
                "main",
                CommitId(before),
                CommitId(after),
                "second commit",
            );
            app.open_history_undo_modal();
            app.history_modal().unwrap().plan.clone()
        });
        git(
            repo,
            &["commit", "-q", "--allow-empty", "-m", "external change"],
        );
        let before = repo_fingerprint(repo);
        let error = kagi_git::Backend::open(repo)
            .unwrap()
            .preflight_check(&plan)
            .unwrap_err();
        let expected = i18n::op_failed(Op::Preflight, error);
        cx.run_until_parked();
        press_enter(cx, &app, window);
        cx.read(|cx| {
            assert_eq!(
                app.read(cx).history_modal().unwrap().error.as_deref(),
                Some(expected.as_str())
            );
        });
        assert_eq!(repo_fingerprint(repo), before);
        let entries = records(repo, "undo-commit");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Failed { .. }));
        unmount(cx, app, window);
    }
    i18n::set_lang(language);
    eprintln!("[gui-e2e] PASS preflight_presentation EN/JA stash handler/history Enter, refusal and localized phase");
}

/// ADR-0196 Wave 3: Branch Cleanup was the last non-run writer deriving its
/// own oplog entry from the outcome the backend had already recorded. The
/// receipt comes back on `CleanupReport` now, so the panel shows the durable
/// entry itself — and the cleanup is admitted through the write lease, which
/// is what holds quit while the deletions run.
pub fn scenario_cleanup_presents_backend_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged", "HEAD~1"]);
    let tip = output(repo, &["rev-parse", "merged"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_branch_cleanup_plan(
            vec![CleanupDeleteTarget {
                name: "merged".into(),
                local_tip: Some(CommitId(tip.clone())),
                remote_tip: None,
                status: MergedBranchStatus::FullyMerged,
            }],
            cx,
        );
        assert!(app.branch_cleanup_modal().unwrap().plan.blockers.is_empty());
    });
    app.update(cx, |app, cx| {
        app.confirm_branch_cleanup(cx);
        assert!(
            app.app_sessions.has_leases(),
            "a dispatched cleanup must hold the write lease (ADR-0196 Wave 3)"
        );
        assert!(
            !app.app_sessions.may_close_host(),
            "quit must be held while branches are being deleted"
        );
    });
    wait_idle(cx, &app);
    assert!(output(repo, &["for-each-ref", "refs/heads/merged"]).is_empty());
    let durable = records(repo, "branch-cleanup");
    assert_eq!(durable.len(), 1, "one attempt, one durable entry");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !app.app_sessions.has_leases(),
            "the lease is released when the cleanup settles"
        );
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|e| e.op == "branch-cleanup")
            .collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, durable[0].id,
            "the panel entry is the durable receipt"
        );
        // A UI copy is built by `OpLogEntry::new`, which sets neither the
        // worktree nor the actor the backend stamps on what it records.
        assert_eq!(
            (
                shown[0].worktree.as_deref(),
                shown[0].timestamp,
                &shown[0].before.head
            ),
            (
                durable[0].worktree.as_deref(),
                durable[0].timestamp,
                &durable[0].before.head
            ),
            "the panel shows the recorded receipt, not an entry re-derived from the outcome"
        );
        assert!(
            shown[0].worktree.is_some(),
            "the backend stamps the worktree on what it records"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS cleanup_presents_backend_receipt: lease held, receipt presented");
}

pub fn scenario_cleanup_open_failure(cx: &mut VisualTestAppContext) {
    for switch_away in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path();
        git(repo, &["branch", "merged", "HEAD~1"]);
        let before = repo_fingerprint(repo);
        let tip = output(repo, &["rev-parse", "merged"]);
        let other = build_fixture();
        let other_before = repo_fingerprint(other.path());
        let (app, window) = mount(cx, repo);
        if switch_away {
            app.update(cx, |app, cx| {
                assert!(app.open_repository(other.path().to_path_buf(), cx));
                app.switch_repo(0, cx);
            });
            cx.run_until_parked();
        }
        app.update(cx, |app, cx| {
            app.open_branch_cleanup_plan(
                vec![CleanupDeleteTarget {
                    name: "merged".into(),
                    local_tip: Some(CommitId(tip.clone())),
                    remote_tip: None,
                    status: MergedBranchStatus::FullyMerged,
                }],
                cx,
            );
            assert!(app.branch_cleanup_modal().unwrap().plan.blockers.is_empty());
        });
        std::fs::rename(repo.join(".git"), repo.join("git-unavailable")).unwrap();
        app.update(cx, |app, cx| {
            app.confirm_branch_cleanup(cx);
            if switch_away {
                app.switch_repo(1, cx);
            }
        });
        wait_idle(cx, &app);
        std::fs::rename(repo.join("git-unavailable"), repo.join(".git")).unwrap();
        assert_eq!(repo_fingerprint(repo), before);
        assert_eq!(repo_fingerprint(other.path()), other_before);
        // ADR-0196 Wave 3: the cleanup rides the run family now, so a
        // repository that will not open is refused at *admission* — nothing
        // ran, nothing was written, and there is no attempt to record. (An
        // open failure after admission is `RunJob::run`'s own recording, which
        // lands before the ownership guard the way this used to.)
        assert!(
            records(repo, "branch-cleanup").is_empty(),
            "a refused admission executed nothing, so it records nothing"
        );
        cx.read(|cx| {
            let app = app.read(cx);
            if switch_away {
                assert_eq!(app.active_tab, 1);
                assert!(app.branch_cleanup_modal().is_none());
            } else {
                assert!(
                    app.branch_cleanup_modal().is_some(),
                    "a refused admission must keep the confirmation"
                );
                assert!(matches!(app.status_footer, FooterStatus::Failed(_)));
            }
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS cleanup_open_failure active/stale refusal without mutation");
}

/// A partial branch cleanup (the remote deletion lands, the local one is
/// refused) is recorded once, never retried, and explained with per-target
/// details in the Operation Log. Presentation follows #747 (e3a6d239): a
/// recorded outcome is shown as an error toast plus the expanded log entry,
/// not as a dismiss-only notice.
pub fn scenario_cleanup_partial_presentation(cx: &mut VisualTestAppContext) {
    use std::os::unix::fs::PermissionsExt;

    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged", "HEAD~1"]);
    let oid = output(repo, &["rev-parse", "merged"]);
    let moved = output(repo, &["rev-parse", "HEAD"]);
    let remote = tempfile::TempDir::new().unwrap();
    git(remote.path(), &["init", "--bare", "."]);
    git(
        repo,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(repo, &["push", "origin", "merged"]);
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_branch_cleanup_plan(
            vec![CleanupDeleteTarget {
                name: "merged".into(),
                local_tip: Some(CommitId(oid.clone())),
                remote_tip: Some(CommitId(oid.clone())),
                status: MergedBranchStatus::FullyMerged,
            }],
            cx,
        );
        assert!(app.branch_cleanup_modal().unwrap().plan.blockers.is_empty());
    });
    // The real remote commits its deletion before this hook makes the local
    // target stale. Neither a mocked result nor a sleep controls the failure.
    let hook = remote.path().join("hooks/post-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    git(
        remote.path(),
        &[
            "config",
            "core.hooksPath",
            hook.parent().unwrap().to_str().unwrap(),
        ],
    );
    let local_git_dir = repo
        .join(".git")
        .display()
        .to_string()
        .replace('\'', "'\\''");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nexec git --git-dir='{local_git_dir}' update-ref refs/heads/merged {moved}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    assert!(output(remote.path(), &["for-each-ref", "refs/heads/merged"]).is_empty());
    assert_eq!(output(repo, &["rev-parse", "merged"]), moved);
    assert_eq!(repo_fingerprint(repo), before);

    let entries = records(repo, "branch-cleanup");
    assert_eq!(entries.len(), 1);
    let recovered = match &entries[0].outcome {
        OpOutcome::Partial { after, .. } => after
            .dirty
            .split(|c: char| !c.is_ascii_hexdigit())
            .find(|token| *token == oid)
            .expect("durable remote recovery OID")
            .to_string(),
        other => panic!("expected partial durable cleanup: {other:?}"),
    };
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.branch_cleanup_modal().is_none(),
            "do not retry the deleted batch"
        );
        let FooterStatus::Failed(footer) = &app.status_footer else {
            panic!(
                "the footer must report the partial cleanup: {:?}",
                app.status_footer
            )
        };
        // #747: a recorded outcome is no longer a dismiss-only notice; the
        // toast and the Operation Log carry it.
        assert!(
            app.app_notice().is_none(),
            "a recorded partial outcome opens no notice (#747)"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .expect("the partial outcome reaches a toast");
        assert!(
            matches!(toast.kind, kagi::ui::ToastKind::Error)
                && toast.message.contains(footer.as_ref())
                && toast.message.contains("merged"),
            "the toast names the batch and what failed: {}",
            toast.message
        );
        assert!(app.bottom_panel_open);
        assert_eq!(app.bottom_tab, kagi::ui::BottomTab::OperationLog);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        assert_eq!(panel.expanded(), Some(0));
        let entry = &panel.entries()[0];
        let OpOutcome::Partial { error, .. } = &entry.outcome else {
            panic!("partial cleanup must be presented as partial");
        };
        let details = kagi::ui::oplog_panel::detail_lines(entry).join("\n");
        assert!(details.contains(&oid));
        assert!(details.contains("merged"));
        assert!(
            details.contains(error),
            "per-target failure must be visible"
        );
    });
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    std::fs::remove_file(hook).unwrap();
    git(
        repo,
        &["push", "origin", &format!("{recovered}:refs/heads/merged")],
    );
    assert_eq!(output(remote.path(), &["rev-parse", "merged"]), oid);
    assert_eq!(output(repo, &["rev-parse", "merged"]), moved);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS cleanup_partial_presentation per-target details and remote recovery");
}

/// #492: the five modals the old hand-written Enter chain never named
/// (create-tag, delete-remote-branch, reset-current, force-with-lease-push,
/// rebase-onto). With a commit row selected behind them, Enter used to fall
/// through to `checkout_selected_commit`, which replaced the confirmation with
/// a checkout plan for that commit. Each one must now consume Enter itself,
/// Esc must clear the slot and leave the app idle, and a confirmation planned
/// in repo A must not survive a switch to repo B.
///
/// The oracle is the modal slot, not stderr: the fall-through's only
/// observable act is `open_plan_modal` / `open_checkout_commit_modal`, which
/// sets `ActiveModal::Checkout` and emits the `[kagi] plan: checkout …` line
/// together. Asserting the slot therefore also asserts the absent klog line —
/// without teaching the runner to capture its own stderr.
pub fn scenario_modal_no_fallthrough(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = output(repo, &["rev-parse", "HEAD"]);
    let parent = CommitId(output(repo, &["rev-parse", "HEAD~1"]));
    let (app, window) = mount(cx, repo);

    // Row 1 is the parent commit — deliberately NOT HEAD. `checkout_selected_commit`
    // returns early on an already-checked-out row, so selecting row 0 would hide
    // the regression instead of catching it.
    app.update(cx, |app, _| app.select_headless(1));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(1),
        "the fall-through needs a non-HEAD row selected behind the modal"
    );

    for case in [
        "create-tag",
        "delete-remote-branch",
        "reset-current",
        "force-lease-push",
        "rebase-onto",
    ] {
        // Every plan below carries blockers on a fixture with no remote and no
        // second branch, so a two-stage confirm only ever arms and a single
        // confirm only ever refuses — no scenario step can mutate the repo.
        app.update(cx, |app, cx| match case {
            "create-tag" => app.open_create_tag_modal(parent.clone(), cx),
            "delete-remote-branch" => app.open_delete_remote_branch_modal("origin/main"),
            "reset-current" => app.open_reset_current_modal(parent.clone(), cx),
            "force-lease-push" => app.open_force_lease_push_modal(cx),
            _ => app.open_rebase_modal("main".to_string(), cx),
        });
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).active_modal.is_some()),
            "{case}: the modal must open before Enter is tested"
        );

        press_enter(cx, &app, window);
        cx.run_until_parked();
        cx.read(|cx| {
            assert!(
                !matches!(app.read(cx).active_modal, Some(ActiveModal::Checkout(_))),
                "{case}: Enter fell through to the commit checkout behind the modal"
            );
        });
        assert_eq!(
            output(repo, &["rev-parse", "HEAD"]),
            head,
            "{case}: Enter must not move HEAD"
        );

        press_key(cx, &app, window, "escape");
        cx.run_until_parked();
        // The root must own focus for Esc to resolve against the app keymap;
        // `false` here would mean a modal text input took it, which is a
        // different failure than the slot surviving a delivered Esc.
        let root_focused = cx
            .update_window(window, |_, window, cx| {
                app.read(cx)
                    .root_focus
                    .as_ref()
                    .is_some_and(|fh| fh.is_focused(window))
            })
            .unwrap();
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                app.active_modal.is_none(),
                "{case}: Esc must clear the modal slot (root focused at Esc: {root_focused})"
            );
            assert!(
                app.write_busy_op.is_none(),
                "{case}: the app must stay usable after Esc"
            );
        });
        assert_eq!(output(repo, &["rev-parse", "HEAD"]), head);
        // Per-case progress: the loop aborts on the first failure, so without
        // this the log cannot show which variants were actually exercised.
        eprintln!("[gui-e2e] ok modal_no_fallthrough/{case}");
    }

    // Repo binding: a destructive confirmation planned against A must be gone
    // once B is active, so its plan can never be applied to B.
    let other = build_fixture();
    let other_before = repo_fingerprint(other.path());
    app.update(cx, |app, cx| app.open_reset_current_modal(parent, cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| matches!(
            app.read(cx).active_modal,
            Some(ActiveModal::ResetCurrent(_))
        )),
        "reset-current must be the active modal before the switch"
    );
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.active_modal.is_none(),
            "a confirmation planned in repo A must not survive the switch to repo B"
        );
        assert!(app.write_busy_op.is_none());
    });
    assert_eq!(output(repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        repo_fingerprint(other.path()),
        other_before,
        "repo B must be untouched by A's dropped confirmation"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS modal_no_fallthrough: 5 previously-unrouted modals consume Enter/Esc, repo-scoped confirm dropped on switch"
    );
}

/// #993 P2: a blocked common plan must not offer a runnable equivalent
/// command through the disclosure, its dedicated Copy action, or Copy all.
pub fn scenario_blocked_plan_command(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let target = CommitId(output(repo, &["rev-parse", "HEAD~1"]));
    git(repo, &["checkout", "-q", "--detach", "HEAD"]);
    let before = repo_fingerprint(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_reset_current_modal(target.clone(), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let plan = &app.read(cx).reset_current_modal().expect("Reset card").plan;
        assert_eq!(
            plan.disposition,
            kagi_domain::plan_note::PlanDisposition::Blocked
        );
        assert!(
            !plan.blockers.is_empty(),
            "detached HEAD blocks Reset Current"
        );
        assert_eq!(
            plan.equivalent_command.as_deref(),
            Some(format!("git reset --soft '{}'", target.0).as_str()),
            "the backend still supplies the command on blocked plans"
        );
    });
    paint(cx, window);
    for id in [
        "plan-equivalent-command",
        "plan-equivalent-command-copy",
        "plan-recovery",
        "plan-recovery-copy",
    ] {
        assert!(
            kagi::ui::e2e::control_bounds(window.window_id(), id).is_none(),
            "blocked Reset Current must hide {id}"
        );
    }
    let copy = kagi::ui::e2e::control_bounds(window.window_id(), "plan-card-copy")
        .expect("Copy all remains available for blocker details");
    cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("blocked plan details copied");
    assert!(copied.contains("blocker:"), "{copied}");
    assert!(
        !copied.contains(&format!("git reset --soft {}", target.0)),
        "{copied}"
    );
    assert!(!copied.contains("\nequivalent command:\n"), "{copied}");
    assert_eq!(repo_fingerprint(repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS blocked_plan_command: detached Reset Current hides executable-looking equivalent and recovery disclosures");
}

/// Escape with nothing else to close clears the Graph selection, which closes
/// the commit details pane; a second Escape on no selection does nothing. An
/// open modal still takes Escape first.
pub fn scenario_graph_escape_clears_selection(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let (app, window) = mount(cx, repo);
    let selected = |cx: &mut VisualTestAppContext| cx.read(|cx| app.read(cx).ui().selected);

    app.update(cx, |app, _| app.select_headless(1));
    assert_eq!(selected(cx), Some(1), "a Graph row is selected");
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert_eq!(
        selected(cx),
        None,
        "graph-escape-clears-selection: Esc must clear the Graph selection"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert_eq!(selected(cx), None, "a second Esc on no selection stays put");

    // A sidebar menu opened over the selection closes first (#920 review).
    app.update(cx, |app, _| app.select_headless(1));
    app.update(cx, |app, _| {
        app.worktree_menu = Some(kagi::ui::worktree_menu::WorktreeMenuState {
            name: "main".into(),
            locked: false,
            is_main: true,
            path: None,
            position: gpui::point(gpui::px(10.), gpui::px(10.)),
        });
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).worktree_menu.is_none()),
        "graph-escape-menu-first: Esc closes the worktree menu"
    );
    assert_eq!(
        selected(cx),
        Some(1),
        "graph-escape-menu-first: the selection behind the menu survives"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert_eq!(selected(cx), None);

    // A modal in front keeps Escape: the selection behind it survives.
    app.update(cx, |app, _| app.select_headless(1));
    press_enter(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_none()));
    assert_eq!(
        selected(cx),
        Some(1),
        "graph-escape-modal-first: Esc closes the modal, not the selection behind it"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS graph_escape_clears_selection: Esc clears the Graph selection; a modal keeps Esc first");
}

/// #643: Remote Browse owns Enter/Esc while it occupies the shared modal slot.
/// A selected non-HEAD commit and a live diff selection make both historical
/// fallthrough paths observable.
pub fn scenario_remote_browse_modal_routing(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = output(repo, &["rev-parse", "HEAD"]);
    let remote_snapshot = kagi_git::Backend::open(repo)
        .expect("open fixture")
        .snapshot(100)
        .expect("snapshot fixture");
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, _| app.select_headless(1));
    press_enter(cx, &app, window);
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_some()),
        "workspace-enter-falls-through-without-modal: Enter must still open checkout when the modal slot is vacant"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_none()),
        "workspace-escape-still-cancels-modal: Esc must still cancel the workspace checkout modal"
    );
    kagi::ui::e2e::seed_diff_selection();

    app.update(cx, |app, cx| app.open_remote_browse(cx));
    press_enter(cx, &app, window);
    cx.run_until_parked();

    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        head,
        "remote-browse-enter-preserves-head: Enter must not checkout the selected commit"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.remote_browse()
                .is_some_and(|modal| modal.error.is_some()),
            "remote-browse-enter-confirms-front: Enter must run the connection-form validation"
        );
    });

    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_none()),
        "remote-browse-escape-closes-front: Esc must close Remote Browse"
    );
    assert!(
        kagi::ui::e2e::diff_selection_present(),
        "remote-browse-escape-preserves-diff-selection: Esc must not reach the diff behind the modal"
    );

    // Clear the process-global selection so later scenarios stay isolated.
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(!kagi::ui::e2e::diff_selection_present());

    // The no-tab Welcome path must install the exact same modal-key wrapper.
    app.update(cx, |app, cx| app.close_tab(0, cx));
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .remote_browse()
            .is_some_and(|modal| modal.error.is_some())),
        "welcome-modal-enter-confirms-connect: Enter on Welcome must run connection-form validation"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_none()),
        "welcome-modal-escape-closes-remote: Esc on Welcome must close Remote Browse"
    );

    let host = kagi_domain::remote::RemoteHost::parse("example.test").expect("host");
    let stale_snapshot = remote_snapshot.clone();
    app.update(cx, |app, cx| {
        app.open_remote_browse(cx);
        kagi::ui::e2e::prepare_remote_browse_open(app, host.clone(), "/stale");
    });
    let stale_timer = cx.background_executor.clone();
    kagi::ui::e2e::queue_remote_open(cx.background_executor.spawn(async move {
        stale_timer.timer(Duration::from_secs(1)).await;
        Ok(("/stale".to_string(), stale_snapshot))
    }));
    app.update(cx, |app, cx| app.start_remote_open_repo(cx));
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            kagi::ui::e2e::set_remote_browse_host_input(app, "fresh-instance", window, cx);
        });
    })
    .unwrap();
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.remote_browse()
                .is_some_and(|modal| modal.host_input == "fresh-instance"),
            "remote-browse-generation-match-only: stale completion must not mutate or close the reopened instance"
        );
        assert!(
            app.remote_view.is_none(),
            "remote-browse-stale-open-dropped: stale completion must not enter its repository"
        );
    });
    app.update(cx, |app, _| app.cancel_remote_browse());

    app.update(cx, |app, cx| {
        app.open_remote_browse(cx);
        kagi::ui::e2e::prepare_remote_browse_open(app, host, "/srv/repo");
    });
    kagi::ui::e2e::queue_remote_open(
        cx.background_executor
            .spawn(async move { Ok(("/srv/repo".to_string(), remote_snapshot)) }),
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.remote_browse().is_none() && app.remote_view.is_some(),
            "welcome-modal-enter-confirms-browse: Enter on Welcome must open the browsed repository"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_browse_modal_routing");
}

fn fake_update_offer() -> (
    kagi_domain::update::UpdatePlan,
    kagi_domain::update::ReleaseInfo,
) {
    use kagi_domain::update::{Asset, ReleaseInfo, UpdatePlan, Version};

    let current = Version::parse("1.0.0").unwrap();
    let latest = Version::parse("1.0.1").unwrap();
    let asset = Asset {
        name: "Kagi-test.dmg".to_string(),
        url: "https://example.invalid/Kagi-test.dmg".to_string(),
        size: 1,
    };
    (
        UpdatePlan {
            current,
            latest: latest.clone(),
            tag: "v1.0.1".to_string(),
            notes: "test update".to_string(),
            asset: asset.clone(),
        },
        ReleaseInfo {
            tag: "v1.0.1".to_string(),
            version: latest,
            notes: "test update".to_string(),
            assets: vec![asset],
        },
    )
}

fn drawn_active_modals(window: AnyWindowHandle) -> Vec<&'static str> {
    [
        ("remote", "active-modal/remote-browse"),
        ("update", "active-modal/update"),
        ("notice", "active-modal/app-notice"),
    ]
    .into_iter()
    .filter_map(|(label, control)| {
        kagi::ui::e2e::control_bounds(window.window_id(), control).map(|_| label)
    })
    .collect()
}

fn repaint_active_modals(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for control in [
        "active-modal/remote-browse",
        "active-modal/update",
        "active-modal/app-notice",
    ] {
        kagi::ui::e2e::clear_control_bounds(window.window_id(), control);
    }
    paint(cx, window);
}

/// #643: window-global modals still share one slot. An arriving AppNotice waits
/// behind the front modal; Enter targets that front modal, and Update consumes
/// Enter without an action while Esc closes it.
pub fn scenario_window_modal_exclusivity(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = output(repo, &["rev-parse", "HEAD"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, _| app.select_headless(1));

    app.update(cx, |app, cx| {
        app.open_remote_browse(cx);
        kagi::ui::e2e::deliver_app_notice(app, "queued behind Remote Browse");
    });
    repaint_active_modals(cx, window);
    assert_eq!(
        drawn_active_modals(window),
        vec!["remote"],
        "single-modal-render-remote-front: an arriving AppNotice must not coexist on screen"
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .remote_browse()
            .is_some_and(|modal| modal.error.is_some())),
        "single-modal-enter-targets-remote-front: queued notice must not steal Enter"
    );
    assert_eq!(output(repo, &["rev-parse", "HEAD"]), head);
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    app.update(cx, |app, _| {
        app.update_available = Some(fake_update_offer());
        app.open_update_modal();
        kagi::ui::e2e::deliver_app_notice(app, "queued behind Update");
    });
    repaint_active_modals(cx, window);
    assert_eq!(
        drawn_active_modals(window),
        vec!["update"],
        "single-modal-render-update-front: Update and AppNotice must not coexist on screen"
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.update_modal().is_some(),
            "update-enter-consumed: Enter must leave the view-only modal open"
        );
        assert!(!app.update_installing);
        assert!(app.update_status.is_none());
    });
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        head,
        "update-enter-preserves-head: consumed Enter must not checkout the selected commit"
    );

    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).update_modal().is_none()),
        "update-escape-closes-front: Esc must close Update"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS window_modal_exclusivity");
}

/// #718: modal displacement preserves every unread AppNotice, while explicit
/// user dismissal requeues only actionable notices. Remote Browse and Update
/// exercise unrelated setters; the final plain notice distinguishes the events.
pub fn scenario_app_notice_modal_replacement(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());

    app.update(cx, |app, cx| {
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "ack after remote"),
            "notice-replacement-seeds-remote-ack: fixture must create an acknowledge action"
        );
        app.open_remote_browse(cx);
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    assert!(
        cx.read(|cx| kagi::ui::e2e::app_notice_is_acknowledgeable(app.read(cx))),
        "notice-replacement-remote-represents-action: closing Remote Browse must re-present the displaced Acknowledge"
    );
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "notice-replacement-remote-ack-executes: the re-presented Acknowledge must clear its reconcile requirement"
        );
    });

    app.update(cx, |app, _| {
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "ack after update"),
            "notice-replacement-seeds-update-ack: fixture must create an acknowledge action"
        );
        app.update_available = Some(fake_update_offer());
        app.open_update_modal();
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    assert!(
        cx.read(|cx| kagi::ui::e2e::app_notice_is_acknowledgeable(app.read(cx))),
        "notice-replacement-update-represents-action: closing Update must re-present the displaced Acknowledge"
    );
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_sessions.reconcile_ids().is_empty()),
        "notice-replacement-update-ack-executes: the second setter must preserve the same executable action"
    );

    app.update(cx, |app, cx| {
        kagi::ui::e2e::deliver_app_notice(app, "plain information");
        app.open_remote_browse(cx);
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    cx.read(|cx| {
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app.read(cx)),
            Some("plain information"),
            "notice-replacement-plain-is-retained: a displaced non-actionable notice must be requeued"
        );
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "notice-dismissal-plain-is-discarded: a user-dismissed non-actionable notice must not be requeued"
    );

    app.update(cx, |app, cx| {
        kagi::ui::e2e::deliver_app_notice(app, "arrival A");
        kagi::ui::e2e::deliver_app_notice(app, "arrival B");
        app.open_remote_browse(cx);
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    cx.read(|cx| {
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app.read(cx)),
            Some("arrival A"),
            "notice-displacement-preserves-arrival-order-a: the displaced older notice must return before queued arrivals"
        );
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    cx.read(|cx| {
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app.read(cx)),
            Some("arrival B"),
            "notice-displacement-preserves-arrival-order-b: the later waiting notice must follow the displaced notice"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS app_notice_modal_replacement");
}
/// #643 S3c: generation state and completion belong to the initiating session.
/// Switching cannot paint them onto another tab, and closing the owner drops the
/// result rather than delivering it to a same-path reopen.
pub fn scenario_smart_commit_generation_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("canonical A");
    let repo_b = fixture_b.path().canonicalize().expect("canonical B");
    let (app, window) = mount(cx, &repo_a);
    let session_a = cx.read(|cx| app.read(cx).active_session().expect("A session"));

    app.update(cx, |app, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
        app.smart_commit.llm_enabled = true;
        app.smart_commit.provider =
            kagi::ui::smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::Codex);
    });
    kagi::ui::e2e::queue_smart_generation(
        cx.background_executor
            .spawn(async { Some(("generated for A".to_string(), true)) }),
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.smart_generate(session_a, window, cx));
    })
    .expect("start A generation");
    assert!(
        !kagi::ui::e2e::smart_generation_queued(),
        "smart-owner-seam-consumed: A's generation must run the queued task, not the provider CLI"
    );
    assert!(
        cx.read(|cx| app.read(cx).ui().smart_commit_generating),
        "smart-owner-starts-on-a: the initiating session must own the spinner"
    );

    let session_b = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.active_session().expect("B session")
    });
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !app.ui().smart_commit_generating && app.ui().smart_commit_status.is_none(),
            "smart-owner-b-never-shows-a: B must not inherit A's spinner or status"
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(session_b));
        assert!(
            !app.ui().smart_commit_generating && app.ui().smart_commit_status.is_none(),
            "smart-owner-completion-stays-off-b: A's completion must not update active B"
        );
    });

    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(session_a));
        assert!(
            !app.ui().smart_commit_generating
                && app.ui().smart_commit_status.as_deref()
                    == Some(kagi_ui_core::i18n::Msg::SmartGeneratedLocal.t()),
            "smart-owner-a-restores-result: A must retain its own completed status"
        );
    });

    app.update(cx, |app, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
    });
    kagi::ui::e2e::queue_smart_generation(
        cx.background_executor
            .spawn(async { Some(("detached result".to_string(), true)) }),
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.smart_generate(session_a, window, cx));
    })
    .expect("start detached generation");
    assert!(
        !kagi::ui::e2e::smart_generation_queued(),
        "smart-detached-seam-consumed: the detached generation must run the queued task, not the provider CLI"
    );
    app.update(cx, |app, cx| {
        let index = app
            .tabs
            .iter()
            .position(|tab| tab.session == session_a)
            .expect("A tab");
        app.close_tab(index, cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(session_b));
        assert!(
            !app.ui().smart_commit_generating && app.ui().smart_commit_status.is_none(),
            "smart-detached-completion-stays-off-b: a detached owner's result must not fall through to the active tab"
        );
    });
    let reopened = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_a.clone(), cx), "reopen A");
        app.active_session().expect("reopened A session")
    });
    assert_ne!(
        reopened, session_a,
        "smart-detached-reopen-has-new-owner: reopening must mint a fresh session"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !app.ui().smart_commit_generating && app.ui().smart_commit_status.is_none(),
            "smart-detached-completion-is-dropped: a same-path reopen must not inherit the closed owner's result"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS smart_commit_generation_owner");
}

/// #643 S3c: capability detection is process/window-global, while Smart Commit
/// presentation uses the same ActiveModal slot and key routing as every modal.
pub fn scenario_smart_commit_modal_and_probe(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["smart_commit_model"]);
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("canonical A");
    let repo_b = fixture_b.path().canonicalize().expect("canonical B");
    let (app, window) = mount(cx, &repo_a);

    app.update(cx, |app, cx| {
        app.smart_commit_probe_revision = 7;
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo_a.clone(), cx);
        app.select_headless(1);
        kagi::ui::e2e::seed_modal_list_scroll(app, 3);
        assert_eq!(
            kagi::ui::e2e::modal_list_scroll_top(app),
            3,
            "modal-list-scroll-seed"
        );
        app.set_smart_commit_modal(kagi::ui::smart_commit::SmartCommitModal::ModelPicker {
            models: vec!["fixture".to_string()],
        });
        assert_eq!(
            kagi::ui::e2e::modal_list_scroll_top(app),
            0,
            "modal-list-scroll-resets-on-replacement: each modal must start at the top"
        );
    });
    kagi::ui::e2e::queue_smart_generation(
        cx.background_executor
            .spawn(async { Some(("fixture draft".to_string(), true)) }),
    );
    paint(cx, window);
    paint(cx, window);
    cx.simulate_keystrokes(window, "enter");
    cx.simulate_event(
        window,
        gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse("enter").unwrap(),
        },
    );
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.smart_commit_modal().is_none()
                && app.plan_modal().is_none()
                && app.smart_commit.model.as_deref() == Some("fixture")
        }),
        "smart-modal-enter-does-not-checkout: Enter confirms the model, never the graph selection"
    );
    app.update(cx, |app, _| {
        app.set_smart_commit_modal(kagi::ui::smart_commit::SmartCommitModal::ModelPicker {
            models: vec!["fixture".to_string()],
        });
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
        "smart-modal-escape-closes-slot: Esc must close Smart Commit"
    );

    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo_b, cx);
        kagi::ui::e2e::ensure_smart_commit_detection(app, cx);
    });
    assert_eq!(
        cx.read(|cx| app.read(cx).smart_commit_probe_revision),
        7,
        "smart-probe-is-global: opening a different repository must not rerun capability detection"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS smart_commit_modal_and_probe");
}

/// #718: the update installer is window-owned operation state. Closing or
/// replacing its presentation cannot erase progress, allow a duplicate start,
/// or hide a later failure; the same modal remains cancellable on Welcome.
pub fn scenario_update_install_lifecycle(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());

    app.update(cx, |app, _| {
        app.update_available = Some(fake_update_offer());
        app.open_update_modal();
        assert!(
            kagi::ui::e2e::begin_update_install_for_test(app),
            "update-install-first-start: the idle window operation must start"
        );
        app.cancel_update_modal();
        app.open_update_modal();
        assert!(
            !kagi::ui::e2e::begin_update_install_for_test(app),
            "update-install-no-double-start: reopening the modal must not start a second installer"
        );
        app.cancel_update_modal();
        kagi::ui::e2e::fail_update_install_for_test(app, "simulated failure");
        app.open_update_modal();
    });
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "update/status");
    paint(cx, window);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !app.update_installing
                && app
                    .update_status
                    .as_deref()
                    .is_some_and(|status| status.contains("simulated failure")),
            "update-install-completion-outlives-modal: completion must update window state after the modal closes"
        );
    });
    assert!(
        kagi::ui::e2e::control_bounds(window.window_id(), "update/status").is_some(),
        "update-install-result-visible-after-reopen: reopening must render the retained completion status"
    );

    app.update(cx, |app, cx| app.close_tab(0, cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.tabs.is_empty() && app.update_modal().is_some()
        }),
        "update-welcome-keeps-window-modal: closing the last tab must retain Update on Welcome"
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).update_modal().is_some()),
        "update-welcome-enter-consumed: Enter on Welcome must route to and retain view-only Update"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).update_modal().is_none()),
        "update-welcome-escape-closes-modal: Esc on Welcome must close the single modal slot"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS update_install_lifecycle");
}

/// #564: a branch context-menu overlay is not a confirmation modal. Enter
/// must neither open a checkout plan for the selected commit behind it nor
/// move HEAD. The registered `CheckoutSelected` action is also dispatched
/// directly because it bypasses raw-key confirmation routing and reaches
/// `checkout_selected_commit` itself.
pub fn scenario_branch_menu_no_checkout_fallthrough(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let head = output(repo, &["rev-parse", "HEAD"]);
    git(repo, &["branch", "menu-target", "HEAD~1"]);
    let (app, window) = mount(cx, repo);

    app.update(cx, |app, _| app.select_headless(1));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(1));

    // Positive control: with no overlay, the registered checkout action opens
    // the expected plan for the selected non-HEAD commit. The no-plan oracle
    // below applies only after the branch menu is opened.
    dispatch_checkout_selected(cx, &app, window);
    cx.run_until_parked();
    assert!(cx.read(|cx| matches!(app.read(cx).active_modal, Some(ActiveModal::Checkout(_)))));
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));

    app.update(cx, |app, _| {
        app.open_local_branch_menu(
            "menu-target".to_string(),
            gpui::point(gpui::px(0.0), gpui::px(0.0)),
        );
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.branch_menu.is_some(), "branch menu must be open");
        assert_eq!(
            app.ui().selected,
            Some(1),
            "the menu must cover a non-HEAD row"
        );
    });

    // The action path proves the fallback itself has the overlay guard.
    dispatch_checkout_selected(cx, &app, window);
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).active_modal.is_none()));

    // The actual user path must have the same result.
    press_enter(cx, &app, window);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.branch_menu.is_some(), "Enter must leave the menu open");
        assert!(
            app.active_modal.is_none(),
            "menu + Enter must not plan checkout for the selected commit"
        );
    });
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        head,
        "menu + Enter must not move HEAD"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS branch_menu_no_checkout_fallthrough: menu blocks selected-commit checkout"
    );
}

/// #718 P1: a failed push is a terminal outcome, not permission to replace
/// whichever foreground modal the user opened while the write ran.
///
/// ADR-0196 §3 (as amended by #747): a recorded failure reaches the user as
/// the Operation Log receipt plus a short toast / footer — never as a
/// dismiss-only `AppNotice`, neither over Remote Browse nor queued behind it.
/// (The scenario used to assert the queued notice, and has failed since #747
/// removed it; #824.)
///
/// The failure is offline and deterministic: push to a bare remote, delete the
/// remote, then commit — the upstream ref still resolves (so the plan is clean
/// and never touches the network) but `git push` cannot find the repository.
pub fn scenario_push_failure_keeps_modal(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_dir = tempfile::tempdir().expect("remote tempdir");
    let bare = remote_dir.path().join("target.git");
    let original_language = kagi::ui::i18n::lang();
    kagi::ui::i18n::set_lang(kagi::ui::i18n::Lang::En);
    let bare_str = bare.to_str().unwrap();
    git(repo, &["init", "--bare", "-q", bare_str]);
    git(repo, &["remote", "add", "origin", bare_str]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::remove_dir_all(&bare).unwrap();
    git(repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    let before = repo_fingerprint(repo);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_push_modal(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .push_modal()
            .is_some_and(|m| m.plan.blockers.is_empty())),
        "the fixture must produce a clean push plan for the failure to be the executor's"
    );

    app.update(cx, |app, cx| app.start_push(cx));
    app.update(cx, |app, cx| app.open_remote_browse(cx));
    paint(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            kagi::ui::e2e::set_remote_browse_host_input(app, "typed-host", window, cx);
        });
    })
    .unwrap();
    wait_idle(cx, &app);
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.remote_browse()
                .is_some_and(|modal| modal.host_input == "typed-host"),
            "push-failure-preserves-remote-input: terminal completion must not replace foreground input"
        );
        assert!(
            app.write_busy_op.is_none(),
            "busy must be released on failure"
        );
        assert!(
            matches!(&app.status_footer, FooterStatus::Failed(text) if text.contains("push")),
            "push-failure-reaches-footer: got {:?}",
            app.status_footer
        );
        let toasts = app.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert!(
            toasts
                .iter()
                .any(|t| t.kind == ToastKind::Error && t.message.contains("push")),
            "push-failure-toasts: the failure must be announced by a toast behind the modal, got {toasts:?}"
        );
        assert!(
            !kagi::ui::e2e::queued_notice_contains(app, "push"),
            "push-failure-no-queued-notice: a recorded failure must not queue a dismiss-only notice"
        );
    });
    app.update(cx, |app, _| {
        app.cancel_remote_browse();
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "push-failure-no-notice-after-close: closing the foreground must not reveal a dismiss-only modal"
    );
    let durable = records(repo, "push");
    assert_eq!(
        durable.len(),
        1,
        "the push attempt must have one durable receipt"
    );
    let failed = durable
        .iter()
        .find(|e| matches!(e.outcome, OpOutcome::Failed { .. }))
        .expect("the failure must also be durable in the oplog");
    // ADR-0196 Wave 2: the panel shows the backend's receipt itself, not a
    // copy the UI re-synthesized from the error text.
    cx.read(|cx| {
        let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
        // This repository's entries: the panel lists the shared log, which
        // earlier scenarios' pushes are in too (#516).
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|e| e.op == "push" && e.repo == failed.repo)
            .collect();
        assert_eq!(shown.len(), 1, "the panel shows the receipt once");
        assert!(
            failed.failure_code.is_some(),
            "the backend records a typed failure code"
        );
        assert_eq!(
            shown[0].failure_code, failed.failure_code,
            "the presented entry keeps the backend's failure code"
        );
    });
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "a failed push must not touch the repository"
    );

    kagi::ui::i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS push_failure_keeps_modal: Remote Browse wins; failure recorded, toasted, no notice"
    );
}

/// ADR-0196 Wave 2 (#643 A1): the checkout family presents the backend's own
/// receipt. A preflight refusal is recorded by `run_recorded` with
/// `failure_code: preflight`; the panel entry must be *that* entry, not one the
/// UI re-synthesized from the stringified error (which carried no code at all).
/// Moving HEAD between plan and execute is the offline way to make preflight
/// refuse.
pub fn scenario_checkout_presents_backend_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "feature/one", "HEAD~1"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_plan_modal("feature/one", cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .plan_modal()
            .is_some_and(|m| m.plan.blockers.is_empty())),
        "the fixture must produce a clean checkout plan so the refusal is preflight's"
    );
    git(
        repo,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "moved under the plan",
        ],
    );
    let head = output(repo, &["rev-parse", "HEAD"]);

    app.update(cx, |app, cx| app.start_checkout(cx));
    wait_idle(cx, &app);
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        head,
        "preflight must refuse the stale plan"
    );
    let durable = records(repo, "checkout");
    assert_eq!(durable.len(), 1, "one attempt, one durable entry");
    assert!(matches!(durable[0].outcome, OpOutcome::Failed { .. }));
    assert_eq!(durable[0].failure_code, Some(FailureCode::Preflight));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.plan_modal().is_none(),
            "Failed must not restore the consumed checkout confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "a recorded checkout failure must not open a dismiss-only modal"
        );
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|e| e.op == "checkout")
            .collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, durable[0].id,
            "the panel entry is the durable receipt"
        );
        assert_eq!(
            shown[0].failure_code,
            Some(FailureCode::Preflight),
            "the presented entry keeps the backend's failure code"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS checkout_presents_backend_receipt: panel shows the recorded receipt");
}

/// ADR-0196 Wave 2: the families routed through `finish_recorded` present the
/// backend's own receipt. Cherry-pick stands in for the seven (cherry-pick,
/// revert, checkout-tracking, switch-to-latest, set-upstream, rename-branch,
/// delete-remote-branch) — they share the one helper, so the assertion is on
/// the helper: the panel entry is the durable entry and keeps `failure_code`.
pub fn scenario_cherry_pick_presents_backend_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    std::fs::write(repo.join("side.txt"), "picked\n").unwrap();
    git(repo, &["add", "side.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    let pick = output(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "-q", "main"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, _| app.open_cherry_pick_modal(CommitId(pick)));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .cherry_pick_modal()
            .is_some_and(|m| m.plan.blockers.is_empty())),
        "the fixture must produce a clean cherry-pick plan so the refusal is preflight's"
    );
    git(
        repo,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "moved under the plan",
        ],
    );
    let head = output(repo, &["rev-parse", "HEAD"]);

    app.update(cx, |app, cx| app.start_cherry_pick(cx));
    wait_idle(cx, &app);
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        head,
        "preflight must refuse the stale plan"
    );
    let durable = records(repo, "cherry-pick");
    assert_eq!(durable.len(), 1, "one attempt, one durable entry");
    assert_eq!(durable[0].failure_code, Some(FailureCode::Preflight));
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.cherry_pick_modal().is_none(),
            "Failed must not restore the consumed cherry-pick confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "a recorded cherry-pick failure must not open a dismiss-only modal"
        );
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|e| e.op == "cherry-pick")
            .collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, durable[0].id,
            "the panel entry is the durable receipt"
        );
        assert_eq!(shown[0].failure_code, Some(FailureCode::Preflight));
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS cherry_pick_presents_backend_receipt: panel shows the recorded receipt"
    );
}

/// #1092: deliver every post-dismissal key where the product left focus.
/// Keeping the old InputState alive makes hidden-input focus observable.
fn assert_created_branch_dismissed(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
) {
    wait_painted(cx, app, window, |app| {
        app.view().branch_targets.contains_key("feat")
    });
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    assert!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window)
        })
        .unwrap(),
        "verified creation must return focus from the removed input to the root"
    );
    let receipt = records(repo, "create-branch");
    assert_eq!(receipt.len(), 1);
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    paint(cx, window);
    let after_enter = records(repo, "create-branch");
    assert_eq!(
        after_enter.len(),
        1,
        "second Enter must not attempt creation again"
    );
    assert_eq!(after_enter[0].id, receipt[0].id);
    cx.simulate_keystrokes(window, "cmd-p");
    cx.run_until_parked();
    paint(cx, window);
    assert!(
        cx.read(|cx| matches!(
            app.read(cx).menu_overlay,
            Some(kagi::ui::commands::MenuOverlay::CommandPalette)
        )),
        "the next root shortcut must open the palette without test-side refocusing"
    );
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
}

/// ADR-0196 Wave 2: the synchronous inline sites (create-branch, create-tag,
/// the auto-stash before a checkout, the continued-merge commit) present the
/// receipt through `present_report`. Create-branch stands in for the four.
/// The durable log is seeded first so the receipt's id differs from the id 0
/// a UI-synthesized entry carries — on a fresh log both would be 0.
pub fn scenario_create_branch_presents_backend_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let original_branch = output(&repo, &["symbolic-ref", "--short", "HEAD"]);
    let seed = OpLogEntry::new(
        "seed",
        repo.display().to_string(),
        StateSummary {
            head: "seed".to_string(),
            dirty: "clean".to_string(),
        },
        OpOutcome::Failed {
            error: "seed".to_string(),
        },
    );
    append_oplog(&seed).expect("seed the durable log");
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
    paint(cx, window);
    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .and_then(|m| m.input_state.clone())
        })
        .expect("the first paint creates the branch-name input");
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "f e a t");
    cx.run_until_parked();
    wait_painted(cx, &app, window, |app| {
        app.create_branch_modal()
            .and_then(|m| m.plan.plan())
            .is_some_and(|plan| plan.blockers.is_empty())
    });

    // Ordinary accepted reloads preserve unsent text and the same live input.
    git(&repo, &["branch", "reload-marker"]);
    app.update(cx, |app, cx| app.reload(cx));
    wait_painted(cx, &app, window, |app| {
        app.view().branch_targets.contains_key("reload-marker")
    });
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .create_branch_modal()
            .expect("unsent form survives reload");
        assert_eq!(modal.input, "feat");
        assert_eq!(
            modal.input_state.as_ref().unwrap().entity_id(),
            input.entity_id()
        );
        assert_eq!(input.read(cx).value().as_str(), "feat");
        assert!(!modal.checkout_after);
        assert!(modal.error.is_none());
    });
    assert!(
        records(&repo, "create-branch").is_empty(),
        "reload never submits unsent input"
    );

    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(
        !output(&repo, &["branch", "--list", "feat"]).is_empty(),
        "Enter on a clean plan creates the branch"
    );
    assert_eq!(
        output(&repo, &["symbolic-ref", "--short", "HEAD"]),
        original_branch,
        "unchecked checkout leaves HEAD on the original branch"
    );
    assert_created_branch_dismissed(cx, &app, window, &repo);
    let durable = records(&repo, "create-branch");
    assert_eq!(durable.len(), 1, "one attempt, one durable entry");
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    assert!(
        durable[0].id >= 1,
        "the seeded log puts the receipt past entry 0"
    );
    cx.read(|cx| {
        let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|e| e.op == "create-branch")
            .collect();
        assert_eq!(shown.len(), 1, "the panel shows the receipt once");
        assert_eq!(
            shown[0].id, durable[0].id,
            "the panel entry is the durable receipt, not a UI copy"
        );
    });

    drop(input);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS create_branch_presents_backend_receipt: panel shows the recorded receipt"
    );
}

/// #1092: a valid ref name can still fail at the real execution boundary:
/// an existing `collision` ref prevents creation of `collision/child`.
pub fn scenario_create_branch_execution_failure_keeps_input(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "collision"]);
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let before = repo_fingerprint(&repo);
    let refs_before = output(&repo, &["show-ref", "--heads"]);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
    paint(cx, window);
    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .unwrap()
                .input_state
                .clone()
        })
        .expect("branch input is mounted");
    cx.simulate_keystrokes(window, "c o l l i s i o n / c h i l d");
    wait_painted(cx, &app, window, |app| {
        app.create_branch_modal()
            .and_then(|modal| modal.plan.plan())
            .is_some_and(|plan| plan.blockers.is_empty())
    });
    let confirm = kagi::ui::e2e::confirm_bounds(window.window_id())
        .expect("valid collision/child plan offers Create");
    cx.simulate_click(window, confirm.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    paint(cx, window);
    let durable = records(&repo, "create-branch");
    assert_eq!(
        durable.len(),
        1,
        "the failed execution has exactly one receipt"
    );
    assert!(
        matches!(durable[0].outcome, OpOutcome::Failed { .. }),
        "namespace collision must fail execution, not planning: {:?}",
        durable[0].outcome
    );
    let error = cx.read(|cx| {
        let app = app.read(cx);
        let modal = app.create_branch_modal().expect("failed form stays open");
        assert_eq!(modal.input, "collision/child");
        assert_eq!(input.read(cx).value().as_str(), "collision/child");
        let error = modal.error.clone().expect("execution error is inspectable");
        assert!(app.app_notice().is_none());
        let shown = app.op_log.as_ref().unwrap().read(cx).entries();
        let shown: Vec<_> = shown
            .iter()
            .filter(|entry| entry.op == "create-branch" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1);
        assert_eq!(
            shown[0].id, durable[0].id,
            "failure presents the backend receipt"
        );
        error
    });
    assert_eq!(repo_fingerprint(&repo), before);
    assert_eq!(output(&repo, &["show-ref", "--heads"]), refs_before);
    assert!(output(&repo, &["branch", "--list", "collision/child"]).is_empty());

    git(&repo, &["branch", "reload-marker"]);
    app.update(cx, |app, cx| app.reload(cx));
    wait_painted(cx, &app, window, |app| {
        app.view().branch_targets.contains_key("reload-marker")
    });
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .create_branch_modal()
            .expect("failed form survives reload");
        assert_eq!(modal.input, "collision/child");
        assert_eq!(
            modal.error.as_ref(),
            Some(&error),
            "reload retains the exact error"
        );
        assert_eq!(
            modal.input_state.as_ref().unwrap().entity_id(),
            input.entity_id()
        );
        assert_eq!(input.read(cx).value().as_str(), "collision/child");
    });
    assert_eq!(
        records(&repo, "create-branch").len(),
        1,
        "reload never retries"
    );
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    paint(cx, window);
    assert!(cx.read(|cx| app.read(cx).create_branch_modal().is_none()));
    drop(input);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS create_branch_execution_failure_keeps_input");
}

/// ADR-0196 Wave 3 / #501: a write that happened but could not be recorded is
/// presented as "changed but not recorded"; a family's own success footer must
/// not paper over it. Delete-branch stands in for every `finish_run` family
/// that sets a success footer. The oplog lock is held so the append fails.
pub fn scenario_run_success_unrecorded_keeps_partial_footer(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    git(&repo, &["branch", "merged-delete"]);
    let (app, window) = mount(cx, &repo);
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    app.update(cx, |app, cx| {
        app.open_delete_branch_modal("merged-delete", cx)
    });
    wait_idle(cx, &app);
    app.update(cx, |app, cx| app.start_delete_branch(cx));
    wait_idle(cx, &app);
    assert_eq!(
        output(&repo, &["branch", "--list", "merged-delete"]),
        "",
        "the write itself went through"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown = panel
            .entries()
            .iter()
            .find(|e| e.op == "delete-branch")
            .expect("the attempted receipt is shown");
        assert!(
            matches!(shown.outcome, OpOutcome::Partial { .. }),
            "unrecorded success is presented as partial: {:?}",
            shown.outcome
        );
        match &app.status_footer {
            FooterStatus::Success(text) => panic!("success footer hid the missing record: {text}"),
            FooterStatus::Failed(text) | FooterStatus::Idle(text) => assert!(
                text.contains("not recorded"),
                "the footer must say the record is missing: {text}"
            ),
            other => panic!("unexpected footer: {other:?}"),
        }
    });

    drop(lock);
    let _ = std::fs::remove_file(&lock_path);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS run_success_unrecorded_keeps_partial_footer");
}

pub(super) fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .unwrap();
}
/// Native layout rounds fractional scaled pixels to half-pixel boundaries.
fn assert_modal_button_height(actual: gpui::Pixels, name: &str) {
    assert!(
        f32::from(actual - theme::scaled_px(24.)).abs() <= 0.5,
        "{name} must be a scaled 24px control, got {actual:?}"
    );
}

/// Paint, advance the test clock, and park until `predicate` holds.
///
/// Three things have to happen for a live replan to settle, and none implies
/// the others: `sync_modal_inputs` copies the real text input into the modal
/// only on a **paint**, that copy schedules the replan on a 250 ms
/// **background timer**, and `VisualTestAppContext::run_until_parked` ticks the
/// scheduler *without advancing the test clock* — only `advance_clock` fires a
/// pending timer. `stash_replan_error` needs none of this because the stash plan
/// is dispatched immediately rather than debounced.
fn wait_painted(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    predicate: impl Fn(&KagiApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        paint(cx, window);
        cx.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        if cx.read(|cx| predicate(app.read(cx))) {
            return;
        }
        assert!(Instant::now() < deadline, "the modal plan did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// #956: a visible disabled Create remains inert with an empty field. Enter
/// while the real input owns marked IME text must accept composition, not run
/// the ready branch plan; ordinary Enter after unmarking still confirms.
pub fn scenario_create_branch_input_confirm_ime(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        app.open_create_branch_modal(head, cx);
        app.create_branch_modal_mut().unwrap().checkout_after = true;
    });
    paint(cx, window);
    wait_painted(cx, &app, window, |app| {
        app.create_branch_modal()
            .and_then(|modal| modal.plan.plan())
            .is_some_and(|plan| !plan.blockers.is_empty())
    });
    kagi::ui::button_style::clear_recorded_modal_buttons();
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-recovery");
    app.update(cx, |_, cx| cx.notify());
    paint(cx, window);
    assert!(
        kagi::ui::e2e::control_bounds(window.window_id(), "input-recovery").is_none(),
        "empty branch name must not render a recovery line"
    );
    let disabled = kagi::ui::e2e::confirm_bounds(window.window_id())
        .expect("empty branch name still shows a disabled Create");
    let blocked_cancel = kagi::ui::e2e::control_bounds(window.window_id(), "create-branch-cancel")
        .expect("blocked branch card still shows Cancel");
    assert_modal_button_height(disabled.size.height, "blocked Create");
    assert_modal_button_height(blocked_cancel.size.height, "Cancel");
    let expected_reason = cx.read(|cx| {
        let app = app.read(cx);
        let blocker = app
            .create_branch_modal()
            .and_then(|modal| modal.plan.plan())
            .and_then(|plan| plan.blockers.first())
            .expect("empty branch name has a blocker");
        kagi_ui_core::i18n::plan_note_text(blocker)
    });
    assert_eq!(
        kagi::ui::button_style::recorded_modal_button("create-branch-confirm"),
        Some(kagi::ui::button_style::ModalButtonA11y {
            role: gpui::Role::Button,
            label: kagi_ui_core::i18n::Msg::InputCreate.t().to_owned(),
            description: Some(expected_reason),
            disabled: true,
        }),
        "blocked Create is an AX-disabled button with the specific reason as aria_description"
    );
    cx.simulate_mouse_move(window, disabled.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, disabled.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).create_branch_modal().is_some()),
        "clicking disabled Create keeps the form open"
    );

    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .and_then(|modal| modal.input_state.clone())
        })
        .expect("first paint creates branch-name input");
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "f e a t");
    wait_painted(cx, &app, window, |app| {
        app.create_branch_modal()
            .and_then(|modal| modal.plan.plan())
            .is_some_and(|plan| plan.blockers.is_empty())
    });
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-recovery");
    paint(cx, window);
    assert!(
        kagi::ui::e2e::control_bounds(window.window_id(), "input-recovery").is_none(),
        "Create Branch omits recovery even after the plan is ready"
    );
    let current = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-current")
        .expect("shared pane renders CURRENT");
    let after = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-after")
        .expect("shared pane renders AFTER");
    let comparison = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-comparison")
        .expect("shared comparison");
    assert!(
        current.bottom() <= after.top() + gpui::px(1.)
            && f32::from(current.left() - after.left()).abs() <= 2.
            && f32::from(current.size.width - after.size.width).abs() <= 2.,
        "both full-width rows must stack and align: {current:?}, {after:?}"
    );
    assert!(current.top() >= comparison.top() && after.bottom() <= comparison.bottom());
    assert!(
        kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-arrow").is_none(),
        "the horizontal transition arrow must be absent"
    );
    for (side, prefix) in [("current", "CURRENT: "), ("after", "AFTER: ")] {
        let (role, label) = kagi::ui::dialog_a11y::recorded_note(&format!("plan-state-{side}"))
            .expect("plan row AX label");
        assert_eq!(role, gpui::Role::Group);
        assert!(label.starts_with(prefix), "{side} AX name: {label}");
    }
    for side in ["current", "after"] {
        let label =
            kagi::ui::e2e::control_bounds(window.window_id(), &format!("plan-state-{side}-label"))
                .expect("fixed-width row label");
        let head =
            kagi::ui::e2e::control_bounds(window.window_id(), &format!("plan-state-{side}-head"))
                .expect("mono row head");
        assert!(
            f32::from(label.size.width - theme::scaled_px(64.)).abs() <= 1.
                && label.size.height <= theme::scaled_px(24.)
                && label.right() <= head.left()
                && head.right() <= comparison.right(),
            "{side} label is one line before its contained head: {label:?}, {head:?}"
        );
    }
    let card = kagi::ui::e2e::control_bounds(window.window_id(), "modal-card")
        .expect("shared plan card measured");
    let cancel = kagi::ui::e2e::control_bounds(window.window_id(), "create-branch-cancel")
        .expect("Create Branch Cancel remains available");
    let confirm =
        kagi::ui::e2e::confirm_bounds(window.window_id()).expect("Create remains available");
    assert_modal_button_height(cancel.size.height, "ready Cancel");
    assert_modal_button_height(confirm.size.height, "ready Create");
    kagi::ui::button_style::clear_recorded_modal_buttons();
    app.update(cx, |_, cx| cx.notify());
    paint(cx, window);
    assert_eq!(
        kagi::ui::button_style::recorded_modal_button("create-branch-confirm"),
        Some(kagi::ui::button_style::ModalButtonA11y {
            role: gpui::Role::Button,
            label: kagi_ui_core::i18n::Msg::InputCreate.t().to_owned(),
            description: None,
            disabled: false,
        }),
        "ready Create becomes an active button with no blocker description"
    );
    for (label, button) in [("Cancel", cancel), ("Create", confirm)] {
        assert!(
            button.top() >= card.top() && button.bottom() <= card.bottom(),
            "{label} must remain inside the fixed card footer: {card:?}, {button:?}"
        );
    }
    assert!(
        current.bottom() < cancel.top()
            && after.bottom() < confirm.top()
            && f32::from(cancel.center().y - confirm.center().y).abs() <= 2.,
        "comparison stays above aligned, fixed footer actions: {current:?}, {after:?}, {cancel:?}, {confirm:?}"
    );

    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            gpui::EntityInputHandler::replace_and_mark_text_in_range(
                state,
                Some(0..4),
                "feat",
                Some(4..4),
                window,
                cx,
            );
        });
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).create_branch_modal().is_some()),
        "composition Enter must not close the form"
    );
    assert!(
        output(&repo, &["branch", "--list", "feat"]).is_empty(),
        "composition Enter must not create a Git ref"
    );
    assert!(
        records(&repo, "create-branch").is_empty(),
        "composition Enter must not record any create attempt"
    );
    cx.read(|cx| {
        let modal = app.read(cx).create_branch_modal().unwrap();
        assert_eq!(modal.input, "feat");
        assert!(modal.checkout_after);
        assert!(modal.error.is_none());
    });

    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            gpui::EntityInputHandler::unmark_text(state, window, cx);
        });
    })
    .unwrap();
    // Do not use press_enter: it forcibly focuses root and would conceal a
    // broken input → confirmation → root handoff.
    paint(cx, window);
    cx.simulate_keystrokes(window, "enter");
    cx.run_until_parked();
    assert!(
        !output(&repo, &["branch", "--list", "feat"]).is_empty(),
        "ordinary Enter after composition still creates the branch"
    );
    assert_eq!(output(&repo, &["symbolic-ref", "--short", "HEAD"]), "feat");
    let durable = records(&repo, "create-branch");
    assert_eq!(durable.len(), 1);
    assert!(matches!(durable[0].outcome, OpOutcome::Success { .. }));
    assert_created_branch_dismissed(cx, &app, window, &repo);
    drop(input);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS create_branch_input_confirm_ime");
}

/// #1017: Stash and the shared plan renderer use the same stacked rows, with
/// one-line EN/JA labels and the full long ref available to assistive technology.
pub fn scenario_stash_push_stacked_preview(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous_lang = kagi_ui_core::i18n::lang();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let long_ref = "origin/fix/issue-1007-windows-recovery-commands";
    assert_eq!(long_ref.len(), 47);
    git(&repo, &["branch", "-m", long_ref]);
    std::fs::write(repo.join("README.md"), "modified before stash\n").unwrap();
    std::fs::write(repo.join("not-tracked.txt"), "untracked before stash\n").unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_stash_push_modal(cx));
    wait_painted(cx, &app, window, |app| {
        app.stash_push_modal()
            .and_then(|modal| modal.plan.as_ref())
            .is_some_and(|plan| plan.blockers.is_empty())
    });
    cx.read(|cx| {
        let modal = app.read(cx).stash_push_modal().unwrap();
        let plan = modal.plan.as_ref().unwrap();
        assert!(plan.current.dirty.contains("1 modified"));
        assert!(plan.current.dirty.contains("1 untracked"));
        assert_eq!(plan.predicted.dirty, "clean");
        assert!(
            !plan.warnings.is_empty(),
            "including untracked needs warning"
        );
    });
    for name in ["plan-state-current", "plan-state-after"] {
        kagi::ui::e2e::clear_control_bounds(window.window_id(), name);
    }
    paint(cx, window);
    let current = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-current")
        .expect("current state is visible");
    let after = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-after")
        .expect("after state is visible");
    assert!(
        current.bottom() <= after.top() + gpui::px(1.)
            && f32::from(current.left() - after.left()).abs() <= 2.,
        "Stash current and after must stack and align: {current:?}, {after:?}"
    );
    // 47-character real ref + "branch: " fits without elision at MD width.
    let comparison = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-comparison")
        .expect("Stash comparison");
    let label = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-current-label")
        .expect("CURRENT label");
    let head = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-current-head")
        .expect("mono head");
    assert!(
        label.size.height <= theme::scaled_px(24.)
            && f32::from(label.size.width - theme::scaled_px(64.)).abs() <= 1.
    );
    assert!(head.left() >= label.right() && head.right() <= comparison.right());
    assert!(
        head.size.width >= theme::scaled_px(55. * 7.8),
        "the 55-character mono head needs its full drawn width: {head:?}, panel={comparison:?}"
    );
    let (role, ax) = kagi::ui::dialog_a11y::recorded_note("plan-state-current")
        .expect("CURRENT row is named for assistive technology");
    assert_eq!(role, gpui::Role::Group);
    assert!(
        ax.contains(long_ref),
        "the full head is available in AX: {ax}"
    );
    for (lang, current_name, after_name) in [
        (kagi_ui_core::i18n::Lang::En, "CURRENT: ", "AFTER: "),
        (kagi_ui_core::i18n::Lang::Ja, "現在: ", "実行後: "),
    ] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        redraw(cx, &app, window);
        for (side, prefix) in [("current", current_name), ("after", after_name)] {
            let (role, name) = kagi::ui::dialog_a11y::recorded_note(&format!("plan-state-{side}"))
                .expect("localized plan row");
            assert_eq!(role, gpui::Role::Group);
            assert!(name.starts_with(prefix), "{lang:?} {side}: {name}");
            let label = kagi::ui::e2e::control_bounds(
                window.window_id(),
                &format!("plan-state-{side}-label"),
            )
            .expect("localized one-line label");
            assert!(
                label.size.height <= theme::scaled_px(24.)
                    && f32::from(label.size.width - theme::scaled_px(64.)).abs() <= 1.,
                "{lang:?} {side} label never wraps: {label:?}"
            );
        }
    }
    let action = kagi::ui::e2e::confirm_bounds(window.window_id())
        .expect("Stash confirm action remains visible");
    assert!(
        action.size.height <= theme::scaled_px(24.),
        "Stash action must use compact 24px control rather than the oversized 32px default: {action:?}"
    );
    assert_eq!(
        before,
        repo_fingerprint(&repo),
        "Stash planning is read-only"
    );
    app.update(cx, |app, cx| app.set_lang(previous_lang, cx));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS stash_push_stacked_preview");
}

/// #1022 review: every standard width resolves its window cap in pixels, so
/// the large card (Create Worktree) is never narrower than the medium one
/// (Create Branch), and the medium card draws its full 640px. A percentage
/// cap resolves against the card's own width in GPUI (LG drew 583px).
pub fn scenario_modal_widths_ordered(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let (app, window) = mount(cx, &repo);
    let mut widths = Vec::new();
    for case in ["branch", "worktree"] {
        app.update(cx, |app, cx| match case {
            "branch" => app.open_create_branch_modal(head.clone(), cx),
            _ => app.open_create_worktree_modal(head.clone(), cx),
        });
        kagi::ui::e2e::clear_control_bounds(window.window_id(), "plan-state-comparison");
        wait_painted(cx, &app, window, |app| match case {
            "branch" => app
                .create_branch_modal()
                .and_then(|m| m.plan.plan())
                .is_some(),
            _ => app
                .create_worktree_modal()
                .and_then(|m| m.plan.plan())
                .is_some(),
        });
        paint(cx, window);
        let comparison = kagi::ui::e2e::control_bounds(window.window_id(), "plan-state-comparison")
            .unwrap_or_else(|| panic!("{case}: the comparison is drawn"));
        widths.push(f32::from(comparison.size.width));
        app.update(cx, |app, cx| {
            app.active_modal = None;
            cx.notify();
        });
        paint(cx, window);
    }
    let (md, lg) = (widths[0], widths[1]);
    assert!(
        lg >= md,
        "LG ({lg}px) must not be narrower than MD ({md}px)"
    );
    // MD 640 minus the card's 1px border and 16px padding on each side.
    assert!(
        (md - f32::from(theme::scaled_px(640. - 2. - 32.))).abs() <= 3.,
        "MD draws its full scaled width: comparison {md}px"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS modal_widths_ordered: md={md} lg={lg}");
}

/// #956: each input-confirm renderer must expose an unavailable primary
/// action rather than remove it when its plan cannot run. A real click at the
/// measured button must leave the modal and repository untouched.
pub fn scenario_input_confirm_disabled_cards(cx: &mut VisualTestAppContext) {
    for case in ["tag", "worktree", "stash", "rename", "upstream"] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let before = repo_fingerprint(&repo);
        let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| match case {
            "tag" => app.open_create_tag_modal(head, cx),
            "worktree" => app.open_create_worktree_modal(head, cx),
            "stash" => app.open_stash_push_modal(cx),
            "rename" => app.open_rename_branch_modal("main".into()),
            "upstream" => app.open_set_upstream_modal("main".into()),
            _ => unreachable!(),
        });
        paint(cx, window);
        if case == "upstream" {
            let input = cx
                .read(|cx| {
                    app.read(cx)
                        .set_upstream_modal()
                        .and_then(|m| m.input_state.clone())
                })
                .expect("the first paint creates the upstream input");
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value("", window, cx));
            })
            .unwrap();
            wait_painted(cx, &app, window, |app| {
                app.set_upstream_modal()
                    .and_then(|m| m.plan.plan())
                    .is_some_and(|plan| !plan.blockers.is_empty())
            });
        }
        if case == "tag" {
            wait_painted(cx, &app, window, |app| {
                app.create_tag_modal()
                    .and_then(|m| m.plan.plan())
                    .is_some_and(|plan| !plan.blockers.is_empty())
            });
        } else if case == "worktree" {
            wait_painted(cx, &app, window, |app| {
                app.create_worktree_modal()
                    .and_then(|m| m.plan.plan())
                    .is_some_and(|plan| !plan.blockers.is_empty())
            });
        } else if case == "rename" {
            wait_painted(cx, &app, window, |app| {
                app.rename_branch_modal()
                    .and_then(|m| m.plan.plan())
                    .is_some_and(|plan| !plan.blockers.is_empty())
            });
        }
        kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-recovery");
        if case == "upstream" {
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-field-error");
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-plan-blockers");
        }
        paint(cx, window);
        assert!(
            kagi::ui::e2e::control_bounds(window.window_id(), "input-recovery").is_none(),
            "{case}: empty or blocked input must not render a recovery line"
        );
        if case == "upstream" {
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "input-field-error").is_some(),
                "invalid upstream must explain its format error beneath the input"
            );
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "input-plan-blockers").is_none(),
                "the same upstream format blocker must not also be listed in the plan"
            );
        }
        let button = kagi::ui::e2e::confirm_bounds(window.window_id())
            .unwrap_or_else(|| panic!("{case}: blocked form still shows its primary action"));
        assert!(
            button.size.width >= theme::scaled_px(40.)
                && button.size.height >= theme::scaled_px(24.) - gpui::px(0.5),
            "{case}: the primary action must occupy visible button space, not a blank wrapper: {button:?}"
        );
        if case == "stash" {
            assert_modal_button_height(button.size.height, "blocked Stash");
        }
        cx.simulate_mouse_move(window, button.center(), None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(window, button.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            let still_open = match case {
                "tag" => app.create_tag_modal().is_some(),
                "worktree" => app.create_worktree_modal().is_some(),
                "stash" => app.stash_push_modal().is_some(),
                "rename" => app.rename_branch_modal().is_some(),
                "upstream" => app.set_upstream_modal().is_some(),
                _ => unreachable!(),
            };
            assert!(
                still_open,
                "{case}: disabled primary click keeps the card open"
            );
        });
        assert_eq!(
            repo_fingerprint(&repo),
            before,
            "{case}: disabled primary click cannot change repository state"
        );
        if case == "tag" {
            let input = cx
                .read(|cx| {
                    app.read(cx)
                        .create_tag_modal()
                        .and_then(|m| m.input_state.clone())
                })
                .expect("tag input remains available");
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value("v-ui", window, cx));
            })
            .unwrap();
            wait_painted(cx, &app, window, |app| {
                app.create_tag_modal()
                    .and_then(|m| m.plan.plan())
                    .is_some_and(|plan| plan.blockers.is_empty())
            });
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "input-recovery");
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "input-recovery").is_some(),
                "ready tag plan must show its command-only recovery row"
            );
            assert_eq!(
                repo_fingerprint(&repo),
                before,
                "planning never writes a tag"
            );
        }
        if case == "rename" || case == "upstream" {
            let input = cx
                .read(|cx| {
                    let app = app.read(cx);
                    match case {
                        "rename" => app
                            .rename_branch_modal()
                            .and_then(|m| m.input_state.clone()),
                        _ => app.set_upstream_modal().and_then(|m| m.input_state.clone()),
                    }
                })
                .expect("ready input card retains its text field");
            let value = if case == "rename" {
                "renamed-956"
            } else {
                "origin/main"
            };
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
                window.focus(&input.read(cx).focus_handle(cx), cx);
                window.draw(cx).clear();
            })
            .unwrap();
            wait_painted(cx, &app, window, |app| {
                let plan = if case == "rename" {
                    app.rename_branch_modal().and_then(|m| m.plan.plan())
                } else {
                    app.set_upstream_modal().and_then(|m| m.plan.plan())
                };
                plan.is_some_and(|plan| plan.blockers.is_empty())
            });
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| {
                    gpui::EntityInputHandler::replace_and_mark_text_in_range(
                        state,
                        Some(0..value.len()),
                        value,
                        Some(value.len()..value.len()),
                        window,
                        cx,
                    );
                });
                window.draw(cx).clear();
            })
            .unwrap();
            cx.simulate_keystrokes(window, "enter");
            cx.run_until_parked();
            let remains_open = cx.read(|cx| {
                let app = app.read(cx);
                if case == "rename" {
                    app.rename_branch_modal().is_some()
                } else {
                    app.set_upstream_modal().is_some()
                }
            });
            assert!(
                remains_open,
                "{case}: IME Enter must keep the ready form open"
            );
            assert_eq!(
                repo_fingerprint(&repo),
                before,
                "{case}: accepting IME candidates must not write Git state"
            );
        }
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS input_confirm_disabled_cards");
}

/// #510: a replan that fails becomes an explicit failure state instead of
/// leaving the plan it was recomputing behind.
///
/// The oracle is the modal's plan slot plus the repository itself: after the
/// repository moves out from under the open session, `confirm_create_branch`
/// replans (it always does, ahead of the debounce), the replan fails, and the
/// slot goes to `Failed` with no plan. Enter and the coordinate the confirm
/// button occupied while the plan was `Ready` must then both be inert — since
/// #559 they are the same `confirm_create_branch` entry, so proving one path
/// refuses proves the dispatch, and proving the other refuses proves the state.
pub fn scenario_create_branch_replan_error(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let head = CommitId(output(&repo, &["rev-parse", "HEAD"]));
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
    // The first paint creates the real branch-name input; the modal's own focus
    // call happens while that frame is still being built, so it is not yet in the
    // dispatch tree a keystroke resolves against. Focus the handle from the test
    // and repaint before typing — same shape as `scenario_editor_save_admission`.
    paint(cx, window);
    let input = cx
        .read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .and_then(|m| m.input_state.clone())
        })
        .expect("the first paint creates the branch-name input");
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "f e a t");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        "feat",
        "the keystrokes must land in the branch-name input, not the root"
    );
    wait_painted(cx, &app, window, |app| {
        app.create_branch_modal()
            .and_then(|m| m.plan.plan())
            .is_some_and(|plan| plan.blockers.is_empty())
    });
    let bounds = kagi::ui::e2e::confirm_bounds(window.window_id())
        .expect("a blocker-free plan renders the Create button");

    let moved = repo.with_extension("moved");
    std::fs::rename(&repo, &moved).unwrap();

    press_enter(cx, &app, window);
    cx.run_until_parked();
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();

    cx.read(|cx| {
        let modal = app
            .read(cx)
            .create_branch_modal()
            .expect("a plan failure keeps the modal open so the user can retry");
        assert!(
            modal.plan.plan().is_none(),
            "the failed replan must drop the plan it was recomputing"
        );
        assert!(
            modal.plan.error().is_some(),
            "the failure must be explicit in the modal, not stderr only"
        );
    });

    std::fs::rename(&moved, &repo).unwrap();
    assert!(
        output(&repo, &["branch", "--list", "feat"]).is_empty(),
        "neither Enter nor the old button coordinate may execute the stale plan"
    );

    // Retry: the same modal, replanning against a repository that is back, must
    // produce a confirmable plan and run it — a failure is recoverable, not terminal.
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(
        !output(&repo, &["branch", "--list", "feat"]).is_empty(),
        "a retry after the failure must plan and execute normally"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS create-branch replan error rejects Enter and the old button coordinate"
    );
}

/// #584: both real confirm inputs share the unmerged arm transition.
pub fn scenario_unmerged_branch_delete_armed(cx: &mut VisualTestAppContext) {
    use kagi::ui::i18n::{self, Lang};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    delayed_delete_plan_stays_with_its_owner(cx);
    delete_recording_failure_does_not_offer_retry(cx);
    for input in ["enter", "button"] {
        let fixture = build_fixture();
        let repo = fixture.path();
        let head = output(repo, &["rev-parse", "HEAD"]);
        git(repo, &["branch", "merged-delete", "HEAD~1"]);
        git(repo, &["checkout", "-q", "-b", "unmerged-delete"]);
        std::fs::write(
            repo.join("only-on-branch.txt"),
            "keep this commit after deletion\n",
        )
        .unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-q", "-m", "unmerged work"]);
        let tip = output(repo, &["rev-parse", "HEAD"]);
        git(repo, &["checkout", "-q", "main"]);
        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| {
            app.open_delete_branch_modal("unmerged-delete", cx)
        });
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app
            .read(cx)
            .delete_branch_modal()
            .unwrap()
            .plan
            .blockers
            .is_empty()));
        confirm_branch_delete(cx, &app, window, input);
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).delete_branch_modal().unwrap().confirm_armed));
        assert_eq!(
            output(repo, &["rev-parse", "refs/heads/unmerged-delete"]),
            tip
        );
        assert_eq!(output(repo, &["rev-parse", "HEAD"]), head);
        assert!(
            records(repo, "delete-branch").is_empty(),
            "arming must not record or execute"
        );
        confirm_branch_delete(cx, &app, window, input);
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_none()));
        assert_eq!(output(repo, &["branch", "--list", "unmerged-delete"]), "");
        assert_eq!(output(repo, &["rev-parse", "HEAD"]), head);
        let entries = records(repo, "delete-branch");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(entries[0].backup_refs.len(), 1);
        assert_eq!(
            output(repo, &["rev-parse", &entries[0].backup_refs[0]]),
            tip
        );
        cx.read(|cx| {
            let panel = app.read(cx).op_log.as_ref().unwrap().read(cx);
            let displayed = panel.entries().front().expect("recorded receipt in panel");
            assert_eq!(displayed.id, entries[0].id);
            assert_eq!(displayed.backup_refs, entries[0].backup_refs);
        });

        // Merged branches still execute on the first Enter/click.
        app.update(cx, |app, cx| {
            app.open_delete_branch_modal("merged-delete", cx)
        });
        wait_idle(cx, &app);
        if input == "button" {
            let command = cx.read(|cx| {
                app.read(cx)
                    .delete_branch_modal()
                    .unwrap()
                    .plan
                    .equivalent_command
                    .clone()
                    .expect("merged delete has an equivalent CLI command")
            });
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "plan-equivalent-command-body");
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_none(),
                "the full command must start collapsed"
            );
            let disclosure =
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command")
                    .expect("plan offers command disclosure");
            let copy =
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-copy")
                    .expect("the command has its own copy button");
            assert!(
                disclosure.size.height < gpui::px(40.)
                    && disclosure.right() <= copy.left() + gpui::px(1.),
                "closed command is a single row with a separate copy action: {disclosure:?}, {copy:?}"
            );
            cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(command.clone()),
                "command copy keeps the unshortened CLI text"
            );
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_none(),
                "copying the command must not expand the disclosure"
            );
            for (language, heading) in [
                (Lang::En, "\nequivalent command:\n"),
                (Lang::Ja, "\n相当するコマンド:\n"),
            ] {
                i18n::set_lang(language);
                paint(cx, window);
                let all = kagi::ui::e2e::control_bounds(window.window_id(), "plan-card-copy")
                    .expect("Copy all remains available");
                cx.simulate_click(window, all.center(), gpui::Modifiers::none());
                cx.run_until_parked();
                let copied = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .expect("whole plan copied");
                assert!(copied.contains(heading), "{language:?}: {copied}");
                assert_eq!(
                    copied.matches(&command).count(),
                    1,
                    "Copy all includes the faithful command only once: {copied}"
                );
            }
            i18n::set_lang(original_language);
            cx.simulate_click(window, disclosure.center(), gpui::Modifiers::none());
            cx.run_until_parked();
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_some(),
                "the full command is readable when expanded"
            );
            // Pointer focus stays on the disclosure: Enter closes it and
            // Space reopens it without arming the destructive confirmation.
            crate::keyboard_nav::keys(cx, window, "enter");
            kagi::ui::e2e::clear_control_bounds(window.window_id(), "plan-equivalent-command-body");
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_none(),
                "Enter on the disclosure must close it"
            );
            crate::keyboard_nav::keys(cx, window, "space");
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_some(),
                "Space on the disclosure must reopen it"
            );
            // The dedicated Copy control is the next Tab stop after the
            // disclosure, and keyboard activation must copy without toggling it.
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                "before keyboard copy".into(),
            ));
            crate::keyboard_nav::keys(cx, window, "tab enter");
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(command.clone()),
                "Enter on the Copy button must write the full command"
            );
            cx.write_to_clipboard(gpui::ClipboardItem::new_string("before space copy".into()));
            crate::keyboard_nav::keys(cx, window, "space");
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(command.clone()),
                "Space on the Copy button must write the full command"
            );
            paint(cx, window);
            assert!(
                kagi::ui::e2e::control_bounds(window.window_id(), "plan-equivalent-command-body")
                    .is_some(),
                "keyboard Copy must not toggle the disclosure"
            );
        }
        confirm_branch_delete(cx, &app, window, input);
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).delete_branch_modal().is_none()));
        assert_eq!(output(repo, &["branch", "--list", "merged-delete"]), "");
        assert_eq!(records(repo, "delete-branch").len(), 2);
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS unmerged_branch_delete_armed: Enter/button arm then execute; merged stays one-stage");
}

fn confirm_branch_delete(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    input: &str,
) {
    if input == "enter" {
        press_enter(cx, app, window);
    } else {
        paint(cx, window);
        let bounds =
            kagi::ui::e2e::confirm_bounds(window.window_id()).expect("delete confirm bounds");
        cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    }
}

fn delayed_delete_plan_stays_with_its_owner(cx: &mut VisualTestAppContext) {
    for revisit in [false, true] {
        let fixture = build_fixture();
        let other = tempfile::tempdir().unwrap();
        // Identical commits/HEAD in two distinct owners: preflight cannot be
        // relied upon to recognize a proposal delivered to the wrong repo.
        git(
            other.path(),
            &["clone", "-q", fixture.path().to_str().unwrap(), "."],
        );
        git(fixture.path(), &["branch", "victim", "HEAD~1"]);
        git(other.path(), &["branch", "victim", "HEAD~1"]);
        assert_eq!(
            output(fixture.path(), &["rev-parse", "HEAD"]),
            output(other.path(), &["rev-parse", "HEAD"])
        );
        let before = repo_fingerprint(other.path());
        let (app, window) = mount(cx, fixture.path());
        app.update(cx, |app, cx| {
            app.open_delete_branch_modal("victim", cx);
            assert!(app.open_repository(other.path().to_path_buf(), cx));
            if revisit {
                app.switch_repo(0, cx);
            }
        });
        // run_until_parked also drains the test background executor.
        cx.run_until_parked();
        wait_idle(cx, &app);
        assert!(
            cx.read(|cx| app.read(cx).delete_branch_modal().is_none()),
            "a departed owner's delayed plan must not install a modal, including on revisit"
        );
        press_enter(cx, &app, window);
        cx.run_until_parked();
        assert_eq!(repo_fingerprint(other.path()), before);
        assert!(records(other.path(), "delete-branch").is_empty());
        assert!(records(fixture.path(), "delete-branch").is_empty());
        app.update(cx, |app, cx| {
            app.switch_repo(0, cx);
            assert!(app.delete_branch_modal().is_none());
            assert!(app.write_busy_op.is_none());
            app.open_delete_branch_modal("victim", cx);
        });
        wait_idle(cx, &app);
        assert!(
            cx.read(|cx| app.read(cx).delete_branch_modal().is_some()),
            "returning to A must permit a fresh plan"
        );
        unmount(cx, app, window);
    }
}

fn delete_recording_failure_does_not_offer_retry(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "victim", "HEAD~1"]);
    let tip = output(repo, &["rev-parse", "victim"]);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_delete_branch_modal("victim", cx));
    wait_idle(cx, &app);
    let log_dir = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    let log_path = log_dir.join("operations.jsonl");
    let before = std::fs::read(&log_path).unwrap_or_default();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(log_dir.join("operations.jsonl.lock"))
        .unwrap();
    lock.lock().unwrap();
    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    drop(lock);
    assert_eq!(output(repo, &["branch", "--list", "victim"]), "");
    assert_eq!(std::fs::read(&log_path).unwrap_or_default(), before);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.delete_branch_modal().is_none(),
            "completed deletion cannot be retried"
        );
        let notice = kagi::ui::e2e::app_notice_message(state).unwrap();
        assert!(notice.contains("recording failed") && notice.contains(repo.to_str().unwrap()));
        let panel = state.op_log.as_ref().unwrap().read(cx);
        let entry = panel
            .entries()
            .front()
            .expect("attempted receipt must reach the panel");
        assert!(matches!(entry.outcome, OpOutcome::Partial { .. }));
        assert_eq!(
            Path::new(&entry.repo).canonicalize().unwrap(),
            repo.canonicalize().unwrap(),
            "receipt owner must resolve to the fixture repository"
        );
        assert_eq!(
            entry.backup_refs.len(),
            1,
            "Partial presentation must retain the attempted recovery root"
        );
        assert_eq!(output(repo, &["rev-parse", &entry.backup_refs[0]]), tip);
    });
    unmount(cx, app, window);
}

/// #590: deliver actual drag events from a graph remote chip to a non-HEAD
/// sidebar branch row, then confirm through the shared Enter entry point.
pub fn scenario_remote_source_merge_into(cx: &mut VisualTestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
    git(dir, &["branch", "target"]);
    let source = {
        let repo = git2::Repository::open(dir).unwrap();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        let mut tree = repo.treebuilder(Some(&base.tree().unwrap())).unwrap();
        tree.insert("remote.txt", repo.blob(b"remote\n").unwrap(), 0o100644)
            .unwrap();
        let sig = repo.signature().unwrap();
        let source = repo
            .commit(
                None,
                &sig,
                &sig,
                "remote source",
                &repo.find_tree(tree.write().unwrap()).unwrap(),
                &[&base],
            )
            .unwrap();
        repo.reference("refs/remotes/origin/source", source, false, "fixture")
            .unwrap();
        source.to_string()
    };
    let before = repo_fingerprint(dir);
    let index = std::fs::read(dir.join(".git/index")).unwrap();
    let (app, window) = mount(cx, dir);
    paint(cx, window);
    let source_box =
        kagi::ui::e2e::control_bounds(window.window_id(), "graph-remote-origin/source")
            .expect("remote chip hitbox");
    let target_box = kagi::ui::e2e::control_bounds(window.window_id(), "sidebar-local-target")
        .expect("non-HEAD sidebar row hitbox");
    cx.simulate_mouse_move(window, source_box.center(), None, Modifiers::none());
    cx.simulate_mouse_down(
        window,
        source_box.center(),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(
        window,
        source_box.center() + gpui::point(gpui::px(12.), gpui::px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_move(
        window,
        target_box.center(),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_up(
        window,
        target_box.center(),
        MouseButton::Left,
        Modifiers::none(),
    );
    wait_idle(cx, &app);
    cx.read(|cx| {
        let modal = app.read(cx).merge_modal().expect("drop opens merge plan");
        assert!(modal.off_branch);
        assert_eq!(modal.target, "origin/source");
        assert_eq!(modal.into_branch, "target");
        assert!(modal.plan.blockers.is_empty());
        assert!(modal
            .plan
            .warnings
            .iter()
            .any(|note| note.message_en().contains(&source)
                && note.message_en().contains("last fetch")));
    });
    assert_eq!(repo_fingerprint(dir), before);
    assert!(records(dir, "merge-into").is_empty());
    assert_ne!(output(dir, &["rev-parse", "target"]), source);
    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    assert_eq!(output(dir, &["rev-parse", "target"]), source);
    assert_eq!(
        output(dir, &["rev-parse", "refs/remotes/origin/source"]),
        source
    );
    assert_eq!(repo_fingerprint(dir), before);
    assert_eq!(std::fs::read(dir.join(".git/index")).unwrap(), index);
    assert_eq!(
        output(dir, &["for-each-ref", "--format=%(refname)", "refs/heads"]),
        "refs/heads/main\nrefs/heads/target"
    );
    let entries = records(dir, "merge-into");
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    unmount(cx, app, window);
}

/// #1136: a filtered Stage refusal reaches the actual Failed footer in EN/JA.
pub fn scenario_stage_external_filter(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e;
    use kagi_ui_core::i18n::{self, Lang};
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous = i18n::lang();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        git(repo, &["init", "-q", "-b", "main"]);
        std::fs::write(
            repo.join(".gitattributes"),
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .unwrap();
        std::fs::write(repo.join("asset.bin"), "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n").unwrap();
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", "pointer fixture"]);
        std::fs::write(repo.join("asset.bin"), "not an LFS pointer\n").unwrap();
        let (app, window) = mount(cx, repo);
        let index = std::fs::read(repo.join(".git/index")).unwrap();
        let fingerprint = repo_fingerprint(repo);
        app.update(cx, |app, cx| {
            app.do_stage_file_by_path(app.active_session().unwrap(), "asset.bin".into(), cx);
        });
        wait_idle(cx, &app);
        paint(cx, window);
        let expected = match language {
            Lang::En => "external filter (lfs)",
            Lang::Ja => "外部 filter (lfs)",
        };
        cx.read(|cx| {
            let state = app.read(cx);
            let FooterStatus::Failed(footer) = &state.status_footer else {
                panic!("filtered Stage must show failure footer")
            };
            assert!(footer.contains(expected), "{footer}");
            assert!(footer.contains("asset.bin") && footer.contains("git"));
            let toast = state
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .last()
                .unwrap();
            assert_eq!(toast.kind, ToastKind::Error);
            assert!(toast.message.contains(expected), "{}", toast.message);
            assert!(e2e::app_notice_message(state).is_none());
        });
        assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
        assert_eq!(repo_fingerprint(repo), fingerprint);
        let entries = records(repo, "stage");
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0].outcome, OpOutcome::Refused { blockers } if blockers[0].contains("external filter (lfs)"))
        );
        unmount(cx, app, window);
    }
    i18n::set_lang(previous);
}

/// #490: same real index.lock failure from panel indices, batch buttons and
/// editor paths, including a linked panel while the main tab stays active.
///
/// ADR-0196 §3 (as amended by #747): a recorded staging failure reaches the
/// user as the Operation Log receipt plus the Failed footer and an Error toast
/// — never as a dismiss-only `AppNotice`, which is reserved for a receipt that
/// could not be persisted. (The scenario used to assert that notice and has
/// failed since #747 removed it; #846, the same decay as #824.)
pub fn scenario_stage_failure_notice(cx: &mut VisualTestAppContext) {
    use kagi::ui::e2e;
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main");
    let linked = temp.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    std::fs::write(main.join("f.txt"), "base\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "base"]);
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
    for repo in [&main, &linked] {
        std::fs::write(repo.join("f.txt"), "dirty\n").unwrap();
    }
    let (app, window) = mount(cx, &main);
    for (repo, entry) in [
        (&main, "editor"),
        (&main, "panel"),
        (&main, "batch"),
        (&linked, "panel"),
        (&linked, "batch"),
    ] {
        for stage in [true, false] {
            if stage {
                git(repo, &["reset", "-q", "--", "f.txt"]);
            } else {
                git(repo, &["add", "f.txt"]);
            }
            cx.read(|cx| assert!(e2e::app_notice_message(app.read(cx)).is_none()));
            app.update(cx, |app, cx| {
                e2e::open_worktree_panel_no_inputs(app, repo.clone(), "fixture", 0, cx);
            });
            cx.run_until_parked();
            let before = (repo_fingerprint(&main), repo_fingerprint(&linked));
            let git_dir =
                std::path::PathBuf::from(output(repo, &["rev-parse", "--absolute-git-dir"]));
            let index = std::fs::read(git_dir.join("index")).unwrap();
            let lock = git_dir.join("index.lock");
            std::fs::write(&lock, "fixture lock").unwrap();
            let op = match (stage, entry) {
                (true, "batch") => "stage-all",
                (false, "batch") => "unstage-all",
                (true, _) => "stage",
                (false, _) => "unstage",
            };
            let count = records(repo, op).len();
            app.update(cx, |app, cx| match (stage, entry) {
                (true, "editor") => {
                    app.do_stage_file_by_path(app.active_session().unwrap(), "f.txt".into(), cx)
                }
                (false, "editor") => {
                    app.do_unstage_file_by_path(app.active_session().unwrap(), "f.txt".into(), cx)
                }
                (true, "batch") => app.do_stage_all(app.active_session().unwrap(), cx),
                (false, "batch") => app.do_unstage_all(app.active_session().unwrap(), cx),
                (true, _) => app.do_stage_file(app.active_session().unwrap(), 0, cx),
                (false, _) => app.do_unstage_file(app.active_session().unwrap(), 0, cx),
            });
            wait_idle(cx, &app);
            cx.read(|cx| {
                let state = app.read(cx);
                let FooterStatus::Failed(footer) = &state.status_footer else {
                    panic!("missing staging failure footer")
                };
                assert!(footer.contains("lock"), "{footer}");
                assert!(footer.contains("f.txt"));
                let owner = if entry == "editor" {
                    state.repo_path.as_ref().expect("editor owner").clone()
                } else {
                    state
                        .ui()
                        .commit_panel
                        .as_ref()
                        .expect("panel")
                        .read(cx)
                        .repo_path
                        .clone()
                };
                assert_eq!(
                    std::fs::canonicalize(&owner).unwrap(),
                    std::fs::canonicalize(repo).unwrap()
                );
                assert!(footer.contains(owner.to_str().unwrap()), "{footer}");
                let toast = state.toast_stack.as_ref().unwrap().read(cx).toasts().last();
                assert!(
                    toast.is_some_and(|t| t.kind == ToastKind::Error
                        && t.message.starts_with(&format!("{op}: failed"))),
                    "stage-failure-toasts: {op} via {entry} must toast its failure, got {toast:?}"
                );
                assert!(
                    e2e::app_notice_message(state).is_none(),
                    "stage-failure-no-notice: a recorded failure must not open a dismiss-only notice, got {:?}",
                    e2e::app_notice_message(state)
                );
                let panel = state.op_log.as_ref().unwrap().read(cx);
                let attempted = panel.entries().front().unwrap();
                assert!(matches!(attempted.outcome, OpOutcome::Failed { .. }));
                assert_eq!(
                    std::fs::canonicalize(&attempted.repo).unwrap(),
                    std::fs::canonicalize(repo).unwrap()
                );
            });
            assert_eq!(records(repo, op).len(), count + 1);
            assert_eq!(std::fs::read(git_dir.join("index")).unwrap(), index);
            std::fs::remove_file(lock).unwrap();
            assert_eq!((repo_fingerprint(&main), repo_fingerprint(&linked)), before);
        }
    }
    // Admission denial never reaches a mutation or opens a modal.
    let guard = app.update(cx, |app, _| app.app_sessions.write_lease(&main).unwrap());
    let count = records(&main, "stage").len();
    app.update(cx, |app, cx| {
        app.do_stage_file_by_path(app.active_session().unwrap(), "f.txt".into(), cx)
    });
    cx.read(|cx| assert!(e2e::app_notice_message(app.read(cx)).is_none()));
    assert_eq!(records(&main, "stage").len(), count + 1);
    assert!(records(&main, "stage")
        .iter()
        .any(|e| matches!(e.outcome, OpOutcome::Refused { .. })));
    guard.complete();
    unmount(cx, app, window);
}

/// #344 slice 2: "Replay <branch> onto <current>…" from the branch menu goes
/// through the shared rebase card with a two-stage confirm (the plan is
/// destructive), then moves the branch by ref update only — the branch is
/// checked out in a second worktree and neither worktree's index or files
/// change, the backup ref holds the old tip, and the oplog has the receipt.
pub fn scenario_replay_onto_armed(cx: &mut VisualTestAppContext) {
    if !kagi_git::cli::GitFeatures::detected().replay_onto {
        eprintln!("[gui-e2e] SKIP replay_onto_armed: git replay needs git 2.44+");
        return;
    }
    for input in ["enter", "button"] {
        let fixture = build_fixture();
        let repo = fixture.path();
        // feat forks before "second commit"; it lives in its own worktree.
        git(repo, &["branch", "feat", "HEAD~1"]);
        // In its own TempDir: a sibling of the fixture outlived the run (#516).
        let wt_dir = tempfile::tempdir().unwrap();
        let wt = wt_dir.path().join(format!("wt-feat-{input}"));
        git(
            repo,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "feat"],
        );
        std::fs::write(wt.join("feat.txt"), "feature work\n").unwrap();
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "feat work"]);
        let feat_before = output(repo, &["rev-parse", "refs/heads/feat"]);
        let head = output(repo, &["rev-parse", "HEAD"]);
        // Dirty the current worktree: replay must not care.
        std::fs::write(repo.join("README.md"), "# fixture\nlocal edit\n").unwrap();
        let main_status = output(repo, &["status", "--porcelain"]);
        let main_index = output(repo, &["ls-files", "-s"]);
        let wt_index = output(&wt, &["ls-files", "-s"]);

        let (app, window) = mount(cx, repo);
        app.update(cx, |app, cx| app.open_replay_modal("feat".to_string(), cx));
        wait_idle(cx, &app);
        cx.read(|cx| {
            let modal = app
                .read(cx)
                .rebase_current_onto_modal()
                .expect("replay modal");
            assert!(
                matches!(modal.op, kagi_git::Operation::ReplayOnto { .. }),
                "the shared rebase card carries the replay op"
            );
            assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
            assert!(modal.plan.destructive);
            assert!(!modal.confirm_armed);
        });
        // Planning wrote nothing.
        assert_eq!(output(repo, &["rev-parse", "refs/heads/feat"]), feat_before);

        confirm_branch_delete(cx, &app, window, input);
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app
                .read(cx)
                .rebase_current_onto_modal()
                .unwrap()
                .confirm_armed),
            "{input}: first confirm arms"
        );
        assert_eq!(output(repo, &["rev-parse", "refs/heads/feat"]), feat_before);
        assert!(
            records(repo, "replay-onto").is_empty(),
            "arming must not record or execute"
        );

        confirm_branch_delete(cx, &app, window, input);
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).rebase_current_onto_modal().is_none()));
        let feat_after = output(repo, &["rev-parse", "refs/heads/feat"]);
        assert_ne!(feat_after, feat_before, "{input}: feat moved");
        assert_eq!(
            output(repo, &["rev-parse", "refs/heads/feat~1"]),
            head,
            "…onto main"
        );
        assert_eq!(output(repo, &["rev-parse", "HEAD"]), head, "HEAD untouched");
        assert_eq!(
            output(repo, &["status", "--porcelain"]),
            main_status,
            "current worktree untouched"
        );
        assert_eq!(output(repo, &["ls-files", "-s"]), main_index);
        assert_eq!(
            output(&wt, &["ls-files", "-s"]),
            wt_index,
            "other worktree's index untouched"
        );
        assert_eq!(
            output(&wt, &["rev-parse", "HEAD"]),
            feat_after,
            "its HEAD follows the ref"
        );
        let entries = records(repo, "replay-onto");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(entries[0].backup_refs.len(), 1);
        assert_eq!(
            output(repo, &["rev-parse", &entries[0].backup_refs[0]]),
            feat_before
        );
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS replay_onto_armed: Enter/button arm then replay; both worktrees untouched, ref moved, backup + oplog");
}

/// #354 slice 4 (ADR-0216): switching to the Color Vision theme through the
/// app's own path changes the added/removed, success/blocker, ours/theirs and
/// diff-row tokens to the blue/orange pair, persists the slug, shows a
/// localized name in Settings, and the live tokens keep CIEDE2000 ≥ 20 for
/// typical vision and ≥ 15 after Machado-2009 protan / deutan / tritan
/// simulation — where the default theme falls below 15 for deutans.
pub fn scenario_color_vision_theme(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "lang"]);
    use gpui_component::select::SelectItem;
    use kagi::ui::commands::{self, ThemeColorVision};
    use kagi::ui::i18n::{self, Lang};
    use kagi_ui_core::color_vision::{delta_e, Cvd};
    use kagi_ui_core::theme::theme;
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let before = theme().slug.to_string();
    app.update(cx, |app, cx| app.set_theme("catppuccin", cx));
    cx.run_until_parked();
    let mocha = theme();
    let pairs = |t: &kagi_ui_core::theme::Theme| {
        [
            ("change added/deleted", t.change_added, t.change_deleted),
            ("success/blocker", t.color_success, t.color_blocker),
            ("ours/theirs", t.color_branch, t.color_remote),
            ("diff bg added/removed", t.diff_added_bg, t.diff_removed_bg),
        ]
    };
    let mocha_deutan = pairs(&*mocha)
        .iter()
        .map(|(_, a, b)| delta_e(*a, *b, Some(Cvd::Deuteranopia)))
        .fold(f64::INFINITY, f64::min);
    assert!(
        mocha_deutan < 15.0,
        "control: Mocha worst deutan ΔE {mocha_deutan:.1}"
    );

    assert!(commands::theme_menu_entries()
        .iter()
        .any(|entry| entry.slug == "color-vision"));
    assert_eq!(
        commands::theme_slug_for_command("theme.colorVision"),
        Some("color-vision")
    );
    cx.dispatch_action(window, ThemeColorVision);
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let t = theme();
    assert_eq!(t.slug, "color-vision");
    assert_eq!(
        kagi_ui_core::settings::Settings::load().theme().as_deref(),
        Some("color-vision"),
        "the choice is persisted to settings.json"
    );
    assert_ne!(t.change_added, mocha.change_added);
    assert_ne!(t.change_deleted, mocha.change_deleted);
    assert_ne!(t.diff_added_bg, mocha.diff_added_bg);
    assert_ne!(t.diff_removed_bg, mocha.diff_removed_bg);
    assert_eq!(t.bg_base, mocha.bg_base, "other tokens are inherited");
    assert_eq!(t.text_main, mocha.text_main);
    for (name, a, b) in pairs(&*t) {
        let normal = delta_e(a, b, None);
        assert!(normal >= 20.0, "{name}: ΔE {normal:.1}");
        let mut line = format!("{name}: normal {normal:.1}");
        for cvd in Cvd::ALL {
            let d = delta_e(a, b, Some(cvd));
            assert!(d >= 15.0, "{name} {cvd:?}: ΔE {d:.1}");
            line.push_str(&format!(" {cvd:?} {d:.1}"));
        }
        eprintln!("[gui-e2e] color_vision {line}");
    }

    // Keep an option from the picker built at startup: its label must change
    // when the app language changes, not just in a newly built options vec.
    let option = kagi::ui::settings_view::theme_options()
        .into_iter()
        .find(|o| o.slug == "color-vision")
        .unwrap();
    let previous = i18n::lang();
    for (lang, want) in [
        (Lang::En, "Color Vision (Blue/Orange)"),
        (Lang::Ja, "色覚対応（青 / 橙）"),
    ] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        assert_eq!(option.title().as_ref(), want, "{lang:?}: cached option");
    }
    app.update(cx, |app, cx| app.set_lang(previous, cx));

    app.update(cx, |app, cx| app.set_theme(&before, cx));
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS color_vision_theme: tokens switched + persisted, ΔE ≥ 20 / ≥ 15 (protan, deutan, tritan), Mocha control < 15, EN/JA name");
}

/// #889 review: opening the current worktree's Commit Panel selects its WIP
/// option for both the visual highlight and accessibility.
pub fn scenario_wip_selected_roles(cx: &mut VisualTestAppContext) {
    use kagi::ui::list_a11y::{clear_recorded_lists, recorded_list};
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("README.md"), "# changed\n").unwrap();
    let (app, window) = mount(cx, fixture.path());
    wait_idle(cx, &app);
    clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let before = recorded_list("commit-list").expect("commit list");
    assert!(!before.rows[&0].1, "closed WIP is not selected");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    assert!(cx.read(|cx| app.read(cx).ui().commit_panel_open));
    let after = recorded_list("commit-list").expect("commit list after opening panel");
    assert!(after.rows[&0].1, "open current WIP must be aria-selected");
    assert_eq!(
        after
            .rows
            .values()
            .filter(|(_, selected)| *selected)
            .count(),
        1,
        "only the current WIP is selected"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS wip_selected_roles: open WIP is selected for AT");
}

fn redraw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    kagi::ui::dialog_a11y::clear_recorded_a11y();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

/// #354 slice 2: confirmation cards are dialogs for assistive technology.
/// The AccessKit tree only exists with an AT connected (#840), so the oracle
/// is what the renderer set: role, name, description, the Confirm/Cancel
/// custom actions, and each note row's role and name — across the armed
/// transition, a single-stage card and a blocked one, on the shared plan
/// card (delete-branch) and a bespoke card (discard).
pub fn scenario_dialog_a11y_roles(cx: &mut VisualTestAppContext) {
    use gpui::Role;
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "merged-delete", "HEAD~1"]);
    git(repo, &["checkout", "-q", "-b", "unmerged-delete"]);
    std::fs::write(repo.join("only-on-branch.txt"), "x\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "unmerged work"]);
    git(repo, &["checkout", "-q", "main"]);
    let (app, window) = mount(cx, repo);

    // Two-stage (unmerged): AlertDialog, "needs two confirmations", Confirm
    // named by the visible label, warning row is a Note.
    app.update(cx, |app, cx| {
        app.open_delete_branch_modal("unmerged-delete", cx)
    });
    wait_idle(cx, &app);
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("plan card drawn");
    let title = cx.read(|cx| {
        kagi_ui_core::i18n::plan_title_text(&app.read(cx).delete_branch_modal().unwrap().plan.title)
    });
    assert_eq!(d.role, Role::AlertDialog);
    assert_eq!(d.label, title);
    let unarmed_desc = d.description.clone().expect("two-stage description");
    assert_eq!(d.actions.len(), 2);
    assert_eq!(
        d.actions[0].1,
        kagi_ui_core::i18n::Msg::PlanDeleteBranch.t()
    );
    let (role, text) =
        kagi::ui::dialog_a11y::recorded_note("plan-warning-0").expect("warning row drawn");
    assert_eq!(role, Role::Note);
    assert!(!text.is_empty());

    // Armed: description and Confirm name change.
    press_enter(cx, &app, window);
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).delete_branch_modal().unwrap().confirm_armed));
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("armed card drawn");
    assert_eq!(d.role, Role::AlertDialog);
    assert_ne!(
        d.description.as_deref(),
        Some(unarmed_desc.as_str()),
        "arming changes the description"
    );
    assert_eq!(
        d.actions[0].1,
        kagi_ui_core::i18n::Msg::PlanDeleteBranchArmed.t()
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    // Single-stage cards omit stage instructions but retain recovery advice.
    app.update(cx, |app, cx| {
        app.open_delete_branch_modal("merged-delete", cx)
    });
    wait_idle(cx, &app);
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("merged card drawn");
    let destructive = cx.read(|cx| app.read(cx).delete_branch_modal().unwrap().plan.destructive);
    assert_eq!(
        d.role,
        if destructive {
            Role::AlertDialog
        } else {
            Role::Dialog
        }
    );
    let recovery = cx.read(|cx| {
        kagi_ui_core::i18n::plan_recovery_text(
            app.read(cx)
                .delete_branch_modal()
                .unwrap()
                .plan
                .recovery
                .as_ref(),
        )
    });
    assert_eq!(d.description.as_deref(), Some(recovery.as_str()));
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    // Blocked (current branch): blocker row is an Alert, no Confirm action.
    app.update(cx, |app, cx| app.open_delete_branch_modal("main", cx));
    wait_idle(cx, &app);
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("blocked card drawn");
    assert_eq!(d.actions.len(), 1, "{:?}", d.actions);
    assert_eq!(d.actions[0].1, kagi_ui_core::i18n::Msg::PlanCancel.t());
    let (role, _) =
        kagi::ui::dialog_a11y::recorded_note("plan-blocker-0").expect("blocker row drawn");
    assert_eq!(role, Role::Alert);
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    // Bespoke card: discard all — AlertDialog, localized Confirm names that
    // follow the armed label.
    std::fs::write(repo.join("README.md"), "# changed\n").unwrap();
    app.update(cx, |app, cx| app.reload_manual(cx));
    wait_idle(cx, &app);
    let repo_buf = repo.to_path_buf();
    app.update(cx, |app, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(app, repo_buf, cx)
    });
    cx.run_until_parked();
    let owner = cx.read(|cx| {
        app.read(cx)
            .ui()
            .commit_panel
            .as_ref()
            .expect("commit panel")
            .read(cx)
            .owner
    });
    app.update(cx, |app, cx| app.open_discard_all_modal(owner, cx));
    wait_idle(cx, &app);
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("discard-card").expect("discard card drawn");
    assert_eq!(d.role, Role::AlertDialog);
    assert_eq!(
        d.actions[0].1,
        kagi_ui_core::i18n::Msg::PlanDiscardConfirm
            .t()
            .replace("{}", "1")
    );
    press_enter(cx, &app, window);
    cx.run_until_parked();
    redraw(cx, &app, window);
    let d = kagi::ui::dialog_a11y::recorded_dialog("discard-card").expect("armed discard drawn");
    assert_eq!(
        d.actions[0].1,
        kagi_ui_core::i18n::Msg::PlanDiscardConfirmArmed
            .t()
            .replace("{}", "1")
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "# changed\n"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS dialog_a11y_roles: dialog role/name/description/actions + note roles across arm, single, blocked, bespoke");
}

/// #536 slice 2: "Sync to remote (keep local)…" from the branch menu goes
/// through the shared branch-plan card with a two-stage confirm (the plan is
/// destructive), then matches branch, index and working tree to the fetched
/// upstream while the old tip and all local work sit in two backup refs.
pub fn scenario_sync_to_remote_armed(cx: &mut VisualTestAppContext) {
    for input in ["enter", "button"] {
        let fixture = build_fixture();
        let repo = fixture.path();
        // A bare origin whose main is one commit *behind* the fixture's
        // second commit, plus one commit of its own: local is ahead 1.
        let origin_dir = tempfile::tempdir().unwrap();
        let origin = origin_dir.path().join(format!("origin-{input}.git"));
        git(
            repo,
            &[
                "clone",
                "-q",
                "--bare",
                repo.to_str().unwrap(),
                origin.to_str().unwrap(),
            ],
        );
        git(repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
        git(repo, &["branch", "--set-upstream-to=origin/main", "main"]);
        let upstream = output(repo, &["rev-parse", "refs/remotes/origin/main"]);
        // Dirty: staged edit, unstaged edit, untracked, ignored.
        std::fs::write(repo.join(".gitignore"), "ignored.log\n").unwrap();
        git(repo, &["add", ".gitignore"]);
        git(repo, &["commit", "-q", "-m", "ignore rules"]);
        let before_tip = output(repo, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("README.md"), "# fixture\nstaged\n").unwrap();
        git(repo, &["add", "README.md"]);
        std::fs::write(repo.join("README.md"), "# fixture\nstaged then edited\n").unwrap();
        std::fs::write(repo.join("untracked.txt"), "keep me in backup\n").unwrap();
        std::fs::write(repo.join("ignored.log"), "never touched\n").unwrap();
        let before_status = output(repo, &["status", "--porcelain"]);

        let (app, window) = mount(cx, repo);
        app.update(cx, |app, _| {
            app.open_branch_plan_modal("main".to_string(), kagi::ui::BranchPlanKind::SyncToRemote)
        });
        wait_idle(cx, &app);
        cx.read(|cx| {
            let modal = app.read(cx).branch_plan_modal().expect("sync modal");
            assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
            assert!(modal.plan.destructive);
            assert!(!modal.confirm_armed);
            let recovery = modal.plan.recovery.as_ref().expect("recovery");
            assert_eq!(recovery.commands.len(), 2, "{:?}", recovery.commands);
        });
        // #354: the sync card is announced as a two-stage alert dialog.
        redraw(cx, &app, window);
        let dialog = kagi::ui::dialog_a11y::recorded_dialog("plan-card").expect("sync card drawn");
        assert_eq!(dialog.role, gpui::Role::AlertDialog);
        let recovery = cx.read(|cx| {
            kagi_ui_core::i18n::plan_recovery_text(
                app.read(cx)
                    .branch_plan_modal()
                    .unwrap()
                    .plan
                    .recovery
                    .as_ref(),
            )
        });
        assert_eq!(
            dialog.description.as_deref(),
            Some(
                format!(
                    "{}\n{recovery}",
                    kagi_ui_core::i18n::Msg::A11yDialogTwoStage.t()
                )
                .as_str()
            )
        );
        assert_eq!(
            output(repo, &["status", "--porcelain"]),
            before_status,
            "planning wrote nothing"
        );

        confirm_branch_delete(cx, &app, window, input);
        cx.run_until_parked();
        assert!(
            cx.read(|cx| app.read(cx).branch_plan_modal().unwrap().confirm_armed),
            "{input}: first confirm arms"
        );
        assert_eq!(output(repo, &["rev-parse", "HEAD"]), before_tip);
        assert!(
            records(repo, "sync-to-remote").is_empty(),
            "arming must not record or execute"
        );

        confirm_branch_delete(cx, &app, window, input);
        wait_idle(cx, &app);
        assert!(cx.read(|cx| app.read(cx).branch_plan_modal().is_none()));
        assert_eq!(
            output(repo, &["rev-parse", "refs/heads/main"]),
            upstream,
            "{input}: branch == upstream"
        );
        assert_eq!(output(repo, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        // Clean except the file that *was* ignored by the now-gone local
        // .gitignore commit — untouched on disk.
        assert_eq!(output(repo, &["status", "--porcelain"]), "?? ignored.log");
        assert_eq!(
            std::fs::read_to_string(repo.join("ignored.log")).unwrap(),
            "never touched\n"
        );
        assert!(
            !repo.join("untracked.txt").exists(),
            "retained untracked removed"
        );
        let entries = records(repo, "sync-to-remote");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(
            entries[0].backup_refs.len(),
            2,
            "{:?}",
            entries[0].backup_refs
        );
        assert_eq!(
            output(repo, &["rev-parse", &entries[0].backup_refs[0]]),
            before_tip
        );
        assert_eq!(
            output(repo, &["cat-file", "-t", &entries[0].backup_refs[1]]),
            "commit",
            "work backup is a stash-shaped commit"
        );
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS sync_to_remote_armed: Enter/button arm then sync; branch==upstream, clean, 2 backups, oplog, ignored untouched");
}

/// #354 slice 3: the commit list is a ListBox whose drawn rows are
/// ListBoxOptions with absolute positions over WIP + stash + commit rows,
/// exactly one selected, named by subject / author / date / short SHA / refs —
/// and positions stay absolute after the virtualized list scrolls.
pub fn scenario_commit_list_roles(cx: &mut VisualTestAppContext) {
    use gpui::Role;
    use kagi::ui::list_a11y::{clear_recorded_lists, recorded_list};
    let fixture = build_fixture();
    let repo = fixture.path();
    for i in 0..200 {
        git(
            repo,
            &[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("filler {i:03}"),
            ],
        );
    }
    git(repo, &["tag", "v-mark", "HEAD~150"]);
    std::fs::write(repo.join("README.md"), "# stashed\n").unwrap();
    git(repo, &["stash", "push", "-q", "-m", "parked work"]);
    std::fs::write(repo.join("README.md"), "# dirty\n").unwrap();
    let (app, window) = mount(cx, repo);
    wait_idle(cx, &app);

    let redraw = |cx: &mut VisualTestAppContext| {
        clear_recorded_lists();
        app.update(cx, |_, cx| cx.notify());
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    };
    redraw(cx);
    let rows_len = cx.read(|cx| app.read(cx).view().rows.len());
    let list = recorded_list("commit-list").expect("commit list drawn");
    assert_eq!(list.role, Some(Role::ListBox));
    assert_eq!(list.label, kagi_ui_core::i18n::Msg::A11yCommitList.t());
    // WIP (1) + stash (1) + commits.
    assert_eq!(
        list.size,
        2 + rows_len,
        "{:?}",
        list.rows.keys().take(5).collect::<Vec<_>>()
    );
    let (wip, _) = &list.rows[&0];
    assert!(wip.contains("main"), "WIP row named by its worktree: {wip}");
    let (stash, _) = &list.rows[&1];
    assert!(stash.contains("parked work"), "{stash}");
    assert!(
        list.rows.keys().max().copied().unwrap() < 60,
        "virtualized: only visible rows drawn"
    );

    // Jump far down: the selected row is drawn, alone selected, at its
    // absolute position, named by its own subject and SHA.
    app.update(cx, |app, _| {
        if app.ui().selected.is_none() {
            app.step_commit_selection(1);
        }
        app.step_commit_selection(150);
    });
    cx.run_until_parked();
    redraw(cx);
    let (sel_ix, summary, short, author, date, date_short, tagged) = cx.read(|cx| {
        let app = app.read(cx);
        let ix = app.ui().selected.expect("selected");
        let row = &app.view().rows[ix];
        (
            ix,
            row.summary.to_string(),
            row.short_id.to_string(),
            row.author.to_string(),
            row.date.to_string(),
            row.date_short.to_string(),
            row.badges.iter().any(|b| b.label.as_ref() == "v-mark"),
        )
    });
    let list = recorded_list("commit-list").expect("commit list drawn after scroll");
    let selected: Vec<_> = list.rows.iter().filter(|(_, (_, s))| *s).collect();
    assert_eq!(selected.len(), 1, "exactly one selected row: {selected:?}");
    let (pos, (label, _)) = selected[0];
    assert_eq!(*pos, 2 + sel_ix, "absolute position after scrolling");
    assert!(*pos > 60, "the list really scrolled");
    assert!(
        label.contains(&summary) && label.contains(&short),
        "{label}"
    );
    assert!(label.contains(&author), "full author missing: {label}");
    assert_ne!(
        date, date_short,
        "the accessible label retains the verbose age"
    );
    assert!(label.contains(&date), "verbose author age missing: {label}");
    if tagged {
        assert!(label.contains("v-mark"), "refs named: {label}");
    }
    // uniform_list also lays out item 0 to measure the row height; apart from
    // that, nothing near the top is drawn any more.
    assert!(
        list.rows
            .keys()
            .filter(|p| **p != 0)
            .min()
            .copied()
            .unwrap()
            > 2,
        "rows above are no longer drawn: {:?}",
        list.rows.keys().take(4).collect::<Vec<_>>()
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_list_roles: ListBox + ListBoxOption, absolute positions over WIP/stash/commits, one selected, labels, virtualized scroll");
}

/// #354/#864: each of the five Graph sections (LOCAL, REMOTE, WORKTREES,
/// TAGS, STASHES) is its own Tree with a pinned section TreeItem; pull
/// requests live only in the PRs tab. Group and leaf positions remain stable
/// inside their pane, and collapsing a section hides only its leaves.
pub fn scenario_sidebar_tree_roles(cx: &mut VisualTestAppContext) {
    use gpui::Role;
    use kagi::ui::list_a11y::{clear_recorded_lists, recorded_list};
    let fixture = build_fixture();
    let repo = fixture.path();
    git(repo, &["branch", "feat/a"]);
    git(repo, &["branch", "feat/b"]);
    git(repo, &["tag", "v1"]);
    let wt_dir = tempfile::tempdir().unwrap();
    let wt = wt_dir.path().join("wt-tree");
    git(
        repo,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    let (app, window) = mount(cx, repo);
    wait_idle(cx, &app);
    let redraw = |cx: &mut VisualTestAppContext| {
        clear_recorded_lists();
        app.update(cx, |_, cx| cx.notify());
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    };
    redraw(cx);
    let trees = [
        ("sidebar-local", kagi_ui_core::i18n::Msg::A11ySidebarLocal),
        ("sidebar-remote", kagi_ui_core::i18n::Msg::A11ySidebarRemote),
        (
            "sidebar-worktrees",
            kagi_ui_core::i18n::Msg::A11ySidebarWorktrees,
        ),
        ("sidebar-tags", kagi_ui_core::i18n::Msg::A11ySidebarTags),
        (
            "sidebar-stashes",
            kagi_ui_core::i18n::Msg::A11ySidebarStashes,
        ),
    ];
    for (id, label) in trees {
        let tree = recorded_list(id).unwrap_or_else(|| panic!("{id} drawn"));
        assert_eq!(tree.role, Some(Role::Tree));
        assert_eq!(tree.label, label.t());
        assert!(
            !tree
                .rows
                .values()
                .any(|(row, _)| row.contains("PULL REQUESTS")),
            "{id} carries no pull request section: {tree:?}"
        );
    }
    assert!(
        recorded_list("sidebar-prs").is_none(),
        "the Graph sidebar draws no PULL REQUESTS pane"
    );
    let list = recorded_list("sidebar-local").expect("local branches drawn");
    let find = |list: &kagi::ui::list_a11y::RecordedList, needle: &str| {
        list.rows
            .iter()
            .find(|(_, (label, _))| label.contains(needle))
            .map(|(ix, (label, _))| (*ix, label.clone(), list.tree[ix]))
            .unwrap_or_else(|| panic!("no row naming {needle}: {:?}", list.rows))
    };
    // Section header: level 1, expanded.
    let (_, _, (level, expanded, ..)) = find(&list, "LOCAL BRANCHES");
    assert_eq!((level, expanded), (1, Some(true)));
    // The current branch is level 2 and named as current.
    let (_, main_label, (level, expanded, ..)) = find(&list, "main");
    assert_eq!((level, expanded), (2, None));
    let current = kagi_ui_core::i18n::Msg::A11ySidebarCurrentBranch
        .t()
        .replace("{}", "main");
    assert_eq!(main_label, current);
    // Grouped leaves under feat/: level 3, positions 1 and 2 of 2.
    let (_, _, a) = find(&list, "feat/a");
    let (_, _, b) = find(&list, "feat/b");
    assert_eq!((a.0, a.2, a.3), (3, 1, 2));
    assert_eq!((b.0, b.2, b.3), (3, 2, 2));
    // Worktree leaves belong to their own independently named Tree.
    let worktrees = recorded_list("sidebar-worktrees").expect("worktree pane drawn");
    let (_, wt_label, (level, ..)) = find(&worktrees, "wt-tree");
    assert_eq!(level, 2, "{wt_label}");
    assert_eq!(
        worktrees.rows.len(),
        2,
        "WORKTREES contains its header and linked row, not the main worktree"
    );
    let linked_count = cx.read(|cx| {
        app.read(cx).sidebar.rows.iter().find_map(|row| match row {
            kagi::ui::sidebar::SidebarRow::SectionHeader { section, count, .. }
                if *section == kagi::ui::sidebar::SECTION_WORKTREES =>
            {
                Some(*count)
            }
            _ => None,
        })
    });
    assert_eq!(linked_count, Some(1), "header counts linked worktrees only");

    // Collapse LOCAL BRANCHES: header reports collapsed, leaves disappear.
    app.update(cx, |app, cx| {
        app.sidebar
            .collapsed
            .insert(kagi::ui::sidebar::SECTION_LOCAL);
        cx.notify();
    });
    cx.run_until_parked();
    redraw(cx);
    let list = recorded_list("sidebar-local").expect("local branches drawn after collapse");
    let (_, _, (_, expanded, ..)) = find(&list, "LOCAL BRANCHES");
    assert_eq!(expanded, Some(false));
    assert!(
        !list.rows.values().any(|(l, _)| l.contains("feat/a")),
        "collapsed leaves are gone"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS sidebar_tree_roles: five named Trees without a PR pane, TreeItem levels, sibling positions, expanded, current named, collapse");
}

/// #354 slice 3 (PR list): the PR triage table is a `List` (rows open the PR
/// on click; the table keeps no selection) whose rows are `ListItem`s named
/// by number, title, state, author, branches, checks and age, positioned in
/// the filtered order.
pub fn scenario_pr_list_roles(cx: &mut VisualTestAppContext) {
    use crate::evidence_support::pull_request;
    use gpui::Role;
    use kagi::ui::list_a11y::{clear_recorded_lists, recorded_list};
    if !kagi_git::github::gh_available() {
        eprintln!("[gui-e2e] SKIP pr_list_roles: gh not available");
        return;
    }
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let prs = vec![
        kagi_domain::github::PullRequest {
            updated_at: "2026-09-03T00:00:00Z".into(),
            ..pull_request(9, "repair the thing", "main")
        },
        kagi_domain::github::PullRequest {
            updated_at: "2026-09-02T00:00:00Z".into(),
            ..pull_request(8, "documentation", "main")
        },
        kagi_domain::github::PullRequest {
            updated_at: "2026-09-01T00:00:00Z".into(),
            ..pull_request(7, "cached", "main")
        },
    ];
    kagi::ui::e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(crate::evidence_support::pr_page(
        prs, "", None,
    ))));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let list = recorded_list("pr-list").expect("PR list drawn");
    assert_eq!(list.role, Some(Role::List));
    assert_eq!(list.label, kagi_ui_core::i18n::Msg::A11yPrList.t());
    assert_eq!(list.size, 3);
    let labels: Vec<&str> = list.rows.values().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels.len(), 3, "{labels:?}");
    // Updated-desc order: #9, #8, #7 at positions 0, 1, 2.
    for (pos, (number, title)) in [(9, "repair the thing"), (8, "documentation"), (7, "cached")]
        .iter()
        .enumerate()
    {
        let (label, selected) = &list.rows[&pos];
        assert!(
            label.contains(&format!("#{number}")) && label.contains(title),
            "{pos}: {label}"
        );
        assert!(
            label.contains("@alice") && label.contains("main"),
            "{label}"
        );
        assert!(!selected, "the table keeps no selection");
    }
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_list_roles: List + ListItem, named rows in filtered order, positions/size");
}
