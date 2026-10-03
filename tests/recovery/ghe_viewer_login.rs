//! Issue #906 — on a GitHub Enterprise repository, "mine" is the login on
//! that server, never the github.com account.
//!
//! The offline `gh` answers `api user` per host: github.com is always known
//! (`dotcom-me`), the Enterprise host only once the test writes its login
//! file (`ghe-me`). With only the github.com login known, the PR and Issues
//! navigators must not claim anything as yours — no counts, no confirmed
//! empty list — and the github.com account's PR must not land in Mine. When
//! the Enterprise login arrives through the production PR refresh, Mine /
//! Assigned / Created follow it.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::{Issue, IssueState, PullRequest};
use kagi_domain::pr_list::PrSection;

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::pr_fields_focus::OfflineGh;

const HOST: &str = "ghe.example.com";
const BASE_REPO: &str = "ghe.example.com/acme/widgets";

/// `api user --hostname ghe.example.com` reads `ghe_login.txt` and fails
/// while it is absent; every other host is github.com's `dotcom-me`.
fn install_gh(dir: &Path) -> OfflineGh {
    let login = dir.join("ghe_login.txt");
    OfflineGh::with_script(&format!(
        r#"#!/bin/sh
case "$1 $2" in
  'api user')
    case "$*" in
      *'--hostname {HOST}'*) cat '{login}' 2>/dev/null || exit 1 ;;
      *) echo dotcom-me ;;
    esac ;;
  *) echo 'no GitHub repository here' >&2; exit 1 ;;
esac
"#,
        login = login.display(),
    ))
}

fn issue(number: u64, author: &str, assignees: &[&str]) -> Issue {
    Issue {
        number,
        title: format!("issue {number}"),
        state: IssueState::Open,
        url: format!("https://{BASE_REPO}/issues/{number}"),
        author: author.into(),
        assignees: assignees.iter().map(|login| (*login).into()).collect(),
        labels: Vec::new(),
        body: String::new(),
        comments: Vec::new(),
        comment_count: 0,
        created_at: format!("2026-09-{:02}T00:00:00Z", 10 + number),
        updated_at: format!("2026-09-{:02}T00:00:00Z", 10 + number),
    }
}

fn enterprise_pr(number: u64, author: &str) -> PullRequest {
    PullRequest {
        url: format!("https://{BASE_REPO}/pull/{number}"),
        author: author.into(),
        base_repo: BASE_REPO.into(),
        ..pull_request(
            number,
            &format!("PR {number}"),
            &format!("remote-only-{number}"),
        )
    }
}

fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), id);
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the navigator");
    e2e::control_bounds(window.window_id(), id).is_some()
}

fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    assert!(drawn(cx, window, id), "{id} was not laid out");
    let bounds = e2e::control_bounds(window.window_id(), id).unwrap();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

/// Run the production PR refresh over `prs`; its completion asks for the
/// login on the PRs' host.
fn refresh_prs(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, prs: Vec<PullRequest>) {
    e2e::queue_github_pr_fetch(cx.background_executor.spawn(async move { Ok(prs) }));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
}

fn wait(
    cx: &mut VisualTestAppContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestAppContext) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !done(cx) {
        assert!(std::time::Instant::now() < deadline, "{what}");
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

pub fn scenario_ghe_viewer_login(cx: &mut VisualTestAppContext) {
    let gh_dir = tempfile::tempdir().expect("offline gh state");
    let _gh = install_gh(gh_dir.path());
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    let prs = vec![enterprise_pr(11, "ghe-me"), enterprise_pr(12, "dotcom-me")];

    // Only the github.com account is known — what the old window-global
    // login was. The Enterprise read fails (no login file yet).
    app.update(cx, |app, _| {
        app.github_host_logins
            .insert(Some("github.com".into()), "dotcom-me".into());
    });
    refresh_prs(cx, &app, prs.clone());
    wait(cx, "the failed Enterprise login read never settled", |cx| {
        !cx.read(|cx| app.read(cx).host_login_requested_for_e2e(Some(HOST)))
    });

    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).pr_section_numbers_for_e2e(PrSection::Mine))
            .is_empty(),
        "the github.com account's PR #12 is not yours on the Enterprise host"
    );
    for section in PrSection::ALL {
        assert!(
            drawn(
                cx,
                window,
                &format!("pr-mode-section-{}-count-unknown", section.index())
            ),
            "{section:?} must not claim a count before the Enterprise login is known"
        );
    }

    app.update(cx, |app, cx| {
        app.show_issues_mode(cx);
        app.seed_issue_list_for_e2e(
            BASE_REPO,
            vec![
                issue(1, "ghe-me", &[]),
                issue(2, "bob", &["ghe-me"]),
                issue(3, "dotcom-me", &[]),
                issue(4, "bob", &["dotcom-me"]),
            ],
            cx,
        );
    });
    cx.run_until_parked();
    for tab in [0, 1] {
        assert!(
            drawn(cx, window, &format!("issue-filter-tab-{tab}-count-unknown")),
            "Issues tab {tab} (yours) must not claim a count yet"
        );
    }
    for tab in [2, 3] {
        assert!(
            !drawn(cx, window, &format!("issue-filter-tab-{tab}-count-unknown")),
            "Issues tab {tab} does not depend on who you are"
        );
    }
    click(cx, window, "issue-filter-tab-0");
    assert!(
        drawn(cx, window, "issue-filter-viewer-unknown"),
        "Assigned says the account is not known, not that the list is empty"
    );
    assert!(
        !drawn(cx, window, "issue-main-row-4"),
        "Issue #4 is assigned to the github.com account, not to you here"
    );

    // The Enterprise login arrives with the next PR refresh.
    std::fs::write(gh_dir.path().join("ghe_login.txt"), "ghe-me\n").unwrap();
    refresh_prs(cx, &app, prs);
    wait(cx, "the Enterprise login was never read", |cx| {
        cx.read(|cx| {
            app.read(cx)
                .github_host_logins
                .get(&Some(HOST.into()))
                .is_some()
        })
    });

    assert!(!drawn(cx, window, "issue-filter-tab-0-count-unknown"));
    assert!(!drawn(cx, window, "issue-filter-viewer-unknown"));
    assert!(drawn(cx, window, "issue-main-row-2"), "Assigned holds #2");
    assert!(!drawn(cx, window, "issue-main-row-4"));
    click(cx, window, "issue-filter-tab-1");
    assert!(drawn(cx, window, "issue-main-row-1"), "Created holds #1");
    assert!(!drawn(cx, window, "issue-main-row-3"));

    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).pr_section_numbers_for_e2e(PrSection::Mine)),
        vec![11],
        "Mine is the Enterprise login's PR"
    );
    assert!(!drawn(cx, window, "pr-mode-section-1-count-unknown"));

    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "reading logins is not a write"
    );
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS ghe_viewer_login: Mine / Assigned / Created follow the Enterprise host's login, and nothing is claimed while it is unknown"
    );
}
