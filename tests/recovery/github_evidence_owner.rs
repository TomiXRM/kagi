//! #643 Wave 4 S2b: GitHub evidence follows the session that observed it.
use crate::evidence_support::{deferred, pr_page, pull_request};
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::app::SessionId;
use kagi::ui::workspace_mode::WorkspaceMode;
use kagi::ui::{e2e, list_a11y, KagiApp};
use kagi_domain::github::PrListSnapshot;
use kagi_git::github::PrFetchError;
use std::collections::HashSet;

fn queue_ready(cx: &mut VisualTestAppContext, result: Result<PrListSnapshot, PrFetchError>) {
    e2e::queue_github_pr_fetch(cx.background_executor.spawn(async move { result }));
}

fn ui_domain(app: &KagiApp) -> HashSet<SessionId> {
    app.ui.keys().copied().collect()
}

/// Put the active session on the top PRs tab and draw one fresh frame. Of
/// `candidates`, return the PR numbers whose row the `pr-list` table laid out
/// in that frame. The tab's own center pane must be drawn, so an empty result
/// is the PRs tab showing no row, not a frame that never reached it. Nothing
/// is parked first: a queued refetch must not repopulate the evidence that
/// the switch itself has to restore.
fn rendered_pr_numbers(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: gpui::AnyWindowHandle,
    candidates: &[u64],
) -> Vec<u64> {
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "the PRs tab is the active workspace"
    );
    let id = window.window_id();
    e2e::clear_control_bounds(id, "pr-mode-center-pane");
    for number in candidates {
        e2e::clear_control_bounds(id, &format!("pr-home-row-{number}"));
    }
    list_a11y::clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the PRs tab");
    assert!(
        e2e::control_bounds(id, "pr-mode-center-pane").is_some(),
        "the PRs tab center pane is drawn"
    );
    let drawn: Vec<u64> = candidates
        .iter()
        .copied()
        .filter(|number| e2e::control_bounds(id, &format!("pr-home-row-{number}")).is_some())
        .collect();
    match list_a11y::recorded_list("pr-list") {
        Some(list) => {
            assert_eq!(list.role, Some(gpui::Role::List));
            assert_eq!(
                (list.size, list.rows.len()),
                (drawn.len(), drawn.len()),
                "pr-list rows are exactly the laid-out candidate rows: {list:?}"
            );
        }
        None => assert!(drawn.is_empty(), "PR rows drawn outside pr-list"),
    }
    drawn
}

fn attached_domain(app: &KagiApp) -> HashSet<SessionId> {
    app.tabs.iter().map(|tab| tab.session).collect()
}

