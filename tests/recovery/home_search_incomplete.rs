//! Searching Home while a list is incomplete (#1070): a search over a list
//! that failed, or was cut at `gh`'s limit, still says so. It is not
//! described as a complete search with no match. Rows read before that
//! match stay. Once a Refresh reads everything, the search the user typed
//! is kept, and a complete search with no match says no match.
//!
//! Repositories: the user's own list cut at the limit, the organizations
//! not listed, and one organization that refuses. Pull requests and issues:
//! a failed search over the lists read before, and a list cut at the limit.
use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{settings, KagiApp};
use kagi_git::github_repos::{org_list_args, repo_list_args, REPO_LIST_LIMIT};
use kagi_git::github_repos_cache::{self, WorkKind};
use kagi_git::github_search::{search_args, WORK_LIST_LIMIT};

use crate::app_conflict::click_control;
use crate::home_github::{drawn, settled, wait_for, EnvCleared};
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

/// Home's list says the search found nothing.
const NO_MATCH: &str = "home-list-no-match";
/// An owner's repository list was cut at `gh`'s limit.
const REPOS_TRUNCATED: &str = "home-github-truncated";
/// One owner's repositories could not be listed.
const OWNER_FAILED: &str = "home-github-owner-failed";
/// The organizations could not be listed at all.
const ORGS_FAILED: &str = "home-github-orgs-failed";
/// A pull request / issue search failed; the list shown is from before.
const WORK_FAILED: &str = "home-work-failed";
/// A pull request / issue list was cut at the search limit.
const WORK_TRUNCATED: &str = "home-work-truncated";
/// The user's own list could not be read again; the list shown is from
/// before (#1070 review).
const REFRESH_FAILED: &str = "home-github-refresh-failed";

/// A search that matches nothing in any list.
const NOTHING: &str = "zzz-none";

const ORG_LIST: &str = r#"[{"nameWithOwner":"acme-org/tool","url":"https://github.com/acme-org/tool","isFork":false,"isPrivate":false,"description":"","updatedAt":"2026-10-01T00:00:00Z"}]"#;

fn repo_list(names: impl IntoIterator<Item = String>) -> String {
    let repos: Vec<String> = names
        .into_iter()
        .map(|name| {
            format!(
                r#"{{"nameWithOwner":"{name}","url":"https://github.com/{name}","isFork":false,"isPrivate":false,"description":"","updatedAt":"2026-10-01T00:00:00Z"}}"#
            )
        })
        .collect();
    format!("[{}]", repos.join(","))
}

/// A pull request (`kind` = `pull`) or issue (`issues`) of acme/local, with
/// the fields `gh search` is asked for.
fn work_entry(kind: &str, number: usize, title: &str, author: &str) -> String {
    let draft = if kind == "pull" {
        r#""isDraft":false,"#
    } else {
        ""
    };
    format!(
        r#"{{"number":{number},"title":"{title}","url":"https://github.com/acme/local/{kind}/{number}",{draft}"author":{{"login":"{author}"}},"updatedAt":"2026-10-05T00:00:00Z","repository":{{"name":"local","nameWithOwner":"acme/local"}}}}"#
    )
}

/// `gh config get user` for the default host, as Home reads the account.
const ACCOUNT: &str = "config get user -h github.com";

/// Branch Cleanup's merged pull request read (`list_merged_prs`), which the
/// fixture repository's tab runs in the background. The fixture is a local
/// repository with no pull requests, so the true answer is none.
const MERGED_PRS: &str =
    "pr list --state merged --limit 200 --json number,title,headRefName,author";

/// A command line Home's reads run, as the stand-in `gh` sees it in `$*`.
fn line(args: Vec<String>) -> String {
    args.join(" ")
}

