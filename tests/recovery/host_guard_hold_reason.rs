//! #1614: a released recovery requirement must not name another operation's hold.
use crate::host_dirty_guard::{
    alive, assert_failure_visible, cleanup, draw, edit, fixture, mount, press,
};
use crate::macos::git;
use gpui::{Entity, VisualTestAppContext};
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
    github_repos::RepoListing,
    oplog::{OpLogEntry, OpOutcome},
    Backend, GitError, StateSummary, Termination,
};
use std::{path::Path, sync::Arc};

/// Settle through the application boundary: a proven stop releases the lease,
/// but the unobserved push outcome still requires reconciliation in repo A.
fn park_released_recovery(
    cx: &mut VisualTestAppContext,
    view: &Entity<KagiApp>,
    repo: &Path,
) -> app::OperationId {
    let backend = Backend::open(repo).unwrap();
    let plan = backend.plan_push_branch("main", false).unwrap();
    let repo_id = backend.write_repo_id().unwrap();
    let before = plan.current.clone();
    let path = repo.to_path_buf();
    view.update(cx, |view, cx| {
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
                        "push reply unavailable after the writer stopped",
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
                            evidence: "push reply unavailable after the writer stopped".into(),
                        },
                    )),
                    stash: None,
                })
            }),
        )
        .unwrap();
        let id = job.id();
        app::apply(&mut view.app_sessions, job.run());
        assert!(view.app_sessions.needs_reconcile(id));
        assert!(view.app_sessions.running_lease().is_none());
        assert!(
            !view.app_sessions.has_leases(),
            "stopped Unknown must retain reconciliation without a writer lease"
        );
        assert!(
            view.may_close_host(),
            "lease-free recovery alone permits host exit"
        );
        cx.notify();
        id
    })
}

