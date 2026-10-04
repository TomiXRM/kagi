//! Dirty Pull GUI E2E scenarios for ADR-0189.

use std::path::Path;
use std::time::Duration;

use gpui::VisualTestAppContext;
use kagi_domain::plan_note::{PlanNote, PullNote};
use kagi_git::oplog::{read_oplog_tail_for_repo, recovery, OpOutcome};

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as output;

fn records(repo: &Path, op: &str) -> Vec<kagi_git::oplog::OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect()
}

fn assert_stash_push_recovery(repo: &Path) {
    let pushes = records(repo, "stash-push");
    assert_eq!(pushes.len(), 1, "auto-stash must be recorded exactly once");
    assert!(
        pushes[0]
            .recovery
            .iter()
            .any(|handle| handle.kind == recovery::STASH && handle.oid.len() == 40),
        "the created stash OID must be a typed recovery handle"
    );
}

pub fn scenario_pull_auto_stash_success(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let gitlink_path = "PCB/EM2/SM20/SteppingDriverBoard_L/.history";
    let cacheinfo = format!("160000,7489b69c1ec9e5763a469d9b367deac0aee76bc4,{gitlink_path}");
    git(repo, &["update-index", "--add", "--cacheinfo", &cacheinfo]);
    git(repo, &["commit", "-qm", "add unpopulated gitlink"]);
    std::fs::create_dir_all(repo.join(gitlink_path)).unwrap();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let upstream_head = output(&other, &["rev-parse", "HEAD"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "local untracked change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "fixture Pull must be executable"
        );
        assert!(
            modal.error.is_none(),
            "confirmation must not begin as an error"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    assert_eq!(output(repo, &["rev-parse", "HEAD"]), upstream_head);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "local staged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "local untracked change\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "Pull success must be durable"
    );
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_auto_stash_success: dirty changes restored after Pull");
}

/// A `git` child that leaves a descendant holding the pipes: the wait resolves,
/// but the capture is not proven complete — `run_git`'s `TerminationUnknown`.
/// `remote.<name>.uploadpack` is what `git fetch` runs for a local remote, and
/// the CLI hardening does not neutralise it, so this is the real path.
fn wait_for(
    cx: &mut VisualTestAppContext,
    mut done: impl FnMut(&mut VisualTestAppContext) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the completion never arrived"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Stand-in for the host's ordinary SSH options, scope probe and pull.
fn blocking_fake_ssh(
    bin: &Path,
    release: &Path,
    calls: &Path,
    fail: &Path,
    probe_release: &Path,
    identity_change: &Path,
    route_change: &Path,
    toplevel_change: &Path,
    branch_change: &Path,
    oid_change: &Path,
    upstream_change: &Path,
    head_read_failure: &Path,
    remote_url_change: &Path,
    dirty_change: &Path,
) {
    std::fs::create_dir_all(bin).expect("shim dir");
    let path = bin.join("ssh");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n\
             echo \"$*\" >> {calls:?}\n\
             if [ \"$1\" = '-G' ]; then\n\
               while [ ! -f {probe_release:?} ]; do sleep 0.05; done\n\
               printf 'hostname e2e.invalid\\nuser kagi-e2e\\nport 22\\nidentityfile none\\nuserknownhostsfile none\\nglobalknownhostsfile none\\n'\n\
               if [ -f {route_change:?} ]; then printf 'proxyjump hop.e2e\\n';\n\
               else printf 'proxyjump none\\n'; fi\n\
               exit 0\n\
             fi\n\
             case \"$*\" in\n\
               *KAGI-COMMON-DIR*) common=/srv/repo/.git; top=/srv/real-worktree\n\
                     branch=main; oid=$(printf '%040d' 0); upstream=origin/main\n\
                     url=ssh://e2e.invalid/repo\n\
                     if [ -f {identity_change:?} ]; then common=/srv/other/.git; fi\n\
                     if [ -f {toplevel_change:?} ]; then top=/srv/other-linked-worktree; fi\n\
                     if [ -f {branch_change:?} ]; then branch=feature; fi\n\
                     if [ -f {oid_change:?} ]; then oid=$(printf '%040d' 1); fi\n\
                     if [ -f {upstream_change:?} ]; then upstream=origin/feature; fi\n\
                     if [ -f {remote_url_change:?} ]; then url=ssh://e2e.invalid/changed; fi\n\
                     if [ -f {head_read_failure:?} ]; then echo 'HEAD query failed' >&2; exit 1; fi\n\
                     printf 'KAGI-COMMON-DIR\\0%s\\0%s\\0branch\\0%s\\0%s\\0%s\\0origin\\0refs/heads/main\\0%s\\0+refs/heads/*:refs/remotes/origin/*\\0KAGI-FETCH-END\\0KAGI-INDEX-BEGIN\\0' \"$common\" \"$top\" \"$branch\" \"$oid\" \"$upstream\" \"$url\"\n\
                     printf '100644 %s 0\\tfile\\0KAGI-INDEX-END\\0KAGI-WORKTREE-BEGIN\\0' \"$oid\"\n\
                     if [ -f {dirty_change:?} ]; then printf '? new-file\\0'; fi\n\
                     printf 'KAGI-WORKTREE-END\\0KAGI-END\\n' ;;\n\
               *pull*) while [ ! -f {release:?} ]; do sleep 0.05; done\n\
                       if [ -f {fail:?} ]; then echo 'unexpected remote reply' >&2; exit 42; fi\n\
                       echo 'Already up to date.' ;;\n\
               *) echo '' ;;\n\
             esac\n",
        ),
    )
    .expect("write fake ssh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn ssh_pulls(calls: &Path) -> usize {
    std::fs::read_to_string(calls)
        .map(|text| text.lines().filter(|line| line.contains("pull")).count())
        .unwrap_or(0)
}
fn ssh_probes(calls: &Path) -> usize {
    std::fs::read_to_string(calls)
        .map(|text| text.lines().filter(|line| line.starts_with("-G ")).count())
        .unwrap_or(0)
}

enum PullLeaseCase {
    Success,
    Unknown,
    IdentityChanged,
    ProxyRouteChanged,
    ToplevelChanged,
    BranchChanged,
    OidChanged,
    UpstreamChanged,
    HeadReadFailure,
    RemoteUrlChanged,
    DirtyChanged,
    CachedPreviewStale,
    PlanningLatch,
}

pub fn scenario_remote_pull_lease(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::Success);
}

pub fn scenario_remote_pull_unknown_release(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::Unknown);
}

