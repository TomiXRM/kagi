//! #643 Wave 4 S2b: GitHub evidence follows the session that observed it.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::app::SessionId;
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::{CiState, Mergeable, PullRequest, ReviewState};
use kagi_git::github::PrFetchError;
use std::collections::HashSet;

fn pull_request(number: u64, title: &str, head: &str) -> PullRequest {
    PullRequest {
        number,
        title: title.to_string(),
        head: head.to_string(),
        head_sha: format!("{number:040x}"),
        base: "main".to_string(),
        is_draft: false,
        ci: CiState::Success,
        review: ReviewState::Approved,
        url: format!("https://github.com/example/repo/pull/{number}"),
        author: "alice".to_string(),
        reviewers: Vec::new(),
        body: String::new(),
        checks: Vec::new(),
        mergeable: Mergeable::Clean,
        cross_repository: false,
        base_repo: "github.com/example/repo".to_string(),
    }
}

fn queue_ready(cx: &mut VisualTestAppContext, result: Result<Vec<PullRequest>, PrFetchError>) {
    e2e::queue_github_pr_fetch(cx.background_executor.spawn(async move { result }));
}

fn ui_domain(app: &KagiApp) -> HashSet<SessionId> {
    app.ui.keys().copied().collect()
}

fn rendered_pr_numbers(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: gpui::AnyWindowHandle,
) -> Vec<u64> {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .expect("draw GitHub sidebar consumer");
    cx.read(|cx| {
        app.read(cx)
            .sidebar
            .rows
            .iter()
            .filter_map(|row| match row {
                kagi::ui::sidebar::SidebarRow::PullRequest { pr, .. } => Some(pr.number),
                _ => None,
            })
            .collect()
    })
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

    queue_ready(cx, Ok(Vec::new()));
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        app.branch_groups_collapsed.clear();
        assert!(app.open_repository(repo_b, cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    queue_ready(cx, Ok(a_prs.clone()));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    assert_eq!(
        rendered_pr_numbers(cx, &app, window),
        vec![101],
        "sidebar must visibly show A's fetched PR before switching",
    );

    queue_ready(cx, Ok(Vec::new()));
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
        rendered_pr_numbers(cx, &app, window).is_empty(),
        "sidebar consumer reused A PR rows for clean B",
    );

    queue_ready(cx, Ok(a_prs));
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
        rendered_pr_numbers(cx, &app, window),
        vec![101],
        "sidebar consumer did not restore A PR row after equal-epoch B",
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

    queue_ready(cx, Ok(b_prs.clone()));
    let (owner_a, owner_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();
    queue_ready(cx, Ok(a_baseline));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();

    let (task, success) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    queue_ready(cx, Ok(b_prs.clone()));
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    success.send(Ok(a_refreshed.clone()));
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
    queue_ready(cx, Ok(a_refreshed.clone()));
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
    queue_ready(cx, Ok(b_prs.clone()));
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
    queue_ready(cx, Ok(b_prs));
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
    queue_ready(cx, Ok(Vec::new()));
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
        (new_owner, ui_domain(app), app.ui().github_unavailable)
    });

    reply.send(Ok(vec![pull_request(303, "stale completion", "old-a-pr")]));
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