/// A stand-in `gh` for the repository list. It answers only the command
/// lines Home's reads build; any other is refused. The user's own list is
/// `state/own.json`, each read counted in `own-calls`; `fail-own` fails the
/// next read (once). `fail-orgs` fails listing the organizations;
/// `locked` makes `locked-org` refuse, as SAML enforcement does. The pull
/// request / issue searches and the merged pull requests find nothing.
fn repos_gh(state: &Path) -> String {
    let org = |login: &str| line(repo_list_args(Some(login), REPO_LIST_LIMIT));
    let searches = WorkKind::ALL
        .map(|kind| format!("'{}'", line(search_args(kind, WORK_LIST_LIMIT))))
        .join("|");
    format!(
        "#!/bin/sh\ncase \"$*\" in\n\
         '{ACCOUNT}') echo acme ;;\n\
         '{orgs}') [ -e '{state}/fail-orgs' ] && {{ echo 'HTTP 502: Bad Gateway' >&2; exit 1; }}\n\
         printf 'acme-org\\nlocked-org\\n' ;;\n\
         '{own}') echo read >> '{state}/own-calls'\n\
         [ -e '{state}/fail-own' ] && {{ rm '{state}/fail-own'; echo 'HTTP 502: Bad Gateway' >&2; exit 1; }}\n\
         cat '{state}/own.json' ;;\n\
         '{acme_org}') cat <<'JSON'\n{ORG_LIST}\nJSON\n;;\n\
         '{locked_org}') [ -e '{state}/locked' ] && {{ echo 'Resource protected by organization SAML enforcement' >&2; exit 1; }}\n\
         echo '[]' ;;\n\
         {searches}) echo '[]' ;;\n\
         '{MERGED_PRS}') echo '[]' ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n",
        orgs = line(org_list_args()),
        own = line(repo_list_args(None, REPO_LIST_LIMIT)),
        acme_org = org("acme-org"),
        locked_org = org("locked-org"),
        state = state.display()
    )
}

/// A stand-in `gh` for the pull request / issue lists. It answers only the
/// command lines Home's reads build, and the merged pull request read;
/// any other is refused. The user belongs to no organization, owns no
/// repository and has no merged pull request. Each search answers
/// `state/<list>.json` (`mine`, `review`, `issues`) and is counted in
/// `searches`; `fail-<list>` fails it.
fn work_gh(state: &Path) -> String {
    let search = |kind: WorkKind| line(search_args(kind, WORK_LIST_LIMIT));
    format!(
        "#!/bin/sh\ncase \"$*\" in\n\
         '{ACCOUNT}') echo acme; exit 0 ;;\n\
         '{orgs}') exit 0 ;;\n\
         '{own}') echo '[]'; exit 0 ;;\n\
         '{MERGED_PRS}') echo '[]'; exit 0 ;;\n\
         '{mine}') list=mine ;;\n\
         '{review}') list=review ;;\n\
         '{issues}') list=issues ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\n\
         esac\n\
         echo $list >> '{state}/searches'\n\
         [ -e '{state}/fail-'$list ] && {{ echo 'HTTP 502: Bad Gateway' >&2; exit 1; }}\n\
         cat '{state}/'$list'.json'\n",
        orgs = line(org_list_args()),
        own = line(repo_list_args(None, REPO_LIST_LIMIT)),
        mine = search(WorkKind::MyPrs),
        review = search(WorkKind::ReviewRequests),
        issues = search(WorkKind::AssignedIssues),
        state = state.display()
    )
}

/// Remove the saved lists, so a scenario starts and ends without them.
fn clear_saved_lists() {
    let dir = settings::settings_path()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .expect("the runner sets KAGI_LOG_DIR");
    let _ = std::fs::remove_file(github_repos_cache::cache_path(&dir));
    let _ = std::fs::remove_file(github_repos_cache::work_cache_path(&dir));
}