pub fn scenario_remote_pull_preflight_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::IdentityChanged);
}

pub fn scenario_remote_pull_proxy_route_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::ProxyRouteChanged);
}

pub fn scenario_remote_pull_toplevel_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::ToplevelChanged);
}

pub fn scenario_remote_pull_branch_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::BranchChanged);
}

pub fn scenario_remote_pull_head_oid_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::OidChanged);
}

pub fn scenario_remote_pull_upstream_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::UpstreamChanged);
}

pub fn scenario_remote_pull_head_read_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::HeadReadFailure);
}
pub fn scenario_remote_pull_cached_preview_stale(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::CachedPreviewStale);
}
pub fn scenario_remote_pull_url_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::RemoteUrlChanged);
}
pub fn scenario_remote_pull_dirty_refusal(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::DirtyChanged);
}

pub fn scenario_remote_pull_planning_latch(cx: &mut VisualTestAppContext) {
    remote_pull_lease(cx, PullLeaseCase::PlanningLatch);
}

fn remote_pull_lease(cx: &mut VisualTestAppContext, case: PullLeaseCase) {
    use gpui::Modifiers;
    use kagi::ui::e2e;

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("fixture path");
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let (bare_path, other_path) = (bare.to_str().unwrap(), other.to_str().unwrap());
    git(&repo, &["init", "--bare", "-q", bare_path]);
    git(&repo, &["remote", "add", "origin", bare_path]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(&repo, &["fetch", "-q", "origin"]);

    let shim = tempfile::tempdir().expect("shim root");
    let release = shim.path().join("release");
    let fail = shim.path().join("fail");
    let calls = shim.path().join("calls");
    let probe_release = shim.path().join("probe-release");
    let identity_change = shim.path().join("identity-change");
    let route_change = shim.path().join("route-change");
    let toplevel_change = shim.path().join("toplevel-change");
    let branch_change = shim.path().join("branch-change");
    let oid_change = shim.path().join("oid-change");
    let upstream_change = shim.path().join("upstream-change");
    let head_read_failure = shim.path().join("head-read-failure");
    let remote_url_change = shim.path().join("remote-url-change");
    let dirty_change = shim.path().join("dirty-change");
    blocking_fake_ssh(
        shim.path(),
        &release,
        &calls,
        &fail,
        &probe_release,
        &identity_change,
        &route_change,
        &toplevel_change,
        &branch_change,
        &oid_change,
        &upstream_change,
        &head_read_failure,
        &remote_url_change,
        &dirty_change,
    );
    if !matches!(case, PullLeaseCase::PlanningLatch) {
        std::fs::write(&probe_release, b"go").unwrap();
    }
    if matches!(case, PullLeaseCase::CachedPreviewStale) {
        // The cached status says main; the first live SSH probe says feature.
        std::fs::write(&branch_change, b"switched before planning").unwrap();
    }
    let original_path = std::env::var_os("PATH");
    std::env::set_var(
        "PATH",
        format!(
            "{}:{}",
            shim.path().display(),
            original_path.clone().unwrap_or_default().to_string_lossy()
        ),
    );

    let snap = kagi_git::Backend::open(&repo)
        .expect("open fixture")
        .snapshot(10_000)
        .expect("snapshot");
    let host = kagi_domain::remote::RemoteHost {
        user: Some("kagi-e2e".into()),
        host: "e2e.invalid".into(),
        port: None,
        identity_file: None, // ssh-agent-only must pass identity resolution.
    };
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/repo".into(), snap, cx);
        app.open_pull_modal(cx);
        assert!(matches!(
            app.app_sessions.plan_state(),
            kagi::app::PlanState::Planning { .. }
        ));
        if matches!(case, PullLeaseCase::Success) {
            // An async identity result must not replace a newer user modal.
            app.set_pop_modal(kagi::ui::modals::PopPlanModal {
                stash_index: 0,
                plan: None,
                error: None,
            });
        }
    });
    let unblock_probe = if matches!(case, PullLeaseCase::PlanningLatch) {
        // The GPUI harness pumps background tasks only after this update. The
        // second click occurs while the first plan is queued; once pumped,
        // fake ssh stays in -G until this independent releaser sees its call.
        app.update(cx, |app, cx| {
            app.open_pull_modal(cx);
            assert!(
                matches!(&app.status_footer, kagi::ui::FooterStatus::Idle(text)
                    if text.as_ref() == kagi::ui::i18n::Msg::OpInProgress.t()),
                "a second pull must be refused during identity planning"
            );
            assert!(e2e::op_latched(app));
        });
        let calls = calls.clone();
        let probe_release = probe_release.clone();
        Some(std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while ssh_probes(&calls) == 0 && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let observed = ssh_probes(&calls) > 0;
            if observed {
                std::thread::sleep(Duration::from_millis(100));
            }
            std::fs::write(probe_release, b"go").unwrap();
            observed
        }))
    } else {
        None
    };
    if matches!(case, PullLeaseCase::Success) {
        wait_for(cx, |cx| {
            cx.read(|cx| {
                let state = app.read(cx);
                matches!(state.app_sessions.plan_state(), kagi::app::PlanState::Draft)
                    && state.pop_modal().is_some()
            })
        });
        app.update(cx, |app, cx| {
            assert!(app.pull_modal().is_none(), "stale plan replaced user modal");
            app.cancel_pop_modal();
            e2e::present_app_notice(app);
            assert!(
                app.app_notice().is_some(),
                "contended plan must offer a retry notice"
            );
            app.confirm_app_notice(cx);
            app.open_pull_modal(cx);
        });
    }
    if matches!(case, PullLeaseCase::CachedPreviewStale) {
        wait_for(cx, |cx| {
            cx.read(|cx| {
                !matches!(
                    app.read(cx).app_sessions.plan_state(),
                    kagi::app::PlanState::Planning { .. }
                )
            })
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                matches!(
                    state.app_sessions.plan_state(),
                    kagi::app::PlanState::Error {
                        blocker: Some(kagi_domain::plan_note::PlanNote::Pull(
                            kagi_domain::plan_note::PullNote::RemotePreviewStale
                        )),
                        ..
                    }
                ),
                "a cached main preview must be rejected when the live checkout is feature"
            );
            assert!(state.pull_modal().is_none(), "stale preview opened a modal");
            assert!(!state.app_sessions.has_leases());
            assert!(state.app_sessions.may_close_host());
            assert!(
                matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(text)
                if text.as_ref() == kagi::ui::i18n::plan_note_text(
                    &kagi_domain::plan_note::PlanNote::Pull(
                        kagi_domain::plan_note::PullNote::RemotePreviewStale
                    )
                )),
                "stale preview must explain the failure in the UI"
            );
        });
        assert_eq!(ssh_probes(&calls), 1);
        assert_eq!(
            ssh_pulls(&calls),
            0,
            "never pull from an unconfirmed checkout"
        );
        match original_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
        unmount(cx, app, window);
        eprintln!("[gui-e2e] PASS remote_pull_cached_preview_stale");
        return;
    }
    wait_for(cx, |cx| cx.read(|cx| app.read(cx).pull_modal().is_some()));
    if let Some(unblock_probe) = unblock_probe {
        assert!(
            unblock_probe.join().unwrap(),
            "fake ssh must enter the blocked -G probe"
        );
    }
    if matches!(case, PullLeaseCase::PlanningLatch) {
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                !e2e::op_latched(state),
                "planning latch must clear on Ready"
            );
        });
        assert_eq!(
            ssh_probes(&calls),
            1,
            "a retry must not start a second plan"
        );
    }
    if matches!(case, PullLeaseCase::IdentityChanged) {
        std::fs::write(&identity_change, b"different common dir").unwrap();
    }
    if matches!(case, PullLeaseCase::ProxyRouteChanged) {
        std::fs::write(&route_change, b"proxy route changed").unwrap();
    }
    if matches!(case, PullLeaseCase::ToplevelChanged) {
        std::fs::write(
            &toplevel_change,
            b"same repository, different linked worktree",
        )
        .unwrap();
    }
    if matches!(case, PullLeaseCase::BranchChanged) {
        std::fs::write(&branch_change, b"branch switched").unwrap();
    }
    if matches!(case, PullLeaseCase::OidChanged) {
        std::fs::write(&oid_change, b"HEAD advanced").unwrap();
    }
    if matches!(case, PullLeaseCase::UpstreamChanged) {
        std::fs::write(&upstream_change, b"upstream switched").unwrap();
    }
    if matches!(case, PullLeaseCase::HeadReadFailure) {
        std::fs::write(&head_read_failure, b"HEAD query failed").unwrap();
    }
    if matches!(case, PullLeaseCase::RemoteUrlChanged) {
        std::fs::write(&remote_url_change, b"effective URL changed").unwrap();
    }
    if matches!(case, PullLeaseCase::DirtyChanged) {
        std::fs::write(&dirty_change, b"worktree changed").unwrap();
    }
    let refused = matches!(
        case,
        PullLeaseCase::IdentityChanged
            | PullLeaseCase::ProxyRouteChanged
            | PullLeaseCase::ToplevelChanged
            | PullLeaseCase::BranchChanged
            | PullLeaseCase::OidChanged
            | PullLeaseCase::UpstreamChanged
            | PullLeaseCase::HeadReadFailure
            | PullLeaseCase::RemoteUrlChanged
            | PullLeaseCase::DirtyChanged
    );
    app.update(cx, |app, cx| {
        app.start_pull(cx);
        assert!(
            app.app_sessions.has_leases(),
            "pull must hold the remote write lease"
        );
        assert!(
            !app.app_sessions.may_close_host(),
            "quit must wait for remote pull"
        );
        assert!(e2e::op_latched(app), "the gate must read the lease");
        assert_eq!(app.write_busy_op, Some("pull"));
        assert_eq!(
            e2e::busy_snackbar_label(app),
            Some(kagi::ui::i18n::busy_label("pull"))
        );
        e2e::poll_app_jobs(app, cx);
        assert!(
            app.app_sessions.has_leases(),
            "poll must not retire a live lease"
        );
        assert!(
            !app.fetch_async_for(true, None, cx),
            "silent fetch cannot overlap"
        );
        assert!(
            !app.fetch_async_for(false, None, cx),
            "visible fetch cannot overlap"
        );
        app.open_pull_modal(cx);
        assert!(app.pull_modal().is_none(), "second pull must be refused");
    });

    if matches!(case, PullLeaseCase::Unknown) {
        std::fs::write(&fail, b"non-zero unrecognized output").unwrap();
    }
    std::fs::write(&release, b"go").expect("release the transport");
    wait_for(cx, |cx| {
        cx.read(|cx| {
            let state = app.read(cx);
            if matches!(case, PullLeaseCase::Unknown) {
                !state.app_sessions.reconcile_ids().is_empty()
            } else {
                !state.app_sessions.has_leases()
            }
        })
    });
    assert_eq!(
        ssh_pulls(&calls),
        if refused { 0 } else { 1 },
        "a refused identity must never execute git pull"
    );
    if matches!(case, PullLeaseCase::RemoteUrlChanged) {
        assert_eq!(
            ssh_probes(&calls),
            2,
            "plan and preflight each read the URL"
        );
    }
    if !refused {
        let calls = std::fs::read_to_string(&calls).unwrap();
        let command = calls.lines().find(|line| line.contains("pull")).unwrap();
        assert!(
            command.contains("/srv/real-worktree") && !command.contains("/srv/repo"),
            "git pull must target the frozen physical worktree, not the selected symlink: {command}"
        );
        assert!(
            command.contains("--no-rebase") && command.contains("--ff"),
            "the executed merge pull must override remote rebase and ff-only config: {command}"
        );
        assert!(
            command.contains("branch.main.mergeOptions=")
                && command.contains("--no-autostash")
                && command.contains("--no-recurse-submodules"),
            "the executed pull must pin all host-configurable write behavior: {command}"
        );
    }
    if matches!(case, PullLeaseCase::Success) {
        let recorded = kagi_git::oplog::read_oplog_tail(100);
        let entry = recorded.iter().find(|entry| entry.op == "pull").unwrap();
        assert_eq!(
            entry.repo, "kagi-e2e@e2e.invalid:/srv/repo",
            "durable scope must retain the selected root, not the execution path"
        );
    }
    if matches!(case, PullLeaseCase::Unknown) {
        let id = cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                state.app_sessions.has_leases(),
                "Unknown must retain admission"
            );
            assert!(!state.app_sessions.may_close_host());
            assert!(
                e2e::app_notice_message(state).is_some(),
                "reconcile notice is required"
            );
            state.app_sessions.reconcile_ids()[0]
        });
        let entries = kagi_git::oplog::read_oplog_tail(100);
        assert!(entries
            .iter()
            .any(|entry| entry.op == "pull" && matches!(entry.outcome, OpOutcome::Unknown { .. })));
        // Inspect the notice, arm the unobservable release, then confirm it.
        for _ in 0..3 {
            cx.run_until_parked();
            e2e::clear_control_bounds(window.window_id(), "app-notice-confirm");
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let bounds = e2e::control_bounds(window.window_id(), "app-notice-confirm")
                .expect("reconcile confirmation must be drawn");
            cx.simulate_click(window, bounds.center(), Modifiers::none());
        }
        wait_for(cx, |cx| {
            cx.read(|cx| !app.read(cx).app_sessions.needs_reconcile(id))
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                !state.app_sessions.has_leases(),
                "audited release must unlock"
            );
            assert!(state.app_sessions.may_close_host());
        });
        assert_eq!(
            kagi_git::oplog::read_oplog_tail(100)
                .iter()
                .filter(|entry| entry.op == "reconcile-release-unobservable")
                .count(),
            1,
            "one audited release row must precede lease release"
        );
    } else if refused {
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.app_sessions.has_leases());
            assert!(state.app_sessions.reconcile_ids().is_empty());
            assert!(state.app_sessions.may_close_host());
        });
        let pulls = kagi_git::oplog::read_oplog_tail(100)
            .into_iter()
            .filter(|entry| entry.op == "pull")
            .collect::<Vec<_>>();
        assert_eq!(pulls.len(), 1, "preflight must write one durable receipt");
        let expected = if matches!(case, PullLeaseCase::HeadReadFailure) {
            "preflight could not confirm identity"
        } else {
            "identity changed"
        };
        assert!(
            matches!(&pulls[0].outcome, OpOutcome::Refused { blockers }
                if blockers.iter().any(|reason| reason.contains(expected))),
            "the receipt must explain why the preflight refused"
        );
    } else {
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!e2e::op_latched(state));
            assert!(state.app_sessions.may_close_host());
        });
    }
    match original_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS {}",
        match case {
            PullLeaseCase::Success => "remote_pull_lease",
            PullLeaseCase::Unknown => "remote_pull_unknown_release",
            PullLeaseCase::IdentityChanged => "remote_pull_preflight_refusal",
            PullLeaseCase::ProxyRouteChanged => "remote_pull_proxy_route_refusal",
            PullLeaseCase::PlanningLatch => "remote_pull_planning_latch",
            PullLeaseCase::ToplevelChanged => "remote_pull_toplevel_refusal",
            PullLeaseCase::BranchChanged => "remote_pull_branch_refusal",
            PullLeaseCase::OidChanged => "remote_pull_head_oid_refusal",
            PullLeaseCase::UpstreamChanged => "remote_pull_upstream_refusal",
            PullLeaseCase::HeadReadFailure => "remote_pull_head_read_refusal",
            PullLeaseCase::CachedPreviewStale => "remote_pull_cached_preview_stale",
            PullLeaseCase::DirtyChanged => "remote_pull_dirty_refusal",
            PullLeaseCase::RemoteUrlChanged => "remote_pull_url_refusal",
        }
    );
}

