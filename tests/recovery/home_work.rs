//! Home's pull requests and issues (#928, ADR-0219), with a stand-in `gh`
//! whose searches name a repository cloned locally and one that is not.
//!
//! The switch shows no count before anything is read; the saved lists are
//! shown only for the account they were read as; a PR of the local clone
//! opens there once `gh pr view` has its refs, and clicks meanwhile are
//! ignored; an issue opens in Issues mode; a row of a repository without a
//! clone opens on GitHub, and every row's Open button opens it on GitHub
//! without starting the row's own open; one failed search keeps the others
//! and says why, and nothing incomplete is saved.
use std::path::Path;

use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::workspace_mode::WorkspaceMode;
use kagi::ui::{e2e, settings, tabs, KagiApp};
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
/// `fail-review` the review-request search; `default-repo` names the URL
/// `gh repo view` resolves the clone to (`gh repo set-default`), else
/// `acme/local`. Each `pr view` asking for the
/// refs Home opens a PR with is counted in `view-calls` (PR mode's own
/// detail reads are not). The test dispatcher runs `gh` inside its pump, so
/// a state between a read's start and its end is observed by drawing before
/// the pump runs, not by holding `gh`.
fn gh_script(state: &Path) -> String {
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') cat '{state}/user' 2>/dev/null || echo acme ;;\n\
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
         'repo view --json') printf '{{\"url\":\"%s\"}}\\n' \"$(cat '{state}/default-repo' 2>/dev/null || echo https://github.com/acme/local)\" ;;\n\
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

/// Pick `item` and press the control at `at` in one window update, without
/// the pump in between: the pick's `gh pr view` cannot finish before the
/// press (the test dispatcher runs it inside the pump).
fn pick_then_press(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<KagiApp>,
    item: &WorkItem,
    at: gpui::Point<gpui::Pixels>,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.home_work_pick(WorkKind::MyPrs, item.clone(), window, cx)
        });
        let modifiers = gpui::Modifiers::none();
        for input in [
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers,
            }),
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                position: at,
                modifiers,
                button: gpui::MouseButton::Left,
                click_count: 1,
                first_mouse: false,
            }),
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                position: at,
                modifiers,
                button: gpui::MouseButton::Left,
                click_count: 1,
            }),
        ] {
            window.dispatch_event(input, cx);
        }
    })
    .unwrap();
}

