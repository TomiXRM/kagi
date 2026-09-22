//! An unobserved remote result needs an armed, audited release, never dismissal.
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use crate::recovery_operations::press_key;
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::{
    app,
    ui::{
        e2e,
        i18n::{self, Lang, Msg},
        KagiApp,
    },
};
use kagi_git::{
    backend::recording,
    oplog::{read_oplog_tail_for_repo, OpLogEntry, OpOutcome},
    Backend, GitError, StateSummary, Termination,
};
use std::{path::Path, sync::Arc};

fn park_unobservable(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
) -> app::OperationId {
    let backend = Backend::open(repo).unwrap();
    let plan = backend.plan_push_branch("main", false).unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    let before = plan.current.clone();
    let path = repo.to_path_buf();
    app.update(cx, |view, cx| {
        let owner = view
            .app_sessions
            .attachment(view.active_session().unwrap())
            .unwrap();
        let approved = app::approve_run(
            &mut view.app_sessions,
            app::RunRequest {
                owner,
                name: "push",
                path: path.clone(),
                repo: repo_id,
                plan: Arc::new(plan),
                remote: Vec::new(),
            },
        )
        .unwrap();
        let job = app::prepare_run(
            &mut view.app_sessions,
            approved,
            Box::new(move || {
                Ok(recording::RunReport {
                    result: Err(GitError::TerminationUnknown(Termination::stopped(
                        "reply unavailable",
                    ))),
                    recording: recording::finalize(OpLogEntry::new(
                        "push",
                        path.display().to_string(),
                        before,
                        OpOutcome::Unknown {
                            after: StateSummary {
                                head: "unobserved".into(),
                                dirty: "unobserved".into(),
                            },
                            evidence: "reply unavailable".into(),
                        },
                    )),
                    stash: None,
                })
            }),
        )
        .unwrap();
        let id = job.id();
        app::apply(&mut view.app_sessions, job.run());
        e2e::deliver_reconcile_notice(view, id);
        cx.notify();
        id
    })
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "app-notice-release-warning");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

fn click_confirm(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "app-notice-confirm");
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), "app-notice-confirm")
        .expect("the explicit notice action must be drawn");
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    paint(cx, window);
}

fn assert_blocked(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    id: app::OperationId,
) {
    app.update(cx, |view, _| {
        assert!(view.app_sessions.needs_reconcile(id));
        assert!(view.app_sessions.write_lease(repo).is_err());
    });
    assert!(
        !read_oplog_tail_for_repo(repo, 100)
            .iter()
            .any(|entry| entry.op == "reconcile-release-unobservable"),
        "arming or cancelling must not record a release"
    );
}

pub fn scenario_reconcile_unobservable_release(cx: &mut VisualTestAppContext) {
    let old_lang = i18n::lang();
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        let fixture = build_fixture();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "--bare", "-q"]);
        git(
            fixture.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let repo = fixture.path().canonicalize().unwrap();
        let before = repo_fingerprint(&repo);
        let (view, window) = mount(cx, &repo);
        let id = park_unobservable(cx, &view, &repo);
        click_confirm(cx, window); // Inspect, not release.
        cx.read(|cx| {
            let notice = view
                .read(cx)
                .app_notice()
                .expect("the read reaches the notice");
            assert!(notice.message.contains(Msg::AppReconcileUnobservable.t()));
            assert!(notice
                .acknowledge
                .as_ref()
                .unwrap()
                .can_acknowledge_unobserved());
        });
        assert_blocked(cx, &view, &repo, id);
        click_confirm(cx, window); // First release click only arms.
        assert_blocked(cx, &view, &repo, id);
        assert!(
            e2e::control_bounds(window.window_id(), "app-notice-release-warning").is_some(),
            "first release click must draw the final warning"
        );
        press_key(cx, &view, window, "escape");
        paint(cx, window);
        assert_blocked(cx, &view, &repo, id);
        assert!(
            e2e::control_bounds(window.window_id(), "app-notice-release-warning").is_none(),
            "Escape must remove the armed warning"
        );
        click_confirm(cx, window); // Escape discarded the previous arm.
        assert_blocked(cx, &view, &repo, id);
        view.update(cx, |view, cx| view.open_remote_browse(cx));
        press_key(cx, &view, window, "escape");
        paint(cx, window);
        click_confirm(cx, window); // Modal replacement discarded it too.
        assert_blocked(cx, &view, &repo, id);
        assert!(
            e2e::control_bounds(window.window_id(), "app-notice-release-warning").is_some(),
            "the displaced notice must require arming again"
        );
        click_confirm(cx, window); // Explicit final confirmation persists then releases.
        view.update(cx, |view, _| {
            assert!(
                !view.app_sessions.needs_reconcile(id),
                "release remained blocked; notice={:?}",
                view.app_notice().map(|notice| &notice.message)
            );
            view.app_sessions
                .write_lease(&repo)
                .expect("a new plan can acquire the scope")
                .complete();
        });
        let records = read_oplog_tail_for_repo(&repo, 100);
        assert_eq!(
            records
                .iter()
                .filter(|entry| entry.op == "reconcile-release-unobservable")
                .count(),
            1
        );
        assert!(
            records
                .iter()
                .any(|entry| entry.op == "push"
                    && matches!(entry.outcome, OpOutcome::Unknown { .. })),
            "releasing the block must not rewrite the original unknown result"
        );
        assert_eq!(
            repo_fingerprint(&repo),
            before,
            "acknowledgement is not a repository write"
        );
        unmount(cx, view, window);
    }
    i18n::set_lang(old_lang);
    eprintln!("[gui-e2e] PASS reconcile_unobservable_release");
}