pub fn scenario_host_guard_released_recovery_running(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    cx.update(|cx| {
        cx.set_global(e2e::QuitForE2e::default());
        cx.set_global(e2e::RelaunchForE2e::default());
    });
    for lang in [Lang::En, Lang::Ja] {
        i18n::set_lang(lang);
        for cloning in [false, true] {
            for choice in ["save", "discard", "restart"] {
                cx.update(|cx| {
                    cx.global_mut::<e2e::QuitForE2e>().requested = false;
                    cx.global_mut::<e2e::RelaunchForE2e>().requested = false;
                });
                let dir = tempfile::tempdir().unwrap();
                let repo_a = fixture(dir.path(), "released-recovery-a");
                let repo_b = fixture(dir.path(), "running-b");
                let remote = dir.path().join("origin.git");
                std::fs::create_dir_all(&remote).unwrap();
                git(&remote, &["init", "--bare", "-q"]);
                git(
                    &repo_a,
                    &["remote", "add", "origin", remote.to_str().unwrap()],
                );
                let (view, window) = mount(cx, &repo_a, 1000.);
                let recovery = park_released_recovery(cx, &view, &repo_a);
                view.update(cx, |view, cx| {
                    assert!(view.open_repository(repo_b.clone(), cx));
                });
                cx.run_until_parked();
                let editor = (choice != "restart").then(|| {
                    edit(
                        cx,
                        &view,
                        window,
                        "f.txt",
                        "retain B until the writer stops\n",
                    )
                });

                // Exercise clone's separate host hold through its production
                // start/finish path, with only dispatch paused by the existing seam.
                let _gh = cloning.then(|| {
                    crate::pr_fields_focus::OfflineGh::with_script(&format!(
                        "#!/bin/sh\ncase \"$1 $2\" in\n'repo clone') git clone -q '{}' \"$4\" ;;\n*) echo '[]' ;;\nesac\n",
                        repo_b.display()
                    ))
                });
                let (writer, clone_reply) = if cloning {
                    let (hold, reply) = crate::evidence_support::deferred(cx);
                    e2e::queue_clone_run(hold);
                    view.update(cx, |view, cx| {
                        view.open_clone_card(
                            RepoListing {
                                name_with_owner: "acme/held-clone".into(),
                                host: "github.com".into(),
                                updated_at: String::new(),
                                description: String::new(),
                                is_private: false,
                                is_fork: false,
                            },
                            cx,
                        );
                        view.replan_clone(dir.path(), cx);
                        view.start_clone(cx);
                        assert!(view.home_github.cloning.is_some());
                        view.cancel_clone();
                    });
                    (None, Some(reply))
                } else {
                    let writer = view.update(cx, |view, _| {
                        view.app_sessions
                            .write_lease(&repo_b)
                            .unwrap()
                            .for_op("fetch")
                    });
                    (Some(writer), None)
                };
                cx.run_until_parked();
                cx.read(|cx| {
                    let view = view.read(cx);
                    assert!(view.app_sessions.needs_reconcile(recovery));
                    assert_eq!(view.app_sessions.has_leases(), !cloning);
                    assert_eq!(view.app_sessions.running_lease().is_some(), !cloning);
                    assert!(!view.may_close_host());
                });
                view.update(cx, |view, cx| {
                    view.request_update_relaunch_for_e2e(
                        kagi::update::Relaunch {
                            program: "kagi".into(),
                            args: Vec::new(),
                        },
                        cx,
                    );
                });
                let action = match choice {
                    "save" => Msg::EditorHostSaveAll.t(),
                    "discard" => Msg::EditorWorkspaceDiscard.t(),
                    _ => Msg::UpdateRestart.t(),
                };
                press(cx, window, action);
                assert_failure_visible(
                    cx,
                    &view,
                    if choice == "restart" {
                        Msg::UpdateRestartWaiting.t()
                    } else {
                        Msg::EditorHostSaveWaiting.t()
                    },
                );
                draw(cx, window);
                let tree = crate::native_ax::tree(cx, window);
                assert!(
                    !tree["nodes"].as_object().unwrap().values().any(|node| {
                        node["aria"]["label"].as_str() == Some(Msg::EditorHostRecoveryRequired.t())
                    }),
                    "released recovery in A is not the live host hold in B: {tree}"
                );
                assert!(alive(cx, window));
                cx.read(|cx| {
                    assert!(!cx.global::<e2e::QuitForE2e>().requested);
                    assert!(!cx.global::<e2e::RelaunchForE2e>().requested);
                    if let Some(editor) = editor.as_ref() {
                        let editor = editor.read(cx);
                        assert!(editor.any_dirty());
                        assert_eq!(
                            editor.editor.as_ref().unwrap().read(cx).value().as_str(),
                            "retain B until the writer stops\n"
                        );
                    }
                });
                assert_eq!(
                    std::fs::read_to_string(repo_b.join("f.txt")).unwrap(),
                    "original\n"
                );

                if let Some(writer) = writer {
                    writer.complete();
                }
                if let Some(reply) = clone_reply {
                    reply.send(());
                    crate::home_github::wait_for(cx, &view, "held clone completion", |view| {
                        view.home_github.cloning.is_none()
                    });
                }
                cx.run_until_parked();
                cx.read(|cx| {
                    let view = view.read(cx);
                    assert!(view.app_sessions.needs_reconcile(recovery));
                    assert!(!view.app_sessions.has_leases());
                    assert!(view.may_close_host());
                    assert!(view.editor_dirty_guard_modal().is_some());
                    assert!(
                        !cx.global::<e2e::RelaunchForE2e>().requested,
                        "completion alone must not resume the retained restart"
                    );
                });
                press(cx, window, action);
                cx.read(|cx| {
                    assert!(cx.global::<e2e::RelaunchForE2e>().requested);
                    assert!(!cx.global::<e2e::QuitForE2e>().requested);
                    assert!(view.read(cx).app_sessions.needs_reconcile(recovery));
                    if let Some(editor) = editor.as_ref() {
                        // Captured process exit does not actually drop the pane.
                        // Discard approves exit without saving or rewriting it.
                        assert_eq!(editor.read(cx).any_dirty(), choice != "save");
                    }
                });
                assert_eq!(
                    std::fs::read_to_string(repo_b.join("f.txt")).unwrap(),
                    if choice == "save" {
                        "retain B until the writer stops\n"
                    } else {
                        "original\n"
                    }
                );
                assert_eq!(
                    std::fs::read_to_string(repo_a.join("f.txt")).unwrap(),
                    "original\n"
                );
                drop(editor);
                cleanup(cx, view, window);
            }
        }
    }
    cx.update(|cx| {
        cx.remove_global::<e2e::RelaunchForE2e>();
        cx.remove_global::<e2e::QuitForE2e>();
    });
    eprintln!("[gui-e2e] PASS host_guard_released_recovery_running");
}
