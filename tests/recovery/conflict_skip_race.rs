//! Execute-time skip refusal must preserve the replacement step and localize it.
use super::{git_succeeds, wait_idle};
use crate::macos::{git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{i18n, FooterStatus, KagiApp, ToastKind};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

struct RestoreLanguage(i18n::Lang);
impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        i18n::set_lang(self.0);
    }
}

fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.name", "Kagi Test"]);
    git(repo, &["config", "user.email", "kagi@example.test"]);
    std::fs::write(repo.join("file.txt"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "side"]);
    for step in ["one", "two"] {
        std::fs::write(repo.join("file.txt"), format!("side {step}\n")).unwrap();
        git(repo, &["commit", "-qam", step]);
    }
    git(repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("file.txt"), "main\n").unwrap();
    git(repo, &["commit", "-qam", "main"]);
    git(repo, &["checkout", "-q", "side"]);
    assert!(!git_succeeds(repo, &["rebase", "main"]));
    fixture
}

pub(super) fn run(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let _language = RestoreLanguage(i18n::lang());
    for language in [i18n::Lang::Ja, i18n::Lang::En] {
        for active in [true, false] {
            i18n::set_lang(language);
            let fixture = fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_window_active_for_e2e(active, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            let pane = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
            pane.update(cx, |view, _| assert!(!view.skip_request_arm()));
            let (hold, release) = crate::evidence_support::deferred::<()>(cx);
            KagiApp::hold_next_conflict_skip_for_e2e(hold);
            app.update(cx, |app, cx| {
                let owner = app
                    .app_sessions
                    .attachment(app.active_session().unwrap())
                    .unwrap();
                app.conflict_skip(owner, cx);
            });
            cx.run_until_parked();
            assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
            // Planning/second confirmation finished; only backend execution is held.
            assert!(!git_succeeds(&repo, &["rebase", "--skip"]));
            let head = std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap();
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            let worktree = std::fs::read(repo.join("file.txt")).unwrap();
            assert_eq!(
                std::fs::read_to_string(repo.join(".git/rebase-merge/msgnum"))
                    .unwrap()
                    .trim(),
                "2"
            );
            release.send(());
            wait_idle(cx, &app);
            let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "rebase-skip")
                .collect();
            assert_eq!(receipts.len(), 1);
            let english = "The operation moved to another step; review the current state";
            assert!(
                matches!(&receipts[0].outcome, OpOutcome::Refused { blockers } if blockers == &[english])
            );
            assert_eq!(std::fs::read(repo.join(".git/REBASE_HEAD")).unwrap(), head);
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
            let expected = match language {
                i18n::Lang::Ja => "別の step に進んでいます。最新の状態を確認してください",
                i18n::Lang::En => english,
            };
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(!state.app_sessions.has_leases());
                assert!(state.app_notice().is_none());
                let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                if active {
                    let FooterStatus::Failed(message) = &state.status_footer else {
                        panic!("execution refusal must be visible in the active footer");
                    };
                    assert!(message.contains(expected), "{language:?}: {message}");
                    assert!(
                        toasts.is_empty(),
                        "active refusal has one footer, no duplicate toast"
                    );
                } else {
                    assert_eq!(toasts.len(), 1);
                    assert_eq!(toasts[0].kind, ToastKind::Error);
                    assert!(
                        toasts[0].message.contains(expected),
                        "{language:?}: {}",
                        toasts[0].message
                    );
                }
            });
            drop(pane);
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS conflict_skip_step_race EN/JA footer/toast and preserved step 2");
}
