//! ↑/↓ between the rows of Home's virtualized list (#959): the list is one
//! Tab stop, the arrows scroll the next row into view and focus it, stop at
//! the ends, and a focused row that leaves the list hands the focus on.
use gpui::{AnyWindowHandle, Entity, Focusable as _, VisualTestAppContext};
use kagi::ui::home_work::HomePane;
use kagi::ui::KagiApp;

use crate::home_github::{drawn, settled, wait_for, EnvCleared};
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

/// A stand-in `gh` listing 60 repositories (enough to scroll) and two of
/// the user's pull requests.
fn gh_script() -> String {
    let repos = (0..60)
        .map(|i| {
            format!(
                r#"{{"nameWithOwner":"acme/r{i:02}","url":"https://github.com/acme/r{i:02}","isFork":false,"isPrivate":false,"description":"","updatedAt":"2026-10-01T00:00:00Z"}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let pr = |n: u32| {
        format!(
            r#"{{"number":{n},"title":"PR {n}","url":"https://github.com/acme/r00/pull/{n}","isDraft":false,"author":{{"login":"acme"}},"updatedAt":"2026-10-0{n}T00:00:00Z","repository":{{"name":"r00","nameWithOwner":"acme/r00"}}}}"#
        )
    };
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo acme ;;\n\
         'api user/orgs '*) ;;\n\
         'repo list --limit') cat <<'JSON'\n[{repos}]\nJSON\n;;\n\
         'search prs --author=@me') cat <<'JSON'\n[{p2},{p1}]\nJSON\n;;\n\
         'search '*) echo '[]' ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n",
        p1 = pr(1),
        p2 = pr(2),
    )
}

fn row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> Option<String> {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx).home_row_focused_for_e2e(window)
    })
    .unwrap()
}

fn focus_row(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    key: &str,
) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| app.focus_home_row_for_e2e(key, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

fn set_filter(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        let input = app
            .read(cx)
            .home_github
            .filter
            .clone()
            .expect("Home's search");
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

pub fn scenario_home_rows(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let start = build_fixture();
    let _gh = OfflineGh::with_script(&gh_script());
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    let dir = kagi::ui::settings::settings_path()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .expect("the runner sets KAGI_LOG_DIR");
    let _ = std::fs::remove_file(kagi_git::github_repos_cache::cache_path(&dir));
    let _ = std::fs::remove_file(kagi_git::github_repos_cache::work_cache_path(&dir));

    let (app, window) = mount(cx, start.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    wait_for(cx, &app, "the lists", settled);
    assert!(drawn(cx, window, "home-gh-acme/r00"));

    // Tab, from the switch, reaches the list at its first row on screen:
    // the list is one Tab stop, not one per row.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.focus_home_pane_for_e2e(0, window, cx))
    })
    .unwrap();
    let mut reached = None;
    for _ in 0..8 {
        keys(cx, window, "tab");
        reached = row(cx, &app, window);
        if reached.is_some() {
            break;
        }
    }
    assert_eq!(reached.as_deref(), Some("repo:acme/r00"));
    keys(cx, window, "tab");
    assert_eq!(row(cx, &app, window), None, "one Tab leaves the list");

    // ↓ moves row by row, scrolling the next one into view.
    focus_row(cx, &app, window, "repo:acme/r00");
    assert!(
        !drawn(cx, window, "home-gh-acme/r40"),
        "r40 starts out of view"
    );
    for _ in 0..40 {
        keys(cx, window, "down");
    }
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r40"));
    assert!(
        drawn(cx, window, "home-gh-acme/r40"),
        "and is scrolled into view"
    );
    keys(cx, window, "up");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r39"));
    keys(cx, window, "down");

    // The list remembers its row: out with Tab, back with Shift+Tab.
    keys(cx, window, "tab");
    assert_eq!(row(cx, &app, window), None);
    keys(cx, window, "shift-tab");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r40"));

    // The ends stop the arrows.
    focus_row(cx, &app, window, "repo:acme/r59");
    keys(cx, window, "down");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r59"));
    focus_row(cx, &app, window, "repo:acme/r00");
    keys(cx, window, "up");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r00"));

    // A focused row that leaves the list hands the focus on: to the first
    // row left, or to the window when none is.
    focus_row(cx, &app, window, "repo:acme/r40");
    set_filter(cx, &app, window, "r0");
    assert_eq!(
        row(cx, &app, window).as_deref(),
        Some("repo:acme/r00"),
        "the focus goes to the first row left"
    );
    set_filter(cx, &app, window, "zzz");
    let root = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap();
    assert!(root, "with no row left, the window has the focus");
    set_filter(cx, &app, window, "");

    // ↑/↓ typed into the search field stay the field's.
    focus_row(cx, &app, window, "repo:acme/r00");
    cx.update_window(window, |_, window, cx| {
        let input = app.read(cx).home_github.filter.clone().unwrap();
        input.update(cx, |input, cx| input.focus(window, cx));
    })
    .unwrap();
    keys(cx, window, "down down");
    assert_eq!(row(cx, &app, window), None, "no row takes the focus");
    let field = cx
        .update_window(window, |_, window, cx| {
            let input = app.read(cx).home_github.filter.clone().unwrap();
            input.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap();
    assert!(field, "the search field keeps it");

    // Pull request rows step the same way.
    app.update(cx, |app, cx| app.set_home_pane(HomePane::Prs, cx));
    cx.run_until_parked();
    focus_row(cx, &app, window, "MyPrs:acme/r00#2");
    keys(cx, window, "down");
    assert_eq!(row(cx, &app, window).as_deref(), Some("MyPrs:acme/r00#1"));

    // ⌘W closes Home with a row focused (#961 review): Home is no longer
    // drawn, so the row hands the focus to the window rather than keep it on
    // a handle nothing tracks.
    app.update(cx, |app, cx| app.close_home_tab(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).home.is_none()));
    assert_eq!(row(cx, &app, window), None);
    let root = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap();
    assert!(root, "closing Home hands its row's focus to the window");
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    wait_for(cx, &app, "the lists again", settled);

    // Another account's read replaces the list on screen with Loading, and
    // the read fails (#961 review): no row is drawn any more, so a focused
    // row hands the focus to the window rather than keep it out of sight.
    app.update(cx, |app, cx| app.set_home_pane(HomePane::Repos, cx));
    cx.run_until_parked();
    focus_row(cx, &app, window, "repo:acme/r00");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r00"));
    let _other = OfflineGh::with_script(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo someone-else ;;\n\
         'api user/orgs '*) ;;\n\
         'search '*) echo '[]' ;;\n\
         *) echo 'gh: offline' >&2; exit 1 ;;\nesac\n",
    );
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the failed read", |app| {
        matches!(
            app.home_github.repos,
            kagi::ui::home_github::GithubRepos::Failed(_)
        )
    });
    assert_eq!(row(cx, &app, window), None);
    let root = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap();
    assert!(root, "the list's focused row hands the focus to the window");

    unmount(cx, app, window);
}