fn leaky_helper(root: &Path, program: &str) -> String {
    let path = root.join(format!("{program}.sh"));
    std::fs::write(
        &path,
        format!("#!/bin/sh\nsleep 4 &\nexec git {program} \"$@\"\n"),
    )
    .expect("write helper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_str().expect("utf-8 path").to_string()
}

/// #702 review P1 + Codex — an unproven termination, end to end.
///
/// The whole chain in one scenario: the fetch's `TerminationUnknown` keeps the
/// lease and parks a reconcile requirement; settlement presents an inspectable
/// AppNotice that reaches reconcile without restoring the consumed Pull
/// confirmation.
pub fn scenario_pull_unknown_offers_its_reconcile(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });
    // The leak is installed only after the confirmation exists: a dirty Pull
    // fetches before it opens the modal (#625), and that read must succeed —
    // execution's own fetch is the one whose termination must be unproven.
    let helper = leaky_helper(remote_root.path(), "upload-pack");
    git(repo, &["config", "remote.origin.uploadpack", &helper]);

    press_enter(cx, &app, window);
    // Not `wait_idle`: a retained lease deliberately keeps the busy mirror set,
    // which is the state under test. Wait for the completion itself.
    wait_for(cx, |cx| {
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.inspect.is_some())
        })
    });

    assert_eq!(
        records(repo, "stash-pop").len(),
        0,
        "a fetch that is not proven stopped must not be followed by a pop"
    );
    assert!(
        !output(repo, &["stash", "list"]).is_empty(),
        "the user's work stays in the stash"
    );
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Unknown { .. })),
        "the indeterminate Pull outcome must be durable"
    );
    // The descendant of the fetch is still holding the pipes, so nothing about
    // this write is proven stopped: the scope stays reserved (ADR-0175).
    cx.read(|cx| {
        assert!(
            app.read(cx).app_sessions.has_leases(),
            "a writer whose process group is still alive keeps its lease"
        );
    });

    // Settlement presents the reconcile action directly; the consumed Pull
    // confirmation must not be resurrected in front of it.
    cx.read(|cx| {
        let notice = app
            .read(cx)
            .app_notice()
            .expect("an unacknowledged reconcile must offer itself");
        assert!(
            app.read(cx).pull_modal().is_none(),
            "Unknown must not restore the consumed Pull confirmation"
        );
        assert!(
            notice.inspect.is_some(),
            "and it must be the inspectable kind: {}",
            notice.message
        );
    });

    // Inspect while the descendant lives: the read must come back with nothing
    // observed and another look, never an acknowledge.
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app.app_notice().expect("still unresolved, still offered");
        assert!(
            notice.inspect.is_some() && notice.acknowledge.is_none(),
            "a live process group is not a stop proof: {}",
            notice.message
        );
        assert!(
            app.app_sessions.has_leases(),
            "and the scope stays reserved while it runs"
        );
    });

    // The descendant exits. Only now is the writer proven stopped, and only now
    // may the read be acknowledged and the scope released.
    std::thread::sleep(Duration::from_secs(4));
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.acknowledge.is_some())
        })
    });
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            !app.read(cx).app_sessions.has_leases(),
            "acknowledging a proven-stopped writer releases the scope"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_unknown_offers_its_reconcile: no pop, stash kept, reconcile reachable"
    );
}

