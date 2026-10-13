//! #1453: genuine empty states describe only their owner section and host.
use crate::home_github::{settled, wait_for};
use crate::macos::{build_fixture, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::VisualTestAppContext;
use kagi::ui::{
    home_github::GithubRepos,
    i18n::{self, Lang, Msg},
};
use kagi_git::github_repos::{OwnerRepos, RepoList, RepoListing};

struct HostGuard(Option<std::ffi::OsString>);
impl Drop for HostGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(host) => std::env::set_var("GH_HOST", host),
            None => std::env::remove_var("GH_HOST"),
        }
    }
}

fn section(owner: Option<&str>, count: usize, host: &str) -> OwnerRepos {
    OwnerRepos {
        owner: owner.map(str::to_owned),
        list: Ok(RepoList {
            truncated: false,
            repos: (0..count)
                .map(|index| RepoListing {
                    name_with_owner: format!("{}/repo-{index}", owner.unwrap_or("self")),
                    host: host.into(),
                    is_private: false,
                    is_fork: false,
                    description: String::new(),
                    updated_at: "2026-10-13T00:00:00Z".into(),
                })
                .collect(),
        }),
    }
}

pub fn scenario_home_empty_section_wording(cx: &mut VisualTestAppContext) {
    let _globals = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos", "lang"]);
    let _host = HostGuard(std::env::var_os("GH_HOST"));
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    wait_for(cx, &app, "initial Home read", settled);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    wait_for(cx, &app, "Home read", settled);
    for host in ["github.com", "git.example.enterprise"] {
        std::env::set_var("GH_HOST", host);
        for lang in [Lang::En, Lang::Ja] {
            i18n::set_lang(lang);
            for all_empty in [false, true] {
                let count = usize::from(!all_empty);
                app.update(cx, |app, cx| {
                    app.home_github.set_account_for_e2e(&format!("{host}/self"));
                    app.home_github.repos = GithubRepos::Loaded {
                        sections: vec![
                            section(None, count, host),
                            section(Some("orgA"), 0, host),
                            section(Some("orgB"), count, host),
                            OwnerRepos {
                                owner: Some("failed-org".into()),
                                list: Err("HTTP 403: SSO required".into()),
                            },
                        ],
                        orgs_error: None,
                    };
                    app.invalidate_home_items_for_e2e();
                    cx.notify();
                });
                cx.update_window(window, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })
                .unwrap();
                let items = cx.read(|cx| app.read(cx).home_item_text_for_e2e());
                let mut owner = String::new();
                let mut empty_owners = Vec::new();
                for (kind, text) in &items {
                    if kind == "heading" {
                        owner = text.clone();
                    }
                    if kind != "note" {
                        continue;
                    }
                    assert!(
                        !text.contains("You have no repositories on GitHub")
                            && !text.contains("GitHub にリポジトリがありません"),
                        "global assertion: {items:?}"
                    );
                    if owner == "failed-org" {
                        assert!(text.contains("HTTP 403: SSO required"));
                        assert!(
                            !text.contains("No repositories")
                                && !text.contains("表示できるリポジトリ")
                        );
                        continue;
                    }
                    let expected = match (owner == Msg::HomeGithubMine.t(), lang) {
                        (true, Lang::En) => {
                            format!("You have no repositories to display on {host}.")
                        }
                        (true, Lang::Ja) => {
                            format!("{host} に表示できるあなたのリポジトリはありません。")
                        }
                        (false, Lang::En) => {
                            format!("No repositories to display for {owner} on {host}.")
                        }
                        (false, Lang::Ja) => {
                            format!("{host} の {owner} には表示できるリポジトリがありません。")
                        }
                    };
                    assert_eq!(text, &expected, "owner-scoped production item");
                    empty_owners.push(owner.clone());
                }
                let expected = if all_empty {
                    vec![
                        Msg::HomeGithubMine.t().to_string(),
                        "orgA".into(),
                        "orgB".into(),
                    ]
                } else {
                    vec!["orgA".into()]
                };
                assert_eq!(empty_owners, expected);
                assert_eq!(
                    items.iter().filter(|(kind, _)| kind == "repo").count(),
                    count * 2
                );
                assert_eq!(
                    items
                        .iter()
                        .filter(|(kind, text)| kind == "note"
                            && text.contains("HTTP 403: SSO required"))
                        .count(),
                    1
                );
            }
        }
    }
    unmount(cx, app, window);
}
