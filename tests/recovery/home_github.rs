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

fn gh_script(bare: &Path) -> String {
    format!(
        "#!/bin/sh\ncase \"$1 $2\" in\n\
         'repo list') cat <<'JSON'\n{REPO_LIST}\nJSON\n;;\n\
         'repo clone') git clone -q '{bare}' \"$4\" && \
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
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos", "clone_parent_dir"]);
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
    settings::write_setting("clone_parent_dir", Some(clones.to_str().unwrap()));
    tabs::record_recent_repo(&local_path);
    let _gh = OfflineGh::with_script(&gh_script(&bare));

    let (app, window) = mount(cx, start.path());
    click_control(cx, window, "tab-add");
    wait_for(cx, &app, "the GitHub list", |app| {
        matches!(app.home_github.repos, GithubRepos::Loaded { .. })
    });
    cx.read(|cx| {
        let GithubRepos::Loaded { list, local } = &app.read(cx).home_github.repos else {
            unreachable!()
        };
        assert_eq!(list.repos.len(), 2);
        assert!(!list.truncated);
        assert_eq!(
            local.get("github.com/acme/local"),
            Some(&local_path),
            "a recent repository whose origin is listed is known as its clone"
        );
    });

    // Known locally: the row opens it, in place of Home.
    click_control(cx, window, "home-gh-acme/local");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).home), None);
    assert_eq!(active_path(cx, &app), local_path);

    // Not local, and its default folder is in use: the card refuses.
    let dest = clones.join("widgets");
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("mine.txt"), "mine").unwrap();
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    click_control(cx, window, "home-gh-acme/widgets");
    cx.run_until_parked();
    cx.read(|cx| {
        let card = app
            .read(cx)
            .clone_modal()
            .expect("the row opens the clone card");
        assert_eq!(card.request.dest, dest);
        assert_eq!(card.request.source, "github.com/acme/widgets");
        assert!(
            !card.plan.blockers.is_empty(),
            "an occupied folder is refused"
        );
        assert!(
            !card.plan.warnings.is_empty(),
            "a fork says gh adds upstream"
        );
    });
    assert!(
        !drawn(cx, window, "clone-confirm"),
        "a refused clone has no Clone button"
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
    assert!(drawn(cx, window, "clone-confirm"));
    click_control(cx, window, "clone-confirm");
    wait_for(cx, &app, "the clone", |app| {
        app.home_github.cloning.is_none()
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
    assert_eq!(
        settings::read_setting("clone_parent_dir").as_deref(),
        Some(clones.to_str().unwrap()),
        "the folder is the next default"
    );

    unmount(cx, app, window);
}