/// Type `text` into Home's search field (the real input's value).
fn search(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    text: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        let input = app
            .read(cx)
            .home_github
            .filter
            .clone()
            .expect("Home has a search field");
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

/// What Home's search field holds.
fn searched(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> String {
    cx.read(|cx| {
        let input = app.read(cx).home_github.filter.clone();
        input.map_or_else(String::new, |input| input.read(cx).value().to_string())
    })
}

/// Press Home's Refresh and wait for every read it started.
fn refresh(cx: &mut VisualTestAppContext, window: AnyWindowHandle, app: &Entity<KagiApp>) {
    click_control(cx, window, "home-github-refresh");
    wait_for(cx, app, "the refresh", settled);
}

/// Open Home from the tab strip and wait for its first read.
fn open_home(cx: &mut VisualTestAppContext, window: AnyWindowHandle, app: &Entity<KagiApp>) {
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    assert!(drawn(cx, window, "home-pane-repos"), "Home is in front");
    wait_for(cx, app, "Home's first read", settled);
}

pub fn scenario_home_search_incomplete_repos(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let start = build_fixture();
    let state_dir = tempfile::tempdir().unwrap();
    let state = state_dir.path().to_path_buf();
    // Exactly as many as `gh` is asked for: the list may go on.
    let cut = repo_list(
        std::iter::once("acme/needle".to_string())
            .chain((1..REPO_LIST_LIMIT).map(|i| format!("acme/r{i:04}"))),
    );
    std::fs::write(state.join("own.json"), cut).unwrap();
    let _gh = OfflineGh::with_script(&repos_gh(&state));
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    clear_saved_lists();
    let mark = |name: &str| std::fs::write(state.join(name), "").unwrap();
    let unmark = |name: &str| std::fs::remove_file(state.join(name)).unwrap();
    let own_reads =
        || std::fs::read_to_string(state.join("own-calls")).map_or(0, |s| s.lines().count());

    // The organizations cannot be listed and the user's own list is cut
    // at the limit. A search that matches nothing must not read as a
    // complete search with no result.
    mark("fail-orgs");
    let (app, window) = mount(cx, start.path());
    open_home(cx, window, &app);
    assert_eq!(own_reads(), 1);
    search(cx, window, &app, NOTHING);
    assert!(
        drawn(cx, window, ORGS_FAILED),
        "searching keeps saying the organizations could not be listed"
    );
    assert!(
        drawn(cx, window, REPOS_TRUNCATED),
        "searching keeps saying the user's list was cut at the limit"
    );
    assert!(
        !drawn(cx, window, NO_MATCH),
        "a search over an incomplete list is not reported as no match"
    );
    // A match in the part that was read is listed, with what is missing.
    search(cx, window, &app, "needle");
    assert!(drawn(cx, window, "home-gh-acme/needle"));
    assert!(drawn(cx, window, REPOS_TRUNCATED));
    assert!(drawn(cx, window, ORGS_FAILED));

    // The organizations are listed, but one of them refuses.
    unmark("fail-orgs");
    mark("locked");
    search(cx, window, &app, NOTHING);
    refresh(cx, window, &app);
    assert_eq!(own_reads(), 2, "Refresh read the list again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        drawn(cx, window, OWNER_FAILED),
        "searching keeps saying an organization could not be read"
    );
    assert!(!drawn(cx, window, ORGS_FAILED));
    assert!(drawn(cx, window, REPOS_TRUNCATED));
    assert!(!drawn(cx, window, NO_MATCH));

    // The user's list cannot be read again: the list read before stays,
    // its matching row and what it lacks with it.
    mark("fail-own");
    search(cx, window, &app, "needle");
    refresh(cx, window, &app);
    assert_eq!(own_reads(), 3, "Refresh tried the list again");
    assert!(
        drawn(cx, window, "home-gh-acme/needle"),
        "the matching row read before stays"
    );
    assert!(drawn(cx, window, REPOS_TRUNCATED));
    assert!(drawn(cx, window, OWNER_FAILED));

    // Everything is read in full: the search the user typed stays, and
    // nothing matching it is now a plain no match.
    std::fs::write(
        state.join("own.json"),
        repo_list(["acme/needle".to_string(), "acme/r0001".to_string()]),
    )
    .unwrap();
    unmark("locked");
    search(cx, window, &app, NOTHING);
    refresh(cx, window, &app);
    assert_eq!(own_reads(), 4, "Refresh read the list again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        drawn(cx, window, NO_MATCH),
        "a complete search with no result says no match"
    );
    assert!(!drawn(cx, window, REPOS_TRUNCATED));
    assert!(!drawn(cx, window, OWNER_FAILED));
    assert!(!drawn(cx, window, ORGS_FAILED));
    search(cx, window, &app, "needle");
    assert!(drawn(cx, window, "home-gh-acme/needle"));
    assert!(!drawn(cx, window, NO_MATCH));

    // The user's list cannot be read again over this complete list (#1070
    // review): the list read before stays, but a search over it is no longer
    // current. It keeps saying why, and does not report no match, until a
    // read succeeds.
    mark("fail-own");
    search(cx, window, &app, NOTHING);
    refresh(cx, window, &app);
    assert_eq!(own_reads(), 5, "Refresh tried the list again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        !drawn(cx, window, NO_MATCH),
        "a search over a list that could not be read again is not reported as no match"
    );
    assert!(
        drawn(cx, window, REFRESH_FAILED),
        "searching keeps saying the list could not be read again"
    );
    search(cx, window, &app, "needle");
    assert!(
        drawn(cx, window, "home-gh-acme/needle"),
        "the matching row read before stays"
    );
    assert!(drawn(cx, window, REFRESH_FAILED));
    assert!(!drawn(cx, window, NO_MATCH));
    // Until a read succeeds: leaving the pane and coming back keeps it.
    search(cx, window, &app, NOTHING);
    click_control(cx, window, "home-pane-prs");
    cx.run_until_parked();
    click_control(cx, window, "home-pane-repos");
    cx.run_until_parked();
    assert!(drawn(cx, window, REFRESH_FAILED));
    assert!(!drawn(cx, window, NO_MATCH));
    // The next read succeeds: the same search is a plain no match again.
    refresh(cx, window, &app);
    assert_eq!(own_reads(), 6, "Refresh read the list again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        drawn(cx, window, NO_MATCH),
        "a complete search with no result says no match"
    );
    assert!(!drawn(cx, window, REFRESH_FAILED));

    unmount(cx, app, window);
    clear_saved_lists();
}

/// A Refresh over a complete repository list, observed before the test
/// dispatcher runs its reads (#1070 review): the list stays with its
/// matching rows, and a search over it is not reported as no match until
/// the read lands.
pub fn scenario_home_search_incomplete_refreshing(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let start = build_fixture();
    let state_dir = tempfile::tempdir().unwrap();
    let state = state_dir.path().to_path_buf();
    std::fs::write(
        state.join("own.json"),
        repo_list(["acme/needle".to_string(), "acme/r0001".to_string()]),
    )
    .unwrap();
    let _gh = OfflineGh::with_script(&repos_gh(&state));
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    clear_saved_lists();
    let own_reads =
        || std::fs::read_to_string(state.join("own-calls")).map_or(0, |s| s.lines().count());

    let (app, window) = mount(cx, start.path());
    open_home(cx, window, &app);
    assert_eq!(own_reads(), 1);
    search(cx, window, &app, NOTHING);
    assert!(
        drawn(cx, window, NO_MATCH),
        "precondition: a complete search with no result says no match"
    );

    // What Refresh's button calls, drawn before the pump runs the reads (a
    // simulated click parks the executor, which would finish them first).
    app.update(cx, |app, cx| app.reload_home_github(cx));
    assert!(
        drawn(cx, window, "home-github-updating"),
        "the read is running"
    );
    assert!(
        !drawn(cx, window, NO_MATCH),
        "a search while the list is read again is not reported as no match"
    );
    wait_for(cx, &app, "the refresh", settled);
    assert_eq!(own_reads(), 2, "the refresh read the list");
    assert!(
        drawn(cx, window, NO_MATCH),
        "once it lands, the complete search says no match again"
    );

    search(cx, window, &app, "needle");
    app.update(cx, |app, cx| app.reload_home_github(cx));
    assert!(
        drawn(cx, window, "home-github-updating"),
        "the read is running"
    );
    assert!(
        drawn(cx, window, "home-gh-acme/needle"),
        "the matching row stays while the list is read again"
    );
    assert!(!drawn(cx, window, NO_MATCH));
    wait_for(cx, &app, "the second refresh", settled);
    assert_eq!(own_reads(), 3, "the refresh read the list");
    assert!(drawn(cx, window, "home-gh-acme/needle"));

    unmount(cx, app, window);
    clear_saved_lists();
}

pub fn scenario_home_search_incomplete_work(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let start = build_fixture();
    let state_dir = tempfile::tempdir().unwrap();
    let state = state_dir.path().to_path_buf();
    let write = |list: &str, entries: Vec<String>| {
        std::fs::write(
            state.join(format!("{list}.json")),
            format!("[{}]", entries.join(",")),
        )
        .unwrap()
    };
    write(
        "mine",
        vec![work_entry("pull", 7, "Fix the local thing", "acme")],
    );
    write(
        "review",
        vec![work_entry("pull", 9, "Please review", "octo")],
    );
    write(
        "issues",
        vec![work_entry("issues", 4, "Crash on start", "octo")],
    );
    let _gh = OfflineGh::with_script(&work_gh(&state));
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    clear_saved_lists();
    let mark = |name: &str| std::fs::write(state.join(name), "").unwrap();
    let unmark = |name: &str| std::fs::remove_file(state.join(name)).unwrap();
    let searches =
        || std::fs::read_to_string(state.join("searches")).map_or(0, |s| s.lines().count());

    let (app, window) = mount(cx, start.path());
    open_home(cx, window, &app);
    assert_eq!(searches(), 3);
    click_control(cx, window, "home-pane-prs");
    cx.run_until_parked();
    assert!(drawn(cx, window, "home-work-acme/local-9"));

    // The review requests and the issues cannot be searched again: the
    // lists read before stay under their errors.
    mark("fail-review");
    mark("fail-issues");
    refresh(cx, window, &app);
    assert_eq!(searches(), 6, "Refresh searched again");
    search(cx, window, &app, "please");
    assert!(
        drawn(cx, window, "home-work-acme/local-9"),
        "the matching review request read before stays"
    );
    assert!(drawn(cx, window, WORK_FAILED));
    search(cx, window, &app, NOTHING);
    assert!(
        drawn(cx, window, WORK_FAILED),
        "searching keeps saying the review requests could not be searched"
    );
    assert!(
        !drawn(cx, window, NO_MATCH),
        "a search over a list that failed is not reported as no match"
    );
    click_control(cx, window, "home-pane-issues");
    cx.run_until_parked();
    assert_eq!(searched(cx, &app), NOTHING, "the search spans the panes");
    assert!(
        drawn(cx, window, WORK_FAILED),
        "searching keeps saying the issues could not be searched"
    );
    assert!(!drawn(cx, window, NO_MATCH));

    // The issues are cut at the search limit, none matching: incomplete.
    unmark("fail-review");
    unmark("fail-issues");
    write(
        "issues",
        (1..=WORK_LIST_LIMIT)
            .map(|n| work_entry("issues", n, &format!("Assigned {n}"), "octo"))
            .collect(),
    );
    refresh(cx, window, &app);
    assert_eq!(searches(), 9, "Refresh searched again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        drawn(cx, window, WORK_TRUNCATED),
        "searching keeps saying the issues were cut at the limit"
    );
    assert!(!drawn(cx, window, WORK_FAILED));
    assert!(!drawn(cx, window, NO_MATCH));
    // The pull requests were read in full: nothing matching is no match.
    click_control(cx, window, "home-pane-prs");
    cx.run_until_parked();
    assert!(
        drawn(cx, window, NO_MATCH),
        "a complete search with no result says no match"
    );
    assert!(!drawn(cx, window, WORK_FAILED));
    assert!(!drawn(cx, window, WORK_TRUNCATED));

    // The issues are read in full: the search stays, and is a no match.
    write(
        "issues",
        vec![work_entry("issues", 4, "Crash on start", "octo")],
    );
    click_control(cx, window, "home-pane-issues");
    cx.run_until_parked();
    refresh(cx, window, &app);
    assert_eq!(searches(), 12, "Refresh searched again");
    assert_eq!(searched(cx, &app), NOTHING, "Refresh keeps the search");
    assert!(
        drawn(cx, window, NO_MATCH),
        "a complete search with no result says no match"
    );
    assert!(!drawn(cx, window, WORK_TRUNCATED));
    assert!(!drawn(cx, window, WORK_FAILED));

    // A Refresh over these complete lists, observed before the test
    // dispatcher runs the searches (#1070 review): the lists stay with their
    // matching rows, and the search is not reported as no match until the
    // searches land. What Refresh's button calls, without the click's pump.
    app.update(cx, |app, cx| app.reload_home_github(cx));
    assert!(
        drawn(cx, window, "home-github-updating"),
        "the searches run"
    );
    assert!(
        !drawn(cx, window, NO_MATCH),
        "a search while the lists are read again is not reported as no match"
    );
    wait_for(cx, &app, "the refresh", settled);
    assert_eq!(searches(), 15, "the refresh searched again");
    assert!(
        drawn(cx, window, NO_MATCH),
        "once they land, the complete search says no match again"
    );
    search(cx, window, &app, "crash");
    app.update(cx, |app, cx| app.reload_home_github(cx));
    assert!(
        drawn(cx, window, "home-github-updating"),
        "the searches run"
    );
    assert!(
        drawn(cx, window, "home-work-acme/local-4"),
        "the matching issue stays while the lists are read again"
    );
    assert!(!drawn(cx, window, NO_MATCH));
    wait_for(cx, &app, "the second refresh", settled);
    assert_eq!(searches(), 18, "the refresh searched again");
    assert!(drawn(cx, window, "home-work-acme/local-4"));

    unmount(cx, app, window);
    clear_saved_lists();
}
