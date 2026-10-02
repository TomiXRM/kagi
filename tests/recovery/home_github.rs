//! Home's GitHub list and clone (#923 PR3, ADR-0219), with a stand-in `gh`
//! whose `repo list` names one repository cloned locally and one that is
//! not, and whose `repo clone` really clones a local bare repository.
//!
//! The local one opens its tab; the other opens the clone card, which refuses
//! a destination that is already in use (no Clone button) and, once it is
//! free, clones, records a receipt, remembers the folder and opens the new
//! repository in place of Home.
use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::home_github::GithubRepos;
use kagi::ui::{e2e, settings, tabs, KagiApp};
use kagi_git::github_repos::{OwnerRepos, RepoList, RepoListing};
use kagi_git::github_repos_cache;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::app_conflict::click_control;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use crate::recovery_operations::press_key;

const REPO_LIST: &str = r#"[
 {"nameWithOwner":"acme/local","url":"https://github.com/acme/local","isFork":false,
  "isPrivate":false,"description":"already cloned","updatedAt":"2026-10-01T00:00:00Z"},
 {"nameWithOwner":"acme/widgets","url":"https://github.com/acme/widgets","isFork":true,
  "isPrivate":true,"description":"not here yet","updatedAt":"2026-10-02T00:00:00Z"}
]"#;

const ORG_LIST: &str = r#"[
 {"nameWithOwner":"acme-org/tool","url":"https://github.com/acme-org/tool","isFork":false,
  "isPrivate":true,"description":"an organization's","updatedAt":"2026-10-03T00:00:00Z"}
]"#;

fn gh_script(bare: &Path) -> String {
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'api user/orgs '*) printf 'acme-org\\nlocked-org\\n' ;;\n\
         'repo list --limit') cat <<'JSON'\n{REPO_LIST}\nJSON\n;;\n\
         'repo list acme-org') cat <<'JSON'\n{ORG_LIST}\nJSON\n;;\n\
         'repo list locked-org') echo 'Resource protected by organization SAML enforcement' >&2; exit 1 ;;\n\
         'repo clone '*) git clone -q '{bare}' \"$4\" && \
         git -C \"$4\" remote set-url origin https://github.com/acme/widgets.git ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n",
        bare = bare.display()
    )
}