/// #702 re-review — the 20 run-pipeline families need the same way in.
///
/// `oplog_outcome_from` records `Unknown` for a push whose termination could
/// not be proven, and `apply` parks a requirement that refuses every later
/// write in the repository. The notice was wired into the pull family only, so
/// a push in that state wedged the repository with no way to open the entry.
/// `finish_run` queues it at settlement now, like `finish_pull` does.
///
/// The other entrance — a later write's refusal naming the entry — is asserted
/// at the application layer (`blocking_reconcile`): reaching it from the GUI
/// needs a refusal that gets past the busy pre-check, and a parked
/// requirement holds that latch too.
pub fn scenario_run_unknown_offers_its_reconcile(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "to push"]);
    // The push's own `git push` leaves a descendant holding the pipes, so its
    // termination cannot be proven — the real production path.
    let helper = leaky_helper(remote_root.path(), "receive-pack");
    git(repo, &["config", "remote.origin.receivepack", &helper]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_push_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).push_modal().is_some(),
            "the fixture must produce a push confirmation"
        );
    });
    press_enter(cx, &app, window);
    wait_for(cx, |cx| {
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.inspect.is_some())
        })
    });
    let durable = records(repo, "push");
    assert_eq!(durable.len(), 1, "one attempt, one durable Push entry");
    assert!(matches!(durable[0].outcome, OpOutcome::Unknown { .. }));

    // Entrance 1: the completion itself.
    cx.read(|cx| {
        let state = app.read(cx);
        let notice = state
            .app_notice()
            .expect("a run completion that parked a requirement offers it");
        assert!(notice.inspect.is_some(), "{}", notice.message);
        assert!(
            state.push_modal().is_none(),
            "Unknown must not restore the consumed Push confirmation"
        );
    });

    // The Inspect action runs reconciliation. While the descendant still
    // owns the pipes it remains inspectable and cannot be acknowledged.
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });
    cx.read(|cx| {
        let app = app.read(cx);
        let notice = app.app_notice().expect("still unresolved, still offered");
        assert!(notice.inspect.is_some() && notice.acknowledge.is_none());
        assert!(app.app_sessions.has_leases());
    });

    std::thread::sleep(Duration::from_secs(4));
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| {
            app.read(cx)
                .app_notice()
                .is_some_and(|notice| notice.acknowledge.is_some())
        })
    });
    app.update(cx, |app, cx| app.confirm_app_notice(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            !app.read(cx).app_sessions.has_leases(),
            "acknowledging the proven-stopped Push releases its scope"
        );
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS run_unknown_offers_its_reconcile: the completion opens it");
}

