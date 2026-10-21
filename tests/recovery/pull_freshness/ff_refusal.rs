//! Non-FF refusal is a readable, cancellable explanation, never a write approval.

use super::*;
use kagi::ui::{button_style, dialog_a11y};
use kagi_domain::plan_note::{PlanNote, PullNote};

#[derive(Clone, Copy)]
enum Drift {
    Head,
    Worktree,
    UpstreamTip,
    RemoteIdentity,
    Visit,
}

pub fn scenario_pull_ff_only_diverged_refusal(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        for remote in ["origin", "team/origin", "."] {
            for drift in [
                Drift::Head,
                Drift::Worktree,
                Drift::UpstreamTip,
                Drift::RemoteIdentity,
                Drift::Visit,
            ] {
                refusal_lifecycle(cx, remote, drift);
            }
        }
    }
    eprintln!("[gui-e2e] PASS pull_ff_only_diverged_refusal");
}

fn assert_refusal(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    remote: &str,
    upstream_ref: &str,
) {
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.fetch_in_flight.is_none());
        assert!(!app.app_sessions.has_leases());
        let modal = app.branch_plan_modal().expect("non-FF reason stays readable");
        assert!(modal.error.is_none());
        assert!(!modal.confirm_armed);
        assert!(modal.plan.pull_identity.is_none(), "refusal is not approval");
        assert!(modal.plan.blockers.iter().any(|note| matches!(
            note,
            PlanNote::Pull(PullNote::CannotFastForward { branch }) if branch == "main"
        )));
        let (identity, _) = modal.fetched_refs.as_ref().unwrap();
        assert_eq!(identity.branch, "main");
        assert_eq!(identity.remote, remote);
        assert_eq!(identity.upstream_ref, upstream_ref);
    });
    button_style::clear_recorded_modal_buttons();
    draw(cx, window);
    let (_, explanation) = dialog_a11y::recorded_note("plan-blocker-0")
        .expect("the actual refusal reason is drawn");
    assert!(
        explanation.contains("main"),
        "the rendered refusal identifies the selected branch"
    );
    assert!(e2e::control_bounds(window.window_id(), "plan-cancel").is_some());
    if let Some(confirm) = button_style::recorded_modal_button("plan-confirm") {
        assert!(confirm.disabled, "a blocked explanation cannot authorize Pull");
    }
}

fn protected_refs(repo: &Path, upstream_ref: &str) -> (String, String) {
    (
        output(
            repo,
            &["for-each-ref", "--format=%(refname) %(objectname)", "refs/heads/"],
        ),
        output(repo, &["rev-parse", "--verify", upstream_ref]),
    )
}

fn refusal_lifecycle(cx: &mut VisualTestAppContext, remote: &str, drift: Drift) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let fetch_remote = if remote == "." { "origin" } else { remote };
    let origin = Origin::new_named(&repo, fetch_remote);
    git(&repo, &["config", "pull.ff", "only"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "local divergence"]);
    let incoming = origin.advance();
    git(&repo, &["fetch", "-q", fetch_remote]);
    let upstream_ref = if remote == "." {
        git(&repo, &["branch", "local-upstream", &incoming]);
        git(&repo, &["branch", "--set-upstream-to=local-upstream", "main"]);
        "refs/heads/local-upstream".to_string()
    } else {
        format!("refs/remotes/{remote}/main")
    };
    let before = Checkout::read(&repo);
    let refs_before = protected_refs(&repo, &upstream_ref);
    let (app, window) = mount(cx, &repo);
    click_branch_sync(cx, window, "main", true);
    assert_refusal(cx, &app, window, remote, &upstream_ref);

    for _ in 0..2 {
        let (owner, revision) = cx.read(|cx| {
            let app = app.read(cx);
            let owner = app.active_session().unwrap();
            (owner, app.reads.revision(owner))
        });
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.reads.revision(owner) > revision);
            assert!(!app.reads.is_loading(owner), "unchanged read was accepted");
        });
        assert_refusal(cx, &app, window, remote, &upstream_ref);
        assert_eq!(Checkout::read(&repo), before);
        assert_eq!(protected_refs(&repo, &upstream_ref), refs_before);
    }
    let cancel = e2e::control_bounds(window.window_id(), "plan-cancel").unwrap();
    cx.simulate_click(window, cancel.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|cx| assert!(app.read(cx).branch_plan_modal().is_none()));
    assert_eq!(Checkout::read(&repo), before);
    assert_eq!(protected_refs(&repo, &upstream_ref), refs_before);

    // A new request is admitted; an older captured snapshot cannot keep its
    // explanation after live HEAD, checkout, upstream mapping or visit drift.
    click_branch_sync(cx, window, "main", true);
    assert_refusal(cx, &app, window, remote, &upstream_ref);
    let (owner, revision) = app.update(cx, |app, cx| {
        let owner = app.active_session().unwrap();
        app.reload_external(cx);
        (owner, app.reads.revision(owner))
    });
    let dispatcher = cx.background_executor.dispatcher().as_test().unwrap();
    while dispatcher.tick(true) {}
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.reads.is_loading(owner),
            "captured snapshot has not been published"
        );
        assert_eq!(app.reads.revision(owner), revision);
        assert!(app.branch_plan_modal().is_some(), "original refusal is still shown");
    });
    match drift {
        Drift::Head => git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "external HEAD change"],
        ),
        Drift::Worktree => {
            std::fs::write(repo.join("README.md"), "external checkout edit\n").unwrap();
        }
        Drift::UpstreamTip => {
            // Fresh planning now sees an executable no-op. The old blocked
            // intent must disappear, not be promoted into automatic execution.
            git(&repo, &["update-ref", &upstream_ref, &before.head]);
            let fresh = kagi_git::Backend::open(&repo)
                .unwrap()
                .plan_pull_branch_ff_preview("main")
                .unwrap()
                .0;
            assert!(fresh.blockers.is_empty());
        }
        Drift::RemoteIdentity => {
            let url = output(&repo, &["remote", "get-url", fetch_remote]);
            git(&repo, &["remote", "add", "same-ref-alias", &url]);
            let source = if remote == "." {
                "refs/heads/local-upstream"
            } else {
                "refs/heads/main"
            };
            git(
                &repo,
                &[
                    "config",
                    "remote.same-ref-alias.fetch",
                    &format!("+{source}:{upstream_ref}"),
                ],
            );
            git(&repo, &["config", "branch.main.remote", "same-ref-alias"]);
            let (changed, tip) = kagi_git::Backend::open(&repo)
                .unwrap()
                .pull_branch_ref_state("main")
                .unwrap();
            assert_eq!(changed.upstream_ref, upstream_ref);
            assert_eq!(changed.local_oid.0, before.head);
            assert_eq!(tip.0, incoming);
            assert_eq!(changed.remote, "same-ref-alias");
        }
        Drift::Visit => {
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.open_home_tab(window, cx));
            })
            .unwrap();
            app.update(cx, |app, cx| app.close_home_tab(cx));
        }
    }
    let changed = Checkout::read(&repo);
    let changed_refs = protected_refs(&repo, &upstream_ref);
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.branch_plan_modal().is_none(), "stale explanation expired");
        assert!(!app.app_sessions.has_leases());
    });
    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();
    assert_eq!(Checkout::read(&repo), changed);
    assert_eq!(protected_refs(&repo, &upstream_ref), changed_refs);
    assert!(output(&repo, &["stash", "list"]).is_empty());
    for operation in ["pull", "branch-pull-ff", "stash-push", "stash-pop"] {
        assert!(oplog::records(&repo, operation, 100, true).is_empty());
    }
    unmount(cx, app, window);
}