pub fn scenario_github_evidence_restores(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("repo A path");
    let repo_b = fixture_b.path().canonicalize().expect("repo B path");
    let (app, window) = mount(cx, &repo_a);
    let a_prs = vec![pull_request(101, "A evidence", "a-pr")];

    queue_ready(cx, Ok(pr_page(Vec::new(), "github.com/example/repo", None)));
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    queue_ready(
        cx,
        Ok(pr_page(a_prs.clone(), "github.com/example/repo", None)),
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    assert_eq!(
        rendered_pr_numbers(cx, &app, window, &[101]),
        vec![101],
        "the PRs tab must visibly list A's fetched PR before switching",
    );

    queue_ready(cx, Ok(pr_page(Vec::new(), "github.com/example/repo", None)));
    app.update(cx, |app, cx| {
        assert_eq!(
            app.active_session(),
            Some(owner_a),
            "A must be active after fetch"
        );
        app.switch_repo(1, cx);
        assert_eq!(
            app.active_session(),
            Some(owner_b),
            "B must be active after switch"
        );
    });
    cx.run_until_parked();
    assert!(
        rendered_pr_numbers(cx, &app, window, &[101]).is_empty(),
        "the PRs tab reused A's PR row for clean B",
    );

    queue_ready(cx, Ok(pr_page(a_prs, "github.com/example/repo", None)));
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(
            app.ui()
                .github_prs
                .iter()
                .map(|pr| pr.number)
                .collect::<Vec<_>>(),
            vec![101],
            "A PR evidence did not survive A→B→A",
        );
    });
    assert_eq!(
        rendered_pr_numbers(cx, &app, window, &[101]),
        vec![101],
        "the PRs tab did not restore A's PR row after equal-epoch B",
    );

    // The navigator row must retain its natural two-line height when the
    // collection overflows the pane; the pane scrolls, not the cards' text.
    // INBOX keeps what is broken or ready *for the viewer*, so the row only
    // exists once the app knows whose PRs these are - the fixture's author,
    // as the login on the PR's own host (#906).
    app.update(cx, |app, cx| {
        app.github_host_logins
            .insert(Some("github.com".to_string()), "alice".to_string());
        app.show_pr_mode(cx);
    });
    e2e::clear_control_bounds(window.window_id(), "pr-mode-card-101");
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .expect("draw the PR navigator");
    let card = e2e::control_bounds(window.window_id(), "pr-mode-card-101")
        .expect("PR 101's navigator row is drawn");
    let natural_size = card.size;
    let mut many = vec![pull_request(101, "A evidence", "a-pr")];
    many.extend((1..100).map(|n| pull_request(n, "Synthetic overflow row", "a-pr")));
    queue_ready(cx, Ok(pr_page(many, "github.com/example/repo", None)));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    e2e::clear_control_bounds(window.window_id(), "pr-mode-card-101");
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw overflowing PR navigator");
    let overflow_card = e2e::control_bounds(window.window_id(), "pr-mode-card-101")
        .expect("first PR card remains visible");
    assert_eq!(
        overflow_card.size, natural_size,
        "100 cards must scroll without compressing their title and metadata"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS github_evidence_restores");
}

pub fn scenario_github_evidence_background_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("repo A path");
    let repo_b = fixture_b.path().canonicalize().expect("repo B path");
    let (app, window) = mount(cx, &repo_a);
    let b_prs = vec![pull_request(202, "B baseline", "b-pr")];
    let a_baseline = vec![pull_request(101, "A baseline", "a-pr")];
    let a_refreshed = vec![pull_request(102, "A refreshed", "a-next")];

    queue_ready(
        cx,
        Ok(pr_page(b_prs.clone(), "github.com/example/repo", None)),
    );
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    queue_ready(cx, Ok(pr_page(a_baseline, "github.com/example/repo", None)));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    let (task, success) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    queue_ready(
        cx,
        Ok(pr_page(b_prs.clone(), "github.com/example/repo", None)),
    );
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    success.send(Ok(pr_page(
        a_refreshed.clone(),
        "github.com/example/repo",
        None,
    )));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(owner_b), "B must remain active");
        assert_eq!(
            app.ui()
                .github_prs
                .iter()
                .map(|pr| pr.number)
                .collect::<Vec<_>>(),
            vec![202],
            "background success changed B evidence",
        );
        assert!(
            app.ui().github_error.is_none(),
            "background success changed B error"
        );
        assert!(
            !app.ui().github_unavailable,
            "background success changed B availability"
        );
    });
    queue_ready(
        cx,
        Ok(pr_page(
            a_refreshed.clone(),
            "github.com/example/repo",
            None,
        )),
    );
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(
            app.active_session(),
            Some(owner_a),
            "return to A after success"
        );
        assert_eq!(
            app.ui().github_prs[0].number,
            102,
            "background success did not settle on A"
        );
    });
    cx.run_until_parked();

    let (task, failure) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    queue_ready(
        cx,
        Ok(pr_page(b_prs.clone(), "github.com/example/repo", None)),
    );
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    failure.send(Err(PrFetchError::Network("offline-A".to_string())));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.ui().github_prs[0].number,
            202,
            "background error replaced B evidence"
        );
        assert!(
            app.ui().github_error.is_none(),
            "background error leaked into B"
        );
        assert!(
            !app.ui().github_unavailable,
            "background error changed B availability"
        );
    });
    queue_ready(cx, Err(PrFetchError::Network("offline-A".to_string())));
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(
            app.ui().github_prs[0].number,
            102,
            "A error discarded A last-good evidence"
        );
        assert!(
            app.ui()
                .github_error
                .as_deref()
                .is_some_and(|error| error.contains("offline-A")),
            "background error did not settle on A",
        );
        assert!(
            !app.ui().github_unavailable,
            "A network error became unavailable"
        );
    });
    cx.run_until_parked();

    let (task, unavailable) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    queue_ready(cx, Ok(pr_page(b_prs, "github.com/example/repo", None)));
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    unavailable.send(Err(PrFetchError::Unavailable("not-GitHub-A".to_string())));
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert_eq!(
            app.ui().github_prs[0].number,
            202,
            "background unavailable cleared B evidence"
        );
        assert!(
            app.ui().github_error.is_none(),
            "background unavailable changed B error"
        );
        assert!(
            !app.ui().github_unavailable,
            "background unavailable leaked into B"
        );
    });
    queue_ready(
        cx,
        Err(PrFetchError::Unavailable("not-GitHub-A".to_string())),
    );
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert!(
            app.ui().github_prs.is_empty(),
            "A unavailable verdict kept stale A evidence"
        );
        assert!(
            app.ui().github_error.is_none(),
            "A unavailable verdict retained A error"
        );
        assert!(
            app.ui().github_unavailable,
            "background unavailable did not settle on A"
        );
    });
    cx.run_until_parked();

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS github_evidence_background_owner");
}