/// #702 re-review — the reconcile notice is settlement, not presentation.
///
/// A pull whose tab the user left still parks a reconcile requirement, and that
/// requirement refuses every later write in the scope. Building the inspectable
/// notice after the current-tab guard left those completions with an entry
/// nobody could open: the write refusals said `NeedsReconcile` and there was no
/// other way in. The notice is queued with the recording-failure notices now,
/// before the guard, so it survives the switch.
pub fn scenario_pull_unknown_notice_survives_a_tab_switch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let (bare, other) = (
        remote_root.path().join("origin.git"),
        remote_root.path().join("other"),
    );
    let (bare_path, other_path) = (bare.to_str().unwrap(), other.to_str().unwrap());

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    git(repo, &["fetch", "-q", "origin"]);
    let helper = leaky_helper(remote_root.path(), "upload-pack");
    git(repo, &["config", "remote.origin.uploadpack", &helper]);
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "tab A must have a confirmation to press"
        );
    });

    // Confirm and leave in one synchronous turn: the completion certainly
    // arrives while tab B is on screen, so its presentation is dropped.
    app.update(cx, |app, cx| {
        app.start_pull(cx);
        app.switch_repo(1, cx);
    });
    wait_for(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.read(|cx| app.read(cx).app_notice().is_some())
    });

    cx.read(|cx| {
        let notice = app
            .read(cx)
            .app_notice()
            .expect("a parked reconcile must offer itself even to the tab that stayed");
        assert!(
            notice.inspect.is_some(),
            "and it must be the inspectable kind: {}",
            notice.message
        );
    });

    // The helper's `sleep 4 &` outlives the fetch; let it exit before the
    // next scenario runs, as the sibling scenarios do (#516).
    std::thread::sleep(Duration::from_secs(4));
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_unknown_notice_survives_a_tab_switch: settlement queued it, not presentation"
    );
}

/// #702 review P1 — a restored failure must not end on a success.
///
/// `steps` is `stash-push Success → pull Failed → stash-pop Success`, and
/// the old presentation path announced each receipt: the last announcement won,
/// so the window ended with a Pull failure modal over a `stash-pop: … → …`
/// success footer and three toasts. The receipts are panel rows now and the
/// decisive failure gets one toast, without a dismiss-only modal.
pub fn scenario_pull_failure_presents_only_the_decisive_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash && modal.plan.blockers.is_empty()),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });
    // The remote goes away only after the confirmation exists (see
    // `scenario_pull_auto_stash_failure_restores`): the pull's own fetch is the
    // one that must fail, and the restore that follows it must succeed.
    std::fs::remove_dir_all(&bare).unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    for op in ["stash-push", "pull", "stash-pop"] {
        assert_eq!(records(repo, op).len(), 1, "{op} is recorded once");
    }
    assert!(
        matches!(records(repo, "pull")[0].outcome, OpOutcome::Failed { .. }),
        "the pull is the failure the workflow settles on"
    );
    assert!(
        matches!(
            records(repo, "stash-pop")[0].outcome,
            OpOutcome::Success { .. }
        ),
        "and the restore after it succeeded — the case that used to win the footer"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.pull_modal().is_none(),
            "Failed must not restore the consumed Pull confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "a recorded Pull failure must not open a dismiss-only modal"
        );
        match &app.status_footer {
            kagi::ui::FooterStatus::Failed(text) => assert!(
                text.contains("pull"),
                "the footer belongs to the decisive receipt: {text}"
            ),
            other => panic!("a failed workflow must not end on a success footer: {other:?}"),
        }
        let toasts: Vec<String> = app
            .toast_stack
            .as_ref()
            .map(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .iter()
                    .map(|toast| toast.message.to_string())
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !toasts.iter().any(|message| message.contains("stash-pop")),
            "a sibling receipt must not announce itself: {toasts:?}"
        );
        let panel = app.op_log.as_ref().unwrap().read(cx);
        for op in ["stash-push", "pull", "stash-pop"] {
            let durable = records(repo, op);
            assert!(
                panel
                    .entries()
                    .iter()
                    .any(|entry| entry.id == durable[0].id),
                "the panel must still hold the durable {op} receipt"
            );
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_presents_only_the_decisive_receipt: one announcement, three rows"
    );
}