fn wait_for(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    what: &str,
    done: impl Fn(&KagiApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| done(app.read(cx))) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn active_path(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> std::path::PathBuf {
    cx.read(|cx| {
        let app = app.read(cx);
        std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap()
    })
}

/// Draw, then say whether `name` was laid out in this frame.
fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) -> bool {
    e2e::clear_control_bounds(window.window_id(), name);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(window.window_id(), name).is_some()
}

pub fn scenario_home_github(cx: &mut VisualTestAppContext) {
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
    let upstream = build_fixture();
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = bare_dir.path().join("widgets.git");
    git(
        upstream.path(),
        &["clone", "-q", "--bare", ".", bare.to_str().unwrap()],
    );
    let clones_dir = tempfile::tempdir().unwrap();
    let clones = clones_dir.path().canonicalize().unwrap();
    tabs::record_recent_repo(&local_path);
    let _gh = OfflineGh::with_script(&gh_script(&bare));

    // The last read is saved: shown at once while the fresh read runs.
    let cache = github_repos_cache::cache_path(
        settings::settings_path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .expect("the runner sets KAGI_LOG_DIR")
            .as_path(),
    );
    let stale = vec![OwnerRepos {
        owner: None,
        list: Ok(RepoList {
            truncated: false,
            repos: vec![RepoListing {
                name_with_owner: "acme/stale".into(),
                host: "github.com".into(),
                is_fork: false,
                is_private: false,
                description: String::new(),
                updated_at: String::new(),
            }],
        }),
    }];
    github_repos_cache::save(&cache, &stale).unwrap();
    let (app, window) = mount(cx, start.path());
    app.update(cx, |app, cx| app.reload_home_github(cx));
    cx.read(|cx| {
        let home = &app.read(cx).home_github;
        assert!(home.refreshing, "a fresh read runs behind the saved list");
        assert!(
            matches!(&home.repos, GithubRepos::Loaded { sections, .. } if *sections == stale),
            "the saved list is on screen before gh answers"
        );
    });
    wait_for(cx, &app, "the GitHub list", |app| {
        matches!(app.home_github.repos, GithubRepos::Loaded { .. })
            && !app.home_github.refreshing
            && !app.home_github.orgs_loading
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while github_repos_cache::load(&cache).is_none_or(|saved| saved.len() != 3) {
        assert!(Instant::now() < deadline, "the fresh list is saved");
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    // Refresh keeps the list on screen and marks it as being updated.
    app.update(cx, |app, cx| app.reload_home_github(cx));
    assert!(
        drawn(cx, window, "home-github-updating"),
        "a refresh says so"
    );
    assert!(
        drawn(cx, window, "home-gh-acme/widgets"),
        "the list stays while it refreshes"
    );
    wait_for(cx, &app, "the refresh", |app| {
        !app.home_github.refreshing && !app.home_github.orgs_loading
    });
    assert!(!drawn(cx, window, "home-github-updating"));
    cx.read(|cx| {
        let GithubRepos::Loaded { sections, local } = &app.read(cx).home_github.repos else {
            unreachable!()
        };
        let owners: Vec<_> = sections.iter().map(|s| s.owner.as_deref()).collect();
        assert_eq!(owners, [None, Some("acme-org"), Some("locked-org")]);
        assert_eq!(sections[0].list.as_ref().unwrap().repos.len(), 2);
        assert_eq!(
            sections[1].list.as_ref().unwrap().repos[0].name_with_owner,
            "acme-org/tool",
            "the organization's repositories are listed too"
        );
        assert!(
            sections[2].list.as_ref().is_err_and(|e| e.contains("SAML")),
            "an organization that cannot be read keeps its section and says why"
        );
        assert_eq!(
            local.get("github.com/acme/local"),
            Some(&local_path),
            "a recent repository whose origin is listed is known as its clone"
        );
    });

    // The filter spans every owner and drops what does not match.
    let set_filter = |cx: &mut VisualTestAppContext, text: &'static str| {
        cx.update_window(window, |_, window, cx| {
            let input = app
                .read(cx)
                .home_github
                .filter
                .clone()
                .expect("Home has a filter");
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        })
        .unwrap();
        cx.run_until_parked();
    };
    set_filter(cx, "TOOL");
    assert!(
        drawn(cx, window, "home-gh-acme-org/tool"),
        "matches across owners, any case"
    );
    assert!(!drawn(cx, window, "home-gh-acme/widgets"));
    set_filter(cx, "");
    assert!(drawn(cx, window, "home-gh-acme/widgets"));

    // An organization's repository is cloned from its own owner.
    click_control(cx, window, "home-gh-acme-org/tool");
    cx.run_until_parked();
    choose_folder(cx, &app, &clones);
    assert_eq!(
        cx.read(|cx| {
            app.read(cx)
                .clone_modal()
                .and_then(|m| m.target.as_ref().map(|t| t.request.source.clone()))
        }),
        Some("github.com/acme-org/tool".to_string())
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    // Known locally: the row opens it, in place of Home.
    click_control(cx, window, "home-gh-acme/local");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).home), None);
    assert_eq!(active_path(cx, &app), local_path);

    // Not local: the card asks where. No folder is chosen for the user, and
    // Clone does nothing until one is.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    click_control(cx, window, "home-gh-acme/widgets");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .clone_modal()
            .is_some_and(|m| m.target.is_none())),
        "the card opens without a folder"
    );
    click_control(cx, window, "clone-confirm");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_some()));
    assert!(cx.read(|cx| app.read(cx).home_github.cloning.is_none()));

    // The chosen folder already holds a `widgets` with files: refused.
    let dest = clones.join("widgets");
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("mine.txt"), "mine").unwrap();
    choose_folder(cx, &app, &clones);
    cx.read(|cx| {
        let target = app
            .read(cx)
            .clone_modal()
            .and_then(|m| m.target.clone())
            .expect("the folder is planned");
        assert_eq!(target.request.dest, dest);
        assert_eq!(target.request.source, "github.com/acme/widgets");
        assert!(
            !target.plan.blockers.is_empty(),
            "an occupied folder is refused"
        );
        assert!(
            !target.plan.warnings.is_empty(),
            "a fork says gh adds upstream"
        );
    });
    click_control(cx, window, "clone-confirm");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).home_github.cloning.is_none()),
        "a refused clone does not start"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).clone_modal().is_none()));
    assert_eq!(
        std::fs::read_to_string(dest.join("mine.txt")).unwrap(),
        "mine"
    );

    // Free again: Clone clones, records, and opens the new repository.
    std::fs::remove_dir_all(&dest).unwrap();
    click_control(cx, window, "home-gh-acme/widgets");
    cx.run_until_parked();
    choose_folder(cx, &app, &clones);
    // Pressing Clone visibly does something at once: the card stays up as
    // the clone's progress (no silent gap that looks like a missed click).
    // Called directly — what the button's handler calls — because a
    // simulated click parks the executor, which runs the local clone to its
    // end before the running state can be read.
    app.update(cx, |app, cx| app.start_clone(cx));
    assert!(
        cx.read(|cx| {
            let app = app.read(cx);
            app.home_github.cloning.is_some()
                && app.clone_modal().is_some_and(|m| m.started.is_some())
        }),
        "the card shows the running clone"
    );
    wait_for(cx, &app, "the clone", |app| {
        app.home_github.cloning.is_none() && app.clone_modal().is_none()
    });
    cx.run_until_parked();
    assert_eq!(active_path(cx, &app), dest, "the clone opened as a tab");
    assert_eq!(cx.read(|cx| app.read(cx).home), None, "in place of Home");
    let receipts = read_oplog_tail_for_repo(&dest, 5);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0].op, "clone");
    assert!(
        matches!(receipts[0].outcome, OpOutcome::Success { .. }),
        "{:?}",
        receipts[0].outcome
    );

    unmount(cx, app, window);
}

/// The folder dialog's answer, delivered through the same method its
/// callback calls (the native dialog cannot be driven here).
fn choose_folder(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, parent: &Path) {
    app.update(cx, |app, cx| app.replan_clone(parent, cx));
    cx.run_until_parked();
}
