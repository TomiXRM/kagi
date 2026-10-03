//! Home's list keeps its place (#942 review): the entries are rebuilt for
//! what the pane on screen shows and for the language they are written in,
//! not for a read landing for another pane; and a list read again keeps
//! where it was scrolled to.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::i18n::Lang;
use kagi::ui::{e2e, settings, KagiApp};
use kagi_git::github_repos_cache;

use crate::home_github::{drawn, settled, wait_for, EnvCleared};
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

/// A stand-in `gh` listing 60 repositories (enough to scroll) and no pull
/// requests or issues.
fn gh_script() -> String {
    let repos = (0..60)
        .map(|i| {
            format!(
                r#"{{"nameWithOwner":"acme/r{i:02}","url":"https://github.com/acme/r{i:02}","isFork":false,"isPrivate":false,"description":"","updatedAt":"2026-10-01T00:00:00Z"}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "#!/bin/sh\ncase \"$1 $2 $3\" in\n\
         'config get user') echo acme ;;\n\
         'api user/orgs '*) ;;\n\
         'repo list --limit') cat <<'JSON'\n[{repos}]\nJSON\n;;\n\
         'search '*) echo '[]' ;;\n\
         *) echo \"unexpected gh $*\" >&2; exit 1 ;;\nesac\n"
    )
}

fn top(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) -> usize {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| app.read(cx).home_list_top_for_e2e())
        .expect("Home's list")
}

pub fn scenario_home_list_place(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang"]);
    let start = build_fixture();
    let _gh = OfflineGh::with_script(&gh_script());
    let mut cleared = kagi_git::github_repos::TOKEN_OVERRIDES.to_vec();
    cleared.push("GH_HOST");
    let _env = EnvCleared::new(&cleared);
    let dir = settings::settings_path()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .expect("the runner sets KAGI_LOG_DIR");
    let _ = std::fs::remove_file(github_repos_cache::cache_path(&dir));
    let _ = std::fs::remove_file(github_repos_cache::work_cache_path(&dir));

    let (app, window) = mount(cx, start.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    wait_for(cx, &app, "the lists", settled);
    assert!(drawn(cx, window, "home-gh-acme/r00"));
    app.update(cx, |app, _| app.scroll_home_list_for_e2e(30));
    assert_eq!(top(cx, &app, window), 30, "the list scrolls");

    // The pull request / issue lists landing while Repositories is on
    // screen neither rebuild it nor move it.
    let builds = e2e::home_item_builds();
    app.update(cx, |app, cx| app.reload_home_work_for_e2e(cx));
    wait_for(cx, &app, "the pull request / issue lists", settled);
    assert_eq!(
        top(cx, &app, window),
        30,
        "a hidden pane's read keeps the place"
    );
    assert_eq!(
        e2e::home_item_builds(),
        builds,
        "a hidden pane's read does not rebuild the list on screen"
    );

    // Another language: the entries are written again, in place.
    app.update(cx, |app, cx| app.set_lang(Lang::Ja, cx));
    assert_eq!(top(cx, &app, window), 30, "a new language keeps the place");
    assert!(
        e2e::home_item_builds() > builds,
        "a new language writes the entries again"
    );
    app.update(cx, |app, cx| app.set_lang(Lang::En, cx));

    // A Refresh reads the same list again: it lands where the user is.
    app.update(cx, |app, cx| app.reload_home_github(cx));
    wait_for(cx, &app, "the refresh", settled);
    assert_eq!(top(cx, &app, window), 30, "a refresh keeps the place");

    unmount(cx, app, window);
}