/// #702 review — the completion belongs to the tab that asked for it.
///
/// The app-level test proves the stamp is frozen; this one runs the branch that
/// *acts* on it. Tab A confirms a pull that will fail, the user leaves for tab B
/// before the completion lands: nothing of tab B changes (footer, modal), and
/// the failure is shown as #747 (e3a6d239) shows a background owner's receipt —
/// in the window-wide Operation Log and a toast labelled with A's repository.
pub fn scenario_pull_completion_drops_when_its_tab_is_left(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    // Behind by one, by local knowledge: the confirmation opens without a fetch.
    git(repo, &["fetch", "-q", "origin"]);
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    // What tab B's footer reads before tab A does anything.
    let footer_on_b = cx.read(|cx| format!("{:?}", app.read(cx).status_footer));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "tab A must have a confirmation to press"
        );
    });
    std::fs::remove_dir_all(&bare).unwrap();

    // Confirm and leave in one synchronous turn, so the job is queued and has
    // not run yet: the executor is driven only after the switch, which makes
    // "the completion arrives while another tab is on screen" the certain order
    // rather than a hoped-for one (as in `pull_confirm_departure_discards_old_visit`).
    app.update(cx, |app, cx| {
        app.start_pull(cx);
        app.switch_repo(1, cx);
    });
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    let durable = records(repo, "pull");
    assert_eq!(
        durable.len(),
        1,
        "the write still ran and still recorded itself"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        // Tab B first: what the user is looking at does not change.
        assert_eq!(app.active_tab, 1, "the user is still on tab B");
        assert!(
            app.pull_modal().is_none(),
            "tab A's failure must not open over tab B"
        );
        assert!(app.app_notice().is_none(), "nor as a notice over tab B");
        assert_eq!(
            format!("{:?}", app.status_footer),
            footer_on_b,
            "tab B's footer is not tab A's"
        );
        // #747: the receipt is in the window-wide log, and the toast says
        // which repository it came from.
        let panel = app.op_log.as_ref().unwrap().read(cx);
        assert!(
            panel
                .entries()
                .iter()
                .any(|entry| entry.id == durable[0].id),
            "tab A's receipt is in the window-wide Operation Log (#747)"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .expect("tab A's failure reaches a toast");
        assert!(
            matches!(toast.kind, kagi::ui::ToastKind::Error)
                && toast.message.starts_with(&durable[0].repo)
                && toast.message.contains("pull"),
            "the toast is labelled with tab A's repository: {}",
            toast.message
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_completion_drops_when_its_tab_is_left: tab B unchanged, A's receipt in the log with its repository"
    );
}

/// #702 review P2 — a pull refused for known blockers is the core's receipt.
///
/// The UI used to author this `Refused` entry itself, which left pull with two
/// receipt authors. Nothing executes here, so the assertion is that the entry
/// exists, is durable, carries the plan's blockers, and is the row the panel
/// shows — no second, UI-made copy beside it.
pub fn scenario_pull_blocked_plan_presents_a_core_receipt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let (app, window) = mount(cx, repo);
    // No remote at all: `plan_pull` blocks, and the modal is opened directly so
    // the blocker reaches `start_pull` rather than being filtered earlier.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        cx.notify();
    });
    cx.run_until_parked();
    let blocked = cx.read(|cx| {
        app.read(cx)
            .pull_modal()
            .is_some_and(|modal| !modal.plan.blockers.is_empty())
    });
    assert!(
        blocked,
        "the fixture must produce a Pull confirmation carrying blockers"
    );

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    let durable = records(repo, "pull");
    assert_eq!(
        durable.len(),
        1,
        "one refusal, one entry — not a UI copy beside the core's"
    );
    let OpOutcome::Refused { blockers } = &durable[0].outcome else {
        panic!(
            "a blocked plan is refused, not failed: {:?}",
            durable[0].outcome
        );
    };
    assert!(
        !blockers.is_empty(),
        "the refusal must carry the plan's blockers"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        let shown: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.op == "pull" && entry.repo == durable[0].repo)
            .collect();
        assert_eq!(shown.len(), 1, "the panel shows the refusal once");
        assert_eq!(
            shown[0].id, durable[0].id,
            "and it is the durable receipt, not one the UI made"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_blocked_plan_presents_a_core_receipt: the UI authors no pull outcome"
    );
}

/// ADR-0196 Wave 3: the pull workflow presents the backend's own receipts.
///
/// A dirty pull is three writes, each of which already records itself. The UI
/// used to add a fourth entry it synthesized from the composite result — so a
/// successful auto-stash pull wrote two `pull` rows, one of them not produced
/// by any execution. This asserts the three children in execution order, one
/// durable `pull` entry, and a panel row that *is* the durable entry.
pub fn scenario_pull_presents_backend_receipts(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx)
                .pull_modal()
                .is_some_and(|modal| modal.auto_stash && modal.plan.blockers.is_empty()),
            "the fixture must produce a confirmable auto-stash pull"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    // The tail reads newest first; the workflow ran the other way round.
    let durable = read_oplog_tail_for_repo(repo, 100);
    let workflow: Vec<&str> = durable
        .iter()
        .rev()
        .map(|entry| entry.op.as_str())
        .filter(|op| matches!(*op, "stash-push" | "pull" | "stash-pop"))
        .collect();
    assert_eq!(
        workflow,
        vec!["stash-push", "pull", "stash-pop"],
        "a dirty pull records its three children, in execution order"
    );
    let pulls = records(repo, "pull");
    assert_eq!(
        pulls.len(),
        1,
        "the one pull entry is the backend's; the UI must not synthesize a copy"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        let panel = app.op_log.as_ref().unwrap().read(cx);
        // The panel is seeded from the whole log, so scope it to this fixture.
        let mine: Vec<_> = panel
            .entries()
            .iter()
            .filter(|entry| entry.repo == pulls[0].repo)
            .collect();
        let shown: Vec<_> = mine.iter().filter(|entry| entry.op == "pull").collect();
        assert_eq!(
            shown.len(),
            1,
            "the panel shows the receipt once, not a UI copy"
        );
        assert_eq!(
            shown[0].id, pulls[0].id,
            "the panel entry is the durable receipt"
        );
        assert_eq!(
            shown[0].failure_code, pulls[0].failure_code,
            "the presented entry keeps the backend's failure code"
        );
        for op in ["stash-push", "pull", "stash-pop"] {
            let durable = records(repo, op);
            assert_eq!(durable.len(), 1, "{op} is recorded once");
            assert!(
                mine.iter().any(|entry| entry.id == durable[0].id),
                "the panel must show the durable {op} receipt"
            );
        }
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_presents_backend_receipts: three child receipts, no UI-synthesized entry"
    );
}