pub fn scenario_github_evidence_detached_owner(cx: &mut VisualTestAppContext) {
    // Reopen starts both the tab's immediate read and a fresh ticker. Only the
    // former has a queued result; the ticker must answer through a fixture too,
    // rather than letting a real gh auth failure masquerade as stale delivery.
    let _gh = crate::pr_fields_focus::OfflineGh::with_script(
        r#"#!/bin/sh
case "$*" in
  "--version")
    printf 'gh version 2.0.0 (detached owner fixture)\n' ;;
  "repo view --json url")
    printf '%s\n' '{"url":"https://github.com/example/repo"}' ;;
  "api graphql --hostname github.com -F owner=example -F name=repo -F cursor=null -f states[]=OPEN -f query="*"pullRequests(first: 100,"*)
    printf '%s\n' '{"data":{"repository":{"pullRequests":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}' ;;
  *)
    printf 'unsupported detached owner fixture gh request: %s\n' "$*" >&2
    exit 1 ;;
esac
"#,
    );
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().expect("repo path");
    let (app, window) = mount(cx, &repo);
    let old_owner = cx.read(|cx| app.read(cx).active_session().expect("mounted owner"));

    let (task, reply) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| {
        app.refresh_github_prs(cx);
        app.close_tab(0, cx);
    });
    queue_ready(cx, Ok(pr_page(Vec::new(), "github.com/example/repo", None)));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo, cx), "reopen same path");
    });
    cx.run_until_parked();
    let (new_owner, domain_before, availability_before) = cx.read(|cx| {
        let app = app.read(cx);
        let new_owner = app.active_session().expect("reopened owner");
        assert_ne!(
            old_owner, new_owner,
            "same-path reopen reused detached incarnation"
        );
        assert_eq!(
            ui_domain(app),
            attached_domain(app),
            "UI domain was invalid before completion"
        );
        assert!(
            app.ui().github_error.is_none(),
            "reopened owner's own reads must settle without an error"
        );
        assert!(
            !app.ui().github_prs_loading,
            "reopened owner's own reads must settle before stale delivery"
        );
        (new_owner, ui_domain(app), app.ui().github_unavailable)
    });

    reply.send(Ok(pr_page(
        vec![pull_request(303, "stale completion", "old-a-pr")],
        "github.com/example/repo",
        None,
    )));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.active_session(),
            Some(new_owner),
            "old completion changed active owner"
        );
        assert_eq!(
            ui_domain(app),
            domain_before,
            "detached completion recreated its released UI entry",
        );
        assert_eq!(
            ui_domain(app),
            attached_domain(app),
            "detached completion changed UI domain"
        );
        assert!(
            app.ui().github_prs.is_empty(),
            "old completion entered same-path reopen"
        );
        assert!(
            app.ui().github_error.is_none(),
            "old completion added error to same-path reopen"
        );
        assert_eq!(
            app.ui().github_unavailable,
            availability_before,
            "old completion changed reopened availability"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS github_evidence_detached_owner");
}
