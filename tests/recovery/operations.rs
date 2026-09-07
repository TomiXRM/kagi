//! Real UI → backend → durable-record scenarios; no mocked mutation results.
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, Focusable, VisualTestAppContext};
use kagi::ui::{modals::ActiveModal, CheckoutSelected, FooterStatus, KagiApp};
use kagi_domain::branch_cleanup::{CleanupDeleteTarget, MergedBranchStatus};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry, OpOutcome};
use kagi_git::{CommitId, OperationKind};

use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(result.status.success(), "git {args:?}: {:?}", result.stderr);
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).busy_op.is_none()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "mutation did not release busy state"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn press_key(
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

fn press_enter(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    press_key(cx, app, window, "enter");
}

fn dispatch_checkout_selected(
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
        app.operation_history = Default::default();
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
        let expected = i18n::op_failed(Op::Preflight, error);
        cx.run_until_parked();
        // Local stash now shares the root Enter/button approval boundary.
        app.update(cx, |app, cx| app.start_stash_drop(cx));
        wait_idle(cx, &app);
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                kagi::ui::e2e::app_notice_message(app)
                    .is_some_and(|message| message.ends_with(&expected)),
                "localized preflight refusal must reach the shared error modal"
            );
        });
        assert_eq!(output(repo, &["stash", "list"]), before);
        let entries = records(repo, "stash-drop");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Refused { .. }));
        unmount(cx, app, window);

        let fixture = build_fixture();
        let repo = fixture.path();
        let before = output(repo, &["rev-parse", "HEAD~1"]);
        let after = output(repo, &["rev-parse", "HEAD"]);
        let (app, window) = mount(cx, repo);
        let plan = app.update(cx, |app, _| {
            app.operation_history = Default::default();
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
        let entries = records(repo, "branch-cleanup");
        assert_eq!(
            entries.len(),
            1,
            "opening failure must persist before the UI ownership guard"
        );
        assert!(matches!(entries[0].outcome, OpOutcome::Failed { .. }));
        cx.read(|cx| {
            let app = app.read(cx);
            if switch_away {
                assert_eq!(app.active_tab, 1);
                assert!(app.branch_cleanup_modal().is_none());
            } else {
                assert!(app.branch_cleanup_modal().unwrap().error.is_some());
                assert!(matches!(app.status_footer, FooterStatus::Failed(_)));
            }
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS cleanup_open_failure active/stale durable refusal without mutation");
}

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
        assert!(matches!(&app.status_footer, FooterStatus::Failed(_)));
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
        cx.read(|cx| app.read(cx).selected),
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
                app.busy_op.is_none(),
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
        assert!(app.busy_op.is_none());
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
    assert_eq!(cx.read(|cx| app.read(cx).selected), Some(1));

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
        assert_eq!(app.selected, Some(1), "the menu must cover a non-HEAD row");
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

/// #493 safety review: a failed push must reach the **modal** as well as the
/// oplog and the footer (CLAUDE.md's error rule). `start_push` closes the plan
/// modal before the background push, so the completion path has to bring it
/// back with the error — the same thing `start_checkout` / `start_delete_branch`
/// / `start_amend` / the remote-view pull arm already do. Enter and the button
/// share this one completion path, so driving it once covers both.
///
/// The failure is offline and deterministic: push to a bare remote, delete the
/// remote, then commit — the upstream ref still resolves (so the plan is clean
/// and never touches the network) but `git push` cannot find the repository.
pub fn scenario_push_failure_keeps_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_dir = tempfile::tempdir().expect("remote tempdir");
    let bare = remote_dir.path().join("target.git");
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
    wait_idle(cx, &app);
    cx.read(|cx| {
        let app = app.read(cx);
        let modal = app
            .push_modal()
            .expect("a failed push must re-open the plan modal, not only record the failure");
        assert!(
            modal.error.is_some(),
            "the failure text must be shown in the modal"
        );
        assert!(app.busy_op.is_none(), "busy must be released on failure");
    });
    assert!(
        records(repo, "push")
            .iter()
            .any(|e| matches!(e.outcome, OpOutcome::Failed { .. })),
        "the failure must also be durable in the oplog"
    );
    assert_eq!(
        repo_fingerprint(repo),
        before,
        "a failed push must not touch the repository"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS push_failure_keeps_modal: failure reaches the modal and the oplog");
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .unwrap();
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
            assert!(app.busy_op.is_none());
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
        assert_eq!(entry.repo, repo.display().to_string());
        assert_eq!(
            entry.backup_refs.len(),
            1,
            "Partial presentation must retain the attempted recovery root"
        );
        assert_eq!(output(repo, &["rev-parse", &entry.backup_refs[0]]), tip);
    });
    unmount(cx, app, window);
}
