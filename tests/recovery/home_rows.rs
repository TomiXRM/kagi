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
    // Enough review requests to scroll the user's own PRs out of view. No
    // author, so no avatar is fetched (Tier A has no network).
    let reviews = (100..140)
        .map(|n| {
            format!(
                r#"{{"number":{n},"title":"Review {n}","url":"https://github.com/acme/r01/pull/{n}","isDraft":false,"author":{{"login":""}},"updatedAt":"2026-09-01T00:00:00Z","repository":{{"name":"r01","nameWithOwner":"acme/r01"}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo acme ;;\n\
         'api user/orgs '*) ;;\n\
         'repo list --limit') cat <<'JSON'\n[{repos}]\nJSON\n;;\n\
         'search prs --author=@me') cat <<'JSON'\n[{p2},{p1}]\nJSON\n;;\n\
         'search prs --review-requested=@me') cat <<'JSON'\n[{reviews}]\nJSON\n;;\n\
         'search '*) echo '[]' ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n",
        p1 = pr(1),
        p2 = pr(2),
    )
}

/// A stand-in `gh` listing three repositories, then organizations: thirty
/// that cannot be read, `org30` with three repositories, and thirty more that
/// cannot be read. Each unreadable one is a heading and a note, so each run
/// of them is more than a screen of entries with no row.
fn orgs_script() -> String {
    let repos = |owner: &str| {
        (0..3)
            .map(|i| {
                format!(
                    r#"{{"nameWithOwner":"{owner}/r{i:02}","url":"https://github.com/{owner}/r{i:02}","isFork":false,"isPrivate":false,"description":"","updatedAt":"2026-10-01T00:00:00Z"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    let orgs = (0..61)
        .map(|i| format!("org{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo acme ;;\n\
         'api user/orgs '*) cat <<'EOF'\n{orgs}\nEOF\n;;\n\
         'repo list --limit') cat <<'JSON'\n[{mine}]\nJSON\n;;\n\
         'repo list org30') cat <<'JSON'\n[{org30}]\nJSON\n;;\n\
         'search '*) echo '[]' ;;\n\
         *) echo \"gh: $*: HTTP 403\" >&2; exit 1 ;;\nesac\n",
        mine = repos("acme"),
        org30 = repos("org30"),
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
    // A point over the list, for the wheel.
    let over_list = kagi::ui::e2e::control_bounds(window.window_id(), "home-gh-acme/r00")
        .expect("the first row is drawn")
        .center();
    let wheel = |cx: &mut VisualTestAppContext, dy: f32| {
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: over_list,
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(dy))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        // The frame that scrolls, then the one that sees what left.
        for _ in 0..2 {
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
        }
        cx.run_until_parked();
    };

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

    // The ends stop the arrows (each end scrolled into view first: a row
    // the list does not draw does not keep the focus, below).
    wheel(cx, -5000.);
    focus_row(cx, &app, window, "repo:acme/r59");
    keys(cx, window, "down");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r59"));
    wheel(cx, 5000.);
    focus_row(cx, &app, window, "repo:acme/r00");
    keys(cx, window, "up");
    assert_eq!(row(cx, &app, window).as_deref(), Some("repo:acme/r00"));

    // The wheel scrolls the focused row out of the list's drawn range (#961
    // review): it is no longer drawn, so the focus moves to the list's stop,
    // the first row on screen, where ↑/↓ and the ring still work.
    assert!(drawn(cx, window, "home-gh-acme/r00"));
    wheel(cx, -3000.);
    assert!(
        !drawn(cx, window, "home-gh-acme/r00"),
        "precondition: the focused row is scrolled out of view"
    );
    let now = row(cx, &app, window).expect("a row keeps the focus");
    assert_ne!(now, "repo:acme/r00", "the focus left the row scrolled away");
    let shown = format!("home-gh-{}", now.trim_start_matches("repo:"));
    assert!(drawn(cx, window, &shown), "{now} is on screen");
    wheel(cx, 3000.);

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

    // The focus on a row's Open button, and the search drops that row (#961
    // review): the focus goes to the first row left, as from the row itself.
    keys(cx, window, "tab");
    let inside = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).home_row_holds_focus_for_e2e(window, cx)
        })
        .unwrap();
    assert!(
        inside && row(cx, &app, window).is_none(),
        "precondition: on #1's Open button"
    );
    set_filter(cx, &app, window, "PR 2");
    assert_eq!(
        row(cx, &app, window).as_deref(),
        Some("MyPrs:acme/r00#2"),
        "the focus goes to the first row left"
    );
    set_filter(cx, &app, window, "");
    focus_row(cx, &app, window, "MyPrs:acme/r00#1");

    // The focus on #1's Open button, and the wheel scrolls #1 out of the
    // drawn range (#961 review): the button is unmounted with its row, so
    // the focus moves to a row on screen, a review request.
    keys(cx, window, "tab");
    let inside = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).home_row_holds_focus_for_e2e(window, cx)
        })
        .unwrap();
    assert!(inside, "precondition: on #1's Open button");
    wheel(cx, -3000.);
    assert!(
        !drawn(cx, window, "home-work-acme/r00-1"),
        "precondition: #1 is scrolled out of view"
    );
    let now = row(cx, &app, window).expect("a drawn row has the focus");
    let number = now
        .strip_prefix("ReviewRequests:acme/r01#")
        .expect("a review request row");
    assert!(
        drawn(cx, window, &format!("home-work-acme/r01-{number}")),
        "{now} is on screen"
    );
    wheel(cx, 5000.);
    focus_row(cx, &app, window, "MyPrs:acme/r00#1");

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

    // The same with the focus on a row's Open button, a control with its own
    // focus handle inside the row (#961 review): Tab from the row reaches it.
    focus_row(cx, &app, window, "MyPrs:acme/r00#1");
    keys(cx, window, "tab");
    assert_eq!(row(cx, &app, window), None, "no longer the row itself");
    let inside = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).home_row_holds_focus_for_e2e(window, cx)
        })
        .unwrap();
    assert!(inside, "precondition: the focus is on a control in the row");
    app.update(cx, |app, cx| app.close_home_tab(cx));
    cx.run_until_parked();
    let root = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap();
    assert!(
        root,
        "closing Home hands the focus of a row's Open button to the window"
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    wait_for(cx, &app, "the lists again", settled);

    // And with the focus on a cell of Home's switch (#961 review).
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| app.focus_home_pane_for_e2e(0, window, cx))
    })
    .unwrap();
    assert_eq!(
        cx.update_window(window, |_, window, cx| app
            .read(cx)
            .home_pane_focused_for_e2e(window))
            .unwrap(),
        Some(0),
        "precondition: the switch's first cell"
    );
    app.update(cx, |app, cx| app.close_home_tab(cx));
    cx.run_until_parked();
    let root = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx)
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        })
        .unwrap();
    assert!(
        root,
        "closing Home hands the focus of its switch to the window"
    );
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    wait_for(cx, &app, "the lists again", settled);

    // Runs of headings and notes longer than a screen (organizations that
    // cannot be read), with a row focused and scrolled away (#961 review):
    // with no row on screen there is no Tab stop in the list and the focus
    // goes to the window — neither to a row below the screen (org30's,
    // after the first run) nor to row 0 (after the last run).
    {
        let _orgs = OfflineGh::with_script(&orgs_script());
        app.update(cx, |app, cx| app.set_home_pane(HomePane::Repos, cx));
        app.update(cx, |app, cx| app.reload_home_github(cx));
        wait_for(cx, &app, "the organizations' read", settled);
        let no_row_on_screen = |cx: &mut VisualTestAppContext| {
            for owner in ["acme", "org30"] {
                for repo in ["r00", "r01", "r02"] {
                    let name = format!("home-gh-{owner}/{repo}");
                    assert!(!drawn(cx, window, &name), "precondition: {name} is drawn");
                }
            }
        };
        let root_has_focus = |cx: &mut VisualTestAppContext| {
            cx.update_window(window, |_, window, cx| {
                window.draw(cx).clear();
                app.read(cx)
                    .root_focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window))
            })
            .unwrap()
        };
        for (scroll, place) in [(-700., "within the first run"), (-50000., "at the end")] {
            wheel(cx, 50000.);
            assert!(drawn(cx, window, "home-gh-acme/r02"));
            focus_row(cx, &app, window, "repo:acme/r02");
            wheel(cx, scroll);
            no_row_on_screen(cx);
            assert_eq!(
                row(cx, &app, window),
                None,
                "no row keeps the focus ({place})"
            );
            assert!(
                root_has_focus(cx),
                "with no row on screen ({place}), the window has the focus"
            );
        }
        wheel(cx, 50000.);
    }

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
