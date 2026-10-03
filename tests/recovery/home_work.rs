//! Home's pull requests and issues (#928, ADR-0219), with a stand-in `gh`
//! whose searches name a repository cloned locally and one that is not.
//!
//! The switch shows no count before anything is read; the saved lists are
//! shown only for the account they were read as; a PR of the local clone
//! opens there once `gh pr view` has its refs, and clicks meanwhile are
//! ignored; an issue opens in Issues mode; a row of a repository without a
//! clone offers the clone card; one failed search keeps the others and says
//! why, and nothing incomplete is saved.
use std::path::Path;

use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::workspace_mode::WorkspaceMode;
use kagi::ui::{e2e, settings, tabs};
use kagi_git::github_repos_cache::{self, WorkItem, WorkKind, WorkList, WorkLists};

use crate::app_conflict::click_control;
use crate::home_github::{drawn, settled, wait_for, EnvCleared};
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

const MY_PRS: &str = r#"[
 {"number":7,"title":"Fix the local thing","url":"https://github.com/acme/local/pull/7",
  "isDraft":false,"author":{"login":"acme"},"updatedAt":"2026-10-05T00:00:00Z",
  "repository":{"name":"local","nameWithOwner":"acme/local"}},
 {"number":3,"title":"Widgets draft","url":"https://github.com/acme/widgets/pull/3",
  "isDraft":true,"author":{"login":"acme"},"updatedAt":"2026-10-04T00:00:00Z",
  "repository":{"name":"widgets","nameWithOwner":"acme/widgets"}}
]"#;

const REVIEW: &str = r#"[
 {"number":9,"title":"Please review","url":"https://github.com/acme/local/pull/9",
  "isDraft":false,"author":{"login":"octo"},"updatedAt":"2026-10-06T00:00:00Z",
  "repository":{"name":"local","nameWithOwner":"acme/local"}}
]"#;

const ISSUES: &str = r#"[
 {"number":4,"title":"Crash on start","url":"https://github.com/acme/local/issues/4",
  "author":{"login":"octo"},"updatedAt":"2026-10-03T00:00:00Z",
  "repository":{"name":"local","nameWithOwner":"acme/local"}}
]"#;

const PR_VIEW: &str = r#"{"number":7,"title":"Fix the local thing",
 "url":"https://github.com/acme/local/pull/7","state":"OPEN","isDraft":false,
 "isCrossRepository":false,"createdAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-05T00:00:00Z",
 "headRefName":"fix","headRefOid":"0123456789abcdef0123456789abcdef01234567",
 "baseRefName":"main","reviewDecision":"","author":{"login":"acme"},
 "assignees":[],"labels":[],"reviewRequests":[]}"#;

/// A stand-in `gh`. In `state`: `fail-search` fails every search,
/// `fail-review` the review-request search. Each `pr view` asking for the
/// refs Home opens a PR with is counted in `view-calls` (PR mode's own
/// detail reads are not). The test dispatcher runs `gh` inside its pump, so
/// a state between a read's start and its end is observed by drawing before
/// the pump runs, not by holding `gh`.
fn gh_script(state: &Path) -> String {
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo acme ;;\n\
         'api user/orgs '*) ;;\n\
         'repo list --limit') echo '[]' ;;\n\
         'search '*) [ -e '{state}/fail-search' ] && {{ echo 'HTTP 502: Bad Gateway' >&2; exit 1; }}\n\
         case \"$2 $3\" in\n\
         'prs --author=@me') cat <<'JSON'\n{MY_PRS}\nJSON\n;;\n\
         'prs --review-requested=@me')\n\
         [ -e '{state}/fail-review' ] && {{ echo 'HTTP 502: Bad Gateway' >&2; exit 1; }}\n\
         cat <<'JSON'\n{REVIEW}\nJSON\n;;\n\
         'issues --assignee=@me') cat <<'JSON'\n{ISSUES}\nJSON\n;;\n\
         esac ;;\n\
         'pr view -R') case \"$*\" in *reviewRequests*) echo \"$*\" >> '{state}/view-calls' ;; esac\n\
         cat <<'JSON'\n{PR_VIEW}\nJSON\n;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n",
        state = state.display()
    )
}

/// Click a measured control by a name built at run time.
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    assert!(drawn(cx, window, name), "{name} is on screen");
    let bounds = e2e::control_bounds(window.window_id(), name).unwrap();
    cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn one(number: u64, title: &str) -> WorkList {
    WorkList {
        items: vec![WorkItem {
            host: "github.com".into(),
            name_with_owner: "acme/local".into(),
            number,
            title: title.into(),
            url: format!("https://github.com/acme/local/pull/{number}"),
            is_draft: false,
            author: "acme".into(),
            updated_at: String::new(),
        }],
        truncated: false,
    }
}