pub fn scenario_pull_auto_stash_failure_restores(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "restore untracked\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "failure must come from execution"
        );
    });

    // The remote disappears only after the confirmation exists: since #625 a
    // dirty Pull fetches *before* it opens the modal (ADR-0192), so removing it
    // up front would fail that fetch and there would be no modal to confirm —
    // a different scenario. Execution's own fetch is the one that must fail
    // here.
    std::fs::remove_dir_all(&bare).unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "restore after failed Pull\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "restore untracked\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "Pull failure must be durable"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.pull_modal().is_none(),
            "Failed must not restore the consumed Pull confirmation"
        );
        assert!(
            app.app_notice().is_none(),
            "the durable Operation Log row replaces the old plain AppNotice"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_failure_restores: failed Pull restores changes and records its result"
    );
}

/// A repository whose working tree edits the same line `origin/main` moved:
/// `shared.txt` collides, and the collision is a real content conflict.
///
/// Deliberately **not** fetched: `origin/main` is unknown to the repository
/// when the Pull button is pressed, so a path can only be named if the UI
/// fetched first (ADR-0192).
fn overlap_fixture(repo: &Path, remote_root: &Path) {
    let bare = remote_root.join("origin.git");
    let other = remote_root.join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
    git(repo, &["add", "shared.txt"]);
    git(repo, &["commit", "-qm", "add shared.txt"]);
    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root, &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("shared.txt"), "base\nupstream edit\n").unwrap();
    git(&other, &["add", "shared.txt"]);
    git(
        &other,
        &["commit", "-q", "-m", "upstream touches shared.txt"],
    );
    git(&other, &["push", "-q", "origin", "main"]);
    // The same line, edited here and not committed: the restore conflicts.
    std::fs::write(repo.join("shared.txt"), "base\nlocal edit\n").unwrap();
}

/// #625: a dirty Pull whose dirty path is also changed upstream must name that
/// path in the confirmation modal — *before* the user confirms.
///
/// The pull is a fast-forward, so neither the behind count nor
/// `MergePrediction` can see the collision: the path appears only because the
/// UI fetched first and the plan merged the two sides in memory (ADR-0192) —
/// exactly what used to surface after confirming, when the auto-stash failed
/// to restore.
pub fn scenario_pull_auto_stash_overlap_preview(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "the collision is a warning, not a refusal: {:?}",
            modal.plan.blockers
        );
        let shown: String = modal
            .plan
            .blockers
            .iter()
            .chain(modal.plan.warnings.iter())
            .map(|note| note.message_en())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            shown.contains("shared.txt"),
            "the modal must name the colliding path before confirmation:\n{shown}"
        );
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::RestoreConflict { .. }))),
            "the collision must travel as a typed note: {:?}",
            modal.plan.warnings
        );
        // The auto-stash swap must not have dropped it (#625 Part 3).
        assert!(
            modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::AutoStash { .. }))),
            "the auto-stash summary still belongs in the modal: {:?}",
            modal.plan.warnings
        );
    });

    // The regression PM caught: the confirmation opens and then a reload lands
    // on top of it. That reload is *caused by kagi's own fetch* (the watcher
    // sees `.git` change), and the modal-clearing sweep in `apply_reload_data`
    // wiped it ~0.5s later — Pull looked dead: pressed, nothing on screen.
    // Drive the same external reload twice, since the watcher can fire more
    // than once, and require the confirmation to still be there and still name
    // the path.
    for round in 1..=2 {
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            let modal = app
                .read(cx)
                .pull_modal()
                .unwrap_or_else(|| panic!("reload {round} closed the Pull confirmation"));
            assert!(modal.auto_stash, "round {round}");
            let shown: String = modal
                .plan
                .warnings
                .iter()
                .map(|note| note.message_en())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                shown.contains("shared.txt"),
                "reload {round} must keep the colliding path named:\n{shown}"
            );
        });
    }

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_overlap_preview: the modal names the colliding path and survives reloads"
    );
}

/// A Pull's pre-confirmation fetch cannot carry the old visit's proposal
/// across a tab departure. A fresh Pull must be explicitly requested on return.
pub fn scenario_pull_confirm_departure_discards_old_visit(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    let other_tab = build_fixture();

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_tab.path().to_path_buf(), cx));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    // The fetch runs only after this update, so departure precedes completion.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the confirmation must be waiting on a fetch"
        );
        app.switch_repo(1, cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_none(),
            "tab A's confirmation must not open over tab B"
        );
        assert!(app.read(cx).fetch_in_flight.is_none());
    });

    // Coming back does not resurrect an old-visit proposal.
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_none(),
            "the old visit's confirmation must not open on return"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_confirm_departure_discards_old_visit");
}

/// #625 P2: a modal opened while the pre-Pull fetch runs must not be replaced.
///
/// "One modal at a time" is structural (ADR-0093), so the completion cannot
/// simply set its own: the branch-name the user is typing would vanish.
pub fn scenario_pull_confirm_yields_to_another_modal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    let head = output(repo, &["rev-parse", "HEAD"]);
    // Pull, then a different modal — both before the executor runs the fetch,
    // so the completion certainly lands with the other modal already open.
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the confirmation must be waiting on a fetch"
        );
        app.open_create_branch_modal(kagi_git::CommitId(head.clone()), cx);
    });
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        // Both halves of "one modal at a time": the request yields instead of
        // replacing what the user opened, and what the user opened is still
        // there — the fetch's own reload no longer sweeps a pure-input modal.
        assert!(
            app.read(cx).create_branch_modal().is_some(),
            "the modal opened during the fetch must still be on screen"
        );
        assert!(
            app.read(cx).pull_modal().is_none(),
            "the Pull confirmation must not take over the modal slot"
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_confirm_yields_to_another_modal: a modal opened during the fetch is kept"
    );
}

