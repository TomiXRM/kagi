//! #1699: a real merge/rebase conflict must never announce completion.
use crate::macos::{git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{FooterStatus, KagiApp, ToastKind};
use kagi_ui_core::i18n::{self, Lang, Msg};
use std::time::{Duration, Instant};

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

fn stopped_matrix(cx: &mut VisualTestAppContext, rebase: bool) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let language = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        for count in [1, 3] {
            let fixture = fixture(count);
            let repo = fixture.path().canonicalize().unwrap();
            i18n::set_lang(lang);
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                if rebase {
                    app.open_rebase_modal("feature".into(), cx);
                } else {
                    app.open_merge_modal("feature".into(), None, cx);
                }
            });
            wait_for(cx, &app, |app| app.planning.is_none());
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
            cx.read(|cx| {
                let app = app.read(cx);
                let conflict = app.ui().conflict.as_ref().unwrap().read(cx);
                let mode = conflict.mode.as_ref().unwrap();
                assert_eq!(mode.conflicted_count(), observed);
                assert!(!mode.can_continue(), "unresolved files disable Continue");
                let expected = Msg::conflict_stopped(Some(observed));
                assert!(matches!(&app.status_footer,
                    FooterStatus::Idle(text) if text.as_ref() == expected.as_str()),
                    "a conflict stop must not leave a successful footer");
                let stack = app.toast_stack.as_ref().unwrap().read(cx);
                assert!(stack.toasts().iter().any(|toast| {
                    toast.kind == ToastKind::Info && toast.message.as_ref() == expected.as_str()
                }), "localized stopped notice must report the actual conflict count");
                assert!(stack.toasts().iter().all(|toast| toast.kind != ToastKind::Success),
                    "unresolved conflicts and disabled Continue cannot coexist with a green completion toast");
            });
            let operation = if rebase { "rebase" } else { "merge" };
            let receipts: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == operation)
                .collect();
            assert_eq!(
                receipts.len(),
                1,
                "presentation must not duplicate receipts"
            );
            assert!(
                !cx.read(|cx| app.read(cx).app_sessions.has_leases()),
                "the existing known-outcome settlement still releases its lease"
            );
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(language);
}

pub fn scenario_conflict_merge_stopped_toast(cx: &mut VisualTestAppContext) {
    stopped_matrix(cx, false);
}

pub fn scenario_conflict_rebase_stopped_toast(cx: &mut VisualTestAppContext) {
    stopped_matrix(cx, true);
}
