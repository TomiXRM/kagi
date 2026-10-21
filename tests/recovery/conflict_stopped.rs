//! #1699: a real merge/rebase conflict must never announce completion.
use crate::macos::{git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{FooterStatus, KagiApp, ToastKind};
use kagi_ui_core::i18n::{self, Lang};
use std::time::{Duration, Instant};

#[path = "../support/oplog.rs"]
mod oplog;

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
            assert!(stack.toasts().is_empty(),
                "required recording acknowledgment is the only result presentation");
            assert!(matches!(&app.status_footer,
                FooterStatus::Idle(text) if text.is_empty()));
        } else {
            assert!(matches!(&app.status_footer, FooterStatus::Idle(_)),
                "unresolved conflicts must not leave a successful footer");
            let notices: Vec<_> = stack.toasts().iter()
                .filter(|toast| toast.kind == ToastKind::Info)
                .collect();
            assert_eq!(notices.len(), 1);
            assert!(notices[0].message.contains(&observed.to_string()),
                "the stopped result must report the actual unresolved-file count");
            assert!(stack.toasts().iter().all(|toast| toast.kind != ToastKind::Success),
                "unresolved conflicts and disabled Continue cannot coexist with green completion");
        }
    });
}

fn assert_recording_failure(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &std::path::Path,
    operation: &str,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app
            .app_notice()
            .expect("failed append must be immediately visible");
        assert!(notice.message.contains(&repo.display().to_string()));
        assert!(!notice.message.trim().is_empty());
        assert!(app.toast_stack.as_ref().unwrap().read(cx).toasts().is_empty(),
            "a failed append must not add a stopped or error toast beside its acknowledgment");
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let entries: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == operation)
            .collect();
        assert_eq!(entries.len(), 1, "no duplicate in-memory receipts");
        assert!(matches!(&entries[0].outcome,
            kagi_git::oplog::OpOutcome::Partial { error, .. } if !error.is_empty()));
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