/// A production dirty-Pull fetch failure is recorded without disturbing the
/// Remote Browse modal the user opened meanwhile.
pub fn scenario_pull_failure_notice_waits_for_remote_browse(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
        app.open_remote_browse(cx);
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "acknowledge before fetch failure"),
            "the queued notice must carry a real reconciliation acknowledgement",
        );
    });

    // The fetch task has been dispatched but the executor has not run it yet.
    // Removing the remote makes the fetch itself record its failure while
    // Remote Browse owns the slot; the Pull waiter adds no second receipt.
    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    cx.read(|cx| {
        assert!(
            app.read(cx).remote_browse().is_some(),
            "a Pull fetch failure must not replace Remote Browse",
        );
    });

    app.update(cx, |app, cx| {
        app.cancel_remote_browse();
        kagi::ui::e2e::present_app_notice(app);
        assert!(
            kagi::ui::e2e::app_notice_is_acknowledgeable(app),
            "the actionable notice already waiting must stay first",
        );
        app.confirm_app_notice(cx);
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "async-notice-action-still-executes: the delayed Acknowledge must execute",
        );
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "the recorded fetch failure must not add a dismiss-only notice"
    );
    assert!(
        records(repo, "fetch")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "the fetch failure must remain durable in Operation Log"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_waits_for_remote_browse: production failure uses Operation Log without replacing the modal"
    );
}

/// A recorded fetch failure does not add a third item to an existing notice
/// queue containing plain and actionable notices.
pub fn scenario_pull_failure_notice_waits_for_app_notice(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
        kagi::ui::e2e::deliver_app_notice(app, "first plain notice");
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app),
            Some("first plain notice"),
            "async-notice-keeps-first-plain: the first plain notice must own the vacant slot",
        );
        assert!(
            kagi::ui::e2e::deliver_acknowledge_notice(app, "second actionable notice"),
            "the second notice must carry a real reconciliation acknowledgement",
        );
    });

    // Fail the already-dispatched production fetch while a plain AppNotice is
    // visible and an actionable notice is already waiting behind it.
    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        assert_eq!(
            kagi::ui::e2e::app_notice_message(app),
            Some("first plain notice"),
            "async-notice-does-not-replace-app-notice: the later Pull failure must not replace or discard the visible notice",
        );
        app.clear_app_notice();
        kagi::ui::e2e::present_app_notice(app);
        assert!(
            kagi::ui::e2e::app_notice_is_acknowledgeable(app),
            "async-notice-app-fifo-second: the notice already waiting must be presented second",
        );
        app.confirm_app_notice(cx);
        assert!(
            app.app_sessions.reconcile_ids().is_empty(),
            "async-notice-app-action-executes: the queued Acknowledge must still execute",
        );
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "the recorded Pull failure must not become a third plain notice"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_waits_for_app_notice: recorded failures do not extend the notice FIFO"
    );
}

/// A production fetch failure has no modal lifecycle: opening and closing a
/// different modal cannot resurrect a dismiss-only failure notice.
pub fn scenario_pull_failure_notice_displacement_vs_dismissal(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.open_pull_modal(cx);
        assert!(
            app.fetch_in_flight.is_some(),
            "the dirty Pull must be waiting on its production fetch"
        );
    });

    drop(remote_root);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "a recorded fetch failure must not create a plain AppNotice"
    );

    app.update(cx, |app, cx| app.open_remote_browse(cx));
    assert!(
        cx.read(|cx| app.read(cx).remote_browse().is_some()),
        "notice-displacement-opens-new-modal: Remote Browse must displace the visible notice"
    );
    app.update(cx, |app, _| {
        app.cancel_remote_browse();
        kagi::ui::e2e::present_app_notice(app);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "closing another modal must not materialize a recorded failure notice"
    );
    assert!(
        records(repo, "fetch")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "the complete fetch error remains available in Operation Log"
    );

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_failure_notice_displacement_vs_dismissal: recorded failures have no modal lifecycle"
    );
}

/// #625 P1: the confirmation's promise is checked before anything is stashed.
///
/// After the modal is on screen an editor saves *another* path the update also
/// changes. Stashing first would hide it from every downstream guard — the
/// preflight and the execute-time dirty-path check both see a clean tree — and
/// only the restore would conflict, unannounced. The run must refuse instead.
/// The refusal is presented as #718 (34d7d1e5) / #747 (e3a6d239) present a
/// recorded failure: the confirmation closes, and the footer, an error toast
/// and the Operation Log carry the reason.
pub fn scenario_pull_refuses_when_the_dirty_set_moved(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    overlap_fixture(repo, remote_root.path());
    // A second path the upstream also changed, still clean at confirmation time.
    let other = remote_root.path().join("other");
    std::fs::write(other.join("second.txt"), "upstream second\n").unwrap();
    git(&other, &["add", "second.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream adds second.txt"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    cx.read(|cx| {
        assert!(
            app.read(cx).pull_modal().is_some(),
            "the confirmation must be on screen first"
        );
    });

    // The promise goes stale: an editor writes a path the modal never named.
    std::fs::write(repo.join("second.txt"), "local second\n").unwrap();

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert!(
        output(repo, &["stash", "list"]).is_empty(),
        "nothing may be stashed once the confirmation is stale"
    );
    assert_eq!(
        output(repo, &["rev-parse", "HEAD"]),
        output(repo, &["rev-parse", "HEAD@{0}"]),
        "and nothing may be pulled"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("second.txt")).unwrap(),
        "local second\n",
        "the work the user did after confirming is untouched"
    );
    let refusals = records(repo, "pull");
    assert_eq!(refusals.len(), 1, "one receipt for the refused run");
    assert!(
        matches!(&refusals[0].outcome, OpOutcome::Refused { blockers }
            if blockers.iter().any(|b| b.contains("changed after this confirmation"))),
        "the receipt records why: {:?}",
        refusals[0].outcome
    );
    cx.read(|cx| {
        let app = app.read(cx);
        // #718: the confirmation is not put back with the error on it.
        assert!(
            app.pull_modal().is_none(),
            "the stale confirmation is not offered again"
        );
        // #747: nor shown again as a notice (#902 review).
        assert!(
            app.app_notice().is_none(),
            "the refusal is not put back as a notice either"
        );
        let kagi::ui::FooterStatus::Failed(footer) = &app.status_footer else {
            panic!("the footer must carry the refusal: {:?}", app.status_footer)
        };
        assert!(
            footer.contains("changed after this confirmation"),
            "the refusal must say why: {footer}"
        );
        let toast = app
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .expect("the refusal reaches a toast");
        assert!(
            matches!(toast.kind, kagi::ui::ToastKind::Error)
                && toast.message.contains("changed after this confirmation"),
            "the toast says why: {}",
            toast.message
        );
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_refuses_when_the_dirty_set_moved: a stale confirmation stashes nothing"
    );
}