/// An Issues list read that names `base_repo` and holds no issues.
fn issue_list(base_repo: &str) -> kagi_domain::github::IssueListSnapshot {
    kagi_domain::github::IssueListSnapshot {
        issues: Vec::new(),
        mentioned_numbers: Vec::new(),
        base_repo: base_repo.into(),
        next_cursor: None,
    }
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

    // Before anything is read the switch shows a spinner where the count
    // will be — never 0 — and the pane a spinner row. Home opens and draws
    // without the pump running, so the reads have not run yet.
    let saved = |title: &str| WorkLists {
        my_prs: one(1, title),
        ..WorkLists::default()
    };
    github_repos_cache::save_work(&cache, "github.com/someone-else", &saved("theirs")).unwrap();
    mark("fail-search");
    let (app, window) = mount(cx, start.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.home_github.work.pane = HomePane::Prs;
            app.open_home_tab(window, cx);
        })
    })
    .unwrap();
    assert!(
        drawn(cx, window, "home-pane-prs-spinner"),
        "no count before the lists are read"
    );
    assert!(drawn(cx, window, "home-work-loading"));

    // Lists saved as another account are not shown, even when every search
    // fails: each list says why instead.
    wait_for(cx, &app, "the failed searches", settled);
    assert!(!drawn(cx, window, "home-pane-prs-spinner"));
    assert!(drawn(cx, window, "home-work-failed"));
    assert!(
        !drawn(cx, window, "home-work-acme/local-1"),
        "another account's saved lists are not shown"
    );

    // `gh auth switch` to that account: its saved lists replace the ones
    // read as the previous account, and stay when its searches fail.
    std::fs::write(state.join("user"), "someone-else\n").unwrap();
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the other account's lists", settled);
    assert!(
        drawn(cx, window, "home-work-acme/local-1"),
        "the signed-in account's saved lists are shown and kept when gh fails"
    );

    std::fs::remove_file(state.join("user")).unwrap();
    unmark("fail-search");
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the lists", settled);
    wait_for(cx, &app, "the local clone", |app| {
        app.home_github.local.contains_key("github.com/acme/local")
    });
    assert!(!drawn(cx, window, "home-work-failed"));
    assert!(
        !drawn(cx, window, "home-work-acme/local-1"),
        "the lists read as the previous account are dropped"
    );
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

    // #940 review: a repository without a clone opens on GitHub, at the URL
    // the search returned — not the clone card.
    let _ = e2e::take_opened_urls();
    click(cx, window, "home-work-acme/widgets-3");
    assert_eq!(
        e2e::take_opened_urls(),
        vec!["https://github.com/acme/widgets/pull/3".to_string()],
        "a PR of a repository not cloned opens on GitHub"
    );
    assert!(
        cx.read(|cx| app.read(cx).clone_modal().is_none()),
        "the clone card belongs to the Repositories tab"
    );
    // #944: the click left the row focused; Enter and Space press it again,
    // as gpui's keyboard click on the row's own handler.
    for key in ["enter", "space"] {
        crate::keyboard_nav::keys(cx, window, key);
        assert_eq!(
            e2e::take_opened_urls(),
            vec!["https://github.com/acme/widgets/pull/3".to_string()],
            "{key} presses the focused row"
        );
    }
    // Every row's end is Open (on GitHub), cloned or not, and pressing it
    // does not also run the row's click: no refs read, no spinner.
    click(cx, window, "home-work-acme/widgets-3-open");
    click(cx, window, "home-work-acme/local-7-open");
    assert_eq!(
        e2e::take_opened_urls(),
        vec![
            "https://github.com/acme/widgets/pull/3".to_string(),
            "https://github.com/acme/local/pull/7".to_string(),
        ],
    );
    assert_eq!(
        view_calls(),
        0,
        "Open on a cloned PR does not open it locally"
    );
    assert!(cx.read(|cx| app.read(cx).home_github.work.opening.is_none()));
    assert!(
        cx.read(|cx| app.read(cx).home.is_some()),
        "Home stays in front"
    );

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

    // #940 review: the later pick wins. A local PR still waiting for
    // `gh pr view` is dropped when another row is picked meanwhile — here
    // one without a clone, which opens on GitHub; the first must not then
    // move the user to its repository when its refs arrive.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    let (local_pr, remote_pr) = cx.read(|cx| {
        let items = &app
            .read(cx)
            .home_github
            .work
            .lists
            .as_ref()
            .unwrap()
            .my_prs
            .items;
        (items[0].clone(), items[1].clone())
    });
    assert_eq!((local_pr.number, remote_pr.number), (7, 3));
    let _ = e2e::take_opened_urls();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.home_work_pick(WorkKind::MyPrs, local_pr.clone(), window, cx);
            app.home_work_pick(WorkKind::MyPrs, remote_pr.clone(), window, cx);
        })
    })
    .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).home_github.work.opening.is_none()),
        "the later pick drops the open still in progress"
    );
    wait_for(cx, &app, "the dropped refs read", |_| view_calls() == 2);
    cx.run_until_parked();
    assert_eq!(
        e2e::take_opened_urls(),
        vec!["https://github.com/acme/widgets/pull/3".to_string()]
    );
    assert!(
        cx.read(|cx| app.read(cx).home.is_some()),
        "Home stays in front: the dropped PR does not open"
    );

    // The same for a row's Open button (#940 review): pressed while a local
    // PR is still opening, it opens its row on GitHub and drops that open.
    // The pick and the press go in without the pump in between, so the
    // refs read cannot finish first.
    assert!(drawn(cx, window, "home-work-acme/widgets-3-open"));
    let open_at = e2e::control_bounds(window.window_id(), "home-work-acme/widgets-3-open")
        .unwrap()
        .center();
    pick_then_press(cx, window, &app, &local_pr, open_at);
    assert!(
        cx.read(|cx| app.read(cx).home_github.work.opening.is_none()),
        "pressing Open drops the open still in progress"
    );
    wait_for(cx, &app, "the second dropped refs read", |_| {
        view_calls() == 3
    });
    cx.run_until_parked();
    assert_eq!(
        e2e::take_opened_urls(),
        vec!["https://github.com/acme/widgets/pull/3".to_string()]
    );
    assert!(
        cx.read(|cx| app.read(cx).home.is_some()),
        "Home stays in front after Open: the dropped PR does not open"
    );

    // The same for switching pane (#940 review): the pending open is dropped
    // and the user stays on the pane they chose.
    assert!(drawn(cx, window, "home-pane-issues"));
    let pane_at = e2e::control_bounds(window.window_id(), "home-pane-issues")
        .unwrap()
        .center();
    pick_then_press(cx, window, &app, &local_pr, pane_at);
    assert!(
        cx.read(|cx| app.read(cx).home_github.work.opening.is_none()),
        "switching pane drops the open still in progress"
    );
    wait_for(cx, &app, "the third dropped refs read", |_| {
        view_calls() == 4
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            app.home.is_some(),
            "Home stays in front after the pane switch"
        );
        assert_eq!(app.home_github.work.pane, HomePane::Issues);
    });

    // An issue opens in its clone's Issues mode only while that mode
    // addresses the issue's repository: with `gh repo set-default` pointing
    // elsewhere the same number is another issue there.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    click_control(cx, window, "home-pane-issues");
    // An issue row's Open is on GitHub too, at the issue's own URL.
    click(cx, window, "home-work-acme/local-4-open");
    assert_eq!(
        e2e::take_opened_urls(),
        vec!["https://github.com/acme/local/issues/4".to_string()]
    );
    assert!(cx.read(|cx| app.read(cx).home.is_some()));
    std::fs::write(
        state.join("default-repo"),
        "https://github.com/acme/upstream",
    )
    .unwrap();
    click(cx, window, "home-work-acme/local-4");
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.home.is_some(), "Home stays in front");
        let toasts = app.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert!(
            toasts.iter().any(|t| t.message.contains("acme/upstream")),
            "it says which repository the clone addresses"
        );
    });
    // #940 review: the clone's Issues mode is loaded while `gh` resolves it
    // to acme/upstream, so its list and Reply address acme/upstream. Picking
    // acme/local's #4 once `gh` resolves the clone to acme/local re-reads the
    // mode for acme/local before selecting, so a reply cannot land on
    // acme/upstream's #4.
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(issue_list(
        "github.com/acme/upstream",
    ))));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(local_path.clone(), cx));
        app.show_issues_mode(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).issue_write_repo_for_e2e()),
        Some("github.com/acme/upstream".to_string())
    );
    // A reply to acme/upstream's #4 is drafted there; it is that issue's.
    app.update(cx, |app, cx| app.seed_issue_reply_for_e2e(4, cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.insert_issue_reply_body_for_e2e(4, "for upstream #4", window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    let clone = cx.read(|cx| app.read(cx).repo_path.clone().unwrap());
    let upstream_reply = || {
        kagi_git::drafts::load_issue_draft(&clone, "github.com/acme/upstream", Some(4))
            .record
            .map(|draft| draft.body)
    };
    assert_eq!(upstream_reply(), Some("for upstream #4".to_string()));
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(issue_list(
        "github.com/acme/local",
    ))));
    std::fs::remove_file(state.join("default-repo")).unwrap();
    click(cx, window, "home-work-acme/local-4");
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap(),
            local_path
        );
        assert_eq!(app.workspace_mode(), WorkspaceMode::Issues);
        assert_eq!(app.ui().selected_github_issue, Some(4));
        assert_eq!(
            app.issue_write_repo_for_e2e(),
            Some("github.com/acme/local".to_string()),
            "the Reply goes to the verified repository"
        );
        assert_eq!(
            app.issue_reply_draft_for_e2e(4).body,
            "",
            "acme/upstream's reply is not offered for acme/local's #4"
        );
    });
    assert_eq!(
        upstream_reply(),
        Some("for upstream #4".to_string()),
        "and it stays acme/upstream's"
    );

    unmount(cx, app, window);
}