pub fn scenario_home_work(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let start = build_fixture();
    let local = build_fixture();
    git(
        local.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/local.git",
        ],
    );
    let local_path = local.path().canonicalize().unwrap();
    tabs::record_recent_repo(&local_path);
    let state_dir = tempfile::tempdir().unwrap();
    let state = state_dir.path().to_path_buf();
    let _gh = OfflineGh::with_script(&gh_script(&state));
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    let mark = |name: &str| std::fs::write(state.join(name), "").unwrap();
    let unmark = |name: &str| std::fs::remove_file(state.join(name)).unwrap();
    let view_calls =
        || std::fs::read_to_string(state.join("view-calls")).map_or(0, |s| s.lines().count());
    let cache = github_repos_cache::work_cache_path(
        settings::settings_path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .expect("the runner sets KAGI_LOG_DIR")
            .as_path(),
    );
    let _ = std::fs::remove_file(&cache);

    // Lists saved as another account are not shown, even when every search
    // fails: each list says why instead.
    let saved = |title: &str| WorkLists {
        my_prs: one(1, title),
        ..WorkLists::default()
    };
    github_repos_cache::save_work(&cache, "github.com/someone-else", &saved("theirs")).unwrap();
    mark("fail-search");
    let (app, window) = mount(cx, start.path());
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    wait_for(cx, &app, "the failed searches", settled);
    click_control(cx, window, "home-pane-prs");
    assert_eq!(
        cx.read(|cx| app.read(cx).home_github.work.pane),
        HomePane::Prs
    );
    assert!(drawn(cx, window, "home-work-failed"));
    assert!(
        !drawn(cx, window, "home-work-acme/local-1"),
        "another account's saved lists are not shown"
    );

    // Before anything is read the switch shows a spinner where the count
    // will be — never 0 — and the pane a spinner row; then the lists saved
    // for this account, kept when the searches fail.
    github_repos_cache::save_work(&cache, "github.com/acme", &saved("mine")).unwrap();
    app.update(cx, |app, cx| {
        app.home_github.work.lists = None;
        app.reload_home_github(cx);
    });
    assert!(
        drawn(cx, window, "home-pane-prs-spinner"),
        "no count before the lists are read"
    );
    assert!(drawn(cx, window, "home-work-loading"));
    wait_for(cx, &app, "the saved lists", settled);
    assert!(!drawn(cx, window, "home-pane-prs-spinner"));
    assert!(
        drawn(cx, window, "home-work-acme/local-1"),
        "this account's saved lists are shown and kept when gh fails"
    );

    unmark("fail-search");
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the lists", settled);
    wait_for(cx, &app, "the local clone", |app| {
        app.home_github.local.contains_key("github.com/acme/local")
    });
    assert!(!drawn(cx, window, "home-work-failed"));
    assert!(!drawn(cx, window, "home-work-acme/local-1"));
    assert!(drawn(cx, window, "home-work-acme/local-7"));
    assert!(drawn(cx, window, "home-work-acme/widgets-3"));
    assert!(
        drawn(cx, window, "home-work-acme/local-9"),
        "review requests are listed with the user's own"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while github_repos_cache::load_work(&cache, "github.com/acme")
        .is_none_or(|saved| saved.get(WorkKind::MyPrs).items.len() != 2)
    {
        assert!(std::time::Instant::now() < deadline, "the lists are saved");
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // One search fails: the others land, it says why, the list read before
    // stays, and the incomplete set is not saved over the complete one.
    mark("fail-review");
    std::fs::write(&cache, "").unwrap();
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the failed review search", settled);
    assert!(drawn(cx, window, "home-work-failed"));
    assert!(
        drawn(cx, window, "home-work-acme/local-9"),
        "the review requests read before stay under the error"
    );
    cx.run_until_parked();
    assert!(
        github_repos_cache::load_work(&cache, "github.com/acme").is_none(),
        "lists with a failed search are not saved"
    );
    unmark("fail-review");
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the lists again", settled);
    assert!(!drawn(cx, window, "home-work-failed"));

    // A repository without a clone: the clone card.
    click(cx, window, "home-work-acme/widgets-3");
    assert_eq!(
        cx.read(|cx| app.read(cx).clone_modal().map(|m| m.listing.clone_source())),
        Some("github.com/acme/widgets".to_string()),
        "a PR of a repository not cloned offers its clone"
    );
    app.update(cx, |app, cx| {
        app.cancel_clone();
        cx.notify();
    });
    cx.run_until_parked();

    // The local clone's PR: a spinner while `gh pr view` runs, a second
    // click meanwhile is ignored, then the PR opens in the clone's tab.
    let item = cx.read(|cx| {
        let work = &app.read(cx).home_github.work;
        work.lists.as_ref().unwrap().my_prs.items[0].clone()
    });
    assert_eq!(item.number, 7);
    let pick = |cx: &mut VisualTestAppContext| {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.home_work_pick(WorkKind::MyPrs, item.clone(), window, cx)
            })
        })
        .unwrap();
    };
    pick(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).home_github.work.opening.clone()),
        Some(("github.com/acme/local".to_string(), 7))
    );
    assert!(drawn(cx, window, "home-work-acme/local-7-opening"));
    pick(cx);
    wait_for(cx, &app, "the PR to open", |app| {
        app.home_github.work.opening.is_none()
    });
    assert_eq!(
        view_calls(),
        1,
        "a click while opening is ignored: {:?}",
        std::fs::read_to_string(state.join("view-calls"))
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap(),
            local_path,
            "the PR opens in its local clone"
        );
        let mode = app.pr_mode().expect("PR mode");
        let tab = &mode.tabs[mode.active.expect("an open PR")];
        assert_eq!(tab.pr.number, 7);
        assert_eq!(tab.pr.head, "fix");
    });

    // An issue opens in Issues mode, selected.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    click_control(cx, window, "home-pane-issues");
    click(cx, window, "home-work-acme/local-4");
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap(),
            local_path
        );
        assert_eq!(app.workspace_mode(), WorkspaceMode::Issues);
        assert_eq!(app.ui().selected_github_issue, Some(4));
    });

    unmount(cx, app, window);
}
