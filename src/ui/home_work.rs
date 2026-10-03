//! Home's pull requests and issues across every repository (#928, ADR-0219
//! decision 8): the PRs the user opened, the PRs waiting for their review
//! and the issues assigned to them, read with `gh search` and shown behind
//! Home's Repositories / Pull requests / Issues switch.
//!
//! Read with the repository list (Refresh reads both), saved and shown again
//! the way it is: per `gh` account, owner-only on disk, through
//! `github_repos_cache`. A row opens its repository's local clone as a tab
//! and shows the PR or issue there; without a clone it offers the clone card.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, SharedString, Window};
use kagi_git::github_repos_cache::{WorkItem, WorkKind, WorkList, WorkLists};

use super::home_github::{home_dir, GithubRepos};
use super::i18n::Msg;
use super::theme::{self, theme};
use super::KagiApp;

/// What Home's switch shows (#928).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HomePane {
    #[default]
    Repos,
    Prs,
    Issues,
}

/// Home's pull request / issue lists, window-wide like Home itself.
#[derive(Default)]
pub struct HomeWork {
    pub pane: HomePane,
    /// On screen: the saved lists or the last read. `None` until either
    /// lands — the switch then shows a spinner, never a count of 0.
    pub lists: Option<WorkLists>,
    /// Lists whose last read failed, and why. A list read before stays on
    /// screen below its error.
    pub errors: HashMap<WorkKind, String>,
    /// A read is running; Refresh joins it.
    pub reading: bool,
    pub(super) generation: u64,
    /// The `gh` account the lists on screen were read as.
    shown_account: Option<String>,
    /// The PR being prepared for opening (`gh pr view`), as `(repository
    /// identity, number)`: its row shows a spinner and ignores clicks.
    pub opening: Option<(String, u64)>,
    /// Bumped whenever what the panes show changes (lists, errors, the PR
    /// being opened), so the virtualized list knows to lay out again.
    pub(super) version: u64,
}

/// Where the last read is saved: next to `settings.json`.
fn work_cache_file() -> Option<PathBuf> {
    super::settings::settings_path().and_then(|p| {
        p.parent()
            .map(kagi_git::github_repos_cache::work_cache_path)
    })
}

impl KagiApp {
    /// Read the three lists again, unless a read is running: the account
    /// and the list saved for it first (local reads; shown when nothing is
    /// on screen for that account), then the three `gh search`es side by
    /// side. A list on screen read as another account is dropped, as the
    /// repository list's is.
    pub(super) fn reload_home_work(&mut self, cx: &mut Context<Self>) {
        let work = &mut self.home_github.work;
        if work.reading {
            return;
        }
        work.generation += 1;
        work.reading = true;
        let generation = work.generation;
        let started = std::time::Instant::now();
        let cache = work_cache_file();
        let saved_cache = cache.clone();
        let cached = cx.background_spawn(async move {
            let account = kagi_git::github_repos::active_account(&home_dir());
            let saved = match (&account, &cache) {
                (Some(account), Some(path)) => {
                    kagi_git::github_repos_cache::load_work(path, account)
                }
                _ => None,
            };
            (account, saved)
        });
        let reads = WorkKind::ALL.map(|kind| {
            cx.background_spawn(
                async move { kagi_git::github_search::search_work(&home_dir(), kind) },
            )
        });
        cx.spawn(async move |app, acx| {
            let (account, saved) = cached.await;
            let _ = app.update(acx, |app, cx| {
                app.show_saved_work(generation, account.clone(), saved, cx)
            });
            let mut results = Vec::with_capacity(reads.len());
            for read in reads {
                results.push(read.await.map_err(|error| error.to_string()));
            }
            let save = app
                .update(acx, |app, cx| {
                    app.land_work(generation, results, started, cx)
                })
                .ok()
                .flatten();
            if let (Some(lists), Some(path), Some(account)) = (save, saved_cache, account) {
                let saved = acx
                    .background_spawn(async move {
                        kagi_git::github_repos_cache::save_work(&path, &account, &lists)
                    })
                    .await;
                if let Err(error) = saved {
                    eprintln!("home: could not save the GitHub work cache: {error}");
                }
            }
        })
        .detach();
    }

    fn show_saved_work(
        &mut self,
        generation: u64,
        account: Option<String>,
        saved: Option<WorkLists>,
        cx: &mut Context<Self>,
    ) {
        let work = &mut self.home_github.work;
        if work.generation != generation {
            return;
        }
        if work.shown_account != account {
            work.lists = None;
            work.errors.clear();
        }
        work.shown_account = account;
        if work.lists.is_none() {
            work.lists = saved;
        }
        work.version += 1;
        cx.notify();
    }

    /// The three reads have landed. A list that read replaces the one on
    /// screen; one that failed keeps it and says why. Returns what to save:
    /// only three complete lists.
    fn land_work(
        &mut self,
        generation: u64,
        results: Vec<Result<WorkList, String>>,
        started: std::time::Instant,
        cx: &mut Context<Self>,
    ) -> Option<WorkLists> {
        let work = &mut self.home_github.work;
        if work.generation != generation {
            return None;
        }
        work.reading = false;
        work.errors.clear();
        let mut lists = work.lists.take().unwrap_or_default();
        for (kind, result) in WorkKind::ALL.into_iter().zip(results) {
            match result {
                Ok(list) => *lists.get_mut(kind) = list,
                Err(error) => {
                    klog!("home: work {kind:?} failed: {error}");
                    work.errors.insert(kind, error);
                }
            }
        }
        klog!(
            "home: work prs={} review={} issues={} failed={} ms={}",
            lists.my_prs.items.len(),
            lists.review_requests.items.len(),
            lists.assigned_issues.items.len(),
            work.errors.len(),
            started.elapsed().as_millis()
        );
        let save = work.errors.is_empty().then(|| lists.clone());
        work.lists = Some(lists);
        work.version += 1;
        self.ensure_home_avatars(cx);
        cx.notify();
        save
    }

    /// A PR or issue row was clicked. With a local clone of its repository
    /// it opens as a tab showing that PR (once `gh pr view` has its refs) or
    /// issue (once the clone is known to address that repository); without
    /// one it opens on GitHub (#940 review) — the clone card belongs to the
    /// Repositories tab. A click on a row still being prepared is ignored; a
    /// click on any other row wins over it.
    pub fn home_work_pick(
        &mut self,
        kind: WorkKind,
        item: WorkItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (item.identity(), item.number);
        if self.home_github.work.opening.as_ref() == Some(&key) {
            return;
        }
        let local = self
            .home_github
            .local
            .get(&key.0)
            .cloned()
            .filter(|path| path.is_dir());
        let Some(path) = local else {
            // The later pick wins: an open still waiting for `gh` would
            // otherwise move the user to its repository after this row
            // opened on GitHub (#940 review).
            if self.home_github.work.opening.take().is_some() {
                self.home_github.work.version += 1;
                cx.notify();
            }
            self.open_work_item_url(&item.url, cx);
            return;
        };
        self.home_github.work.opening = Some(key.clone());
        self.home_github.work.version += 1;
        cx.notify();
        if !kind.is_pr() {
            self.open_issue_when_addressed(key, path, window, cx);
            return;
        }
        klog!("home: open pr {}#{}", key.0, key.1);
        let (base_repo, number) = key.clone();
        let read = cx.background_spawn(async move {
            kagi_git::github::pr_for_open(&home_dir(), &base_repo, number)
        });
        cx.spawn(async move |app, acx| {
            let pr = read.await;
            let _ = app.update(acx, |app, cx| app.finish_opening_pr(key, path, pr, cx));
        })
        .detach();
    }

    /// Open a PR / issue on GitHub at the URL its search returned, through
    /// the OS opener (#940 review). The GUI runner records the URL instead,
    /// so a scenario never launches the user's browser.
    pub(super) fn open_work_item_url(&mut self, url: &str, cx: &mut Context<Self>) {
        klog!("home: open on github {}", url);
        #[cfg(feature = "gui-e2e")]
        {
            let _ = cx;
            super::e2e::record_opened_url(url);
        }
        #[cfg(not(feature = "gui-e2e"))]
        cx.open_url(url);
    }

    /// The PR's refs have arrived. Opened only while it is still the PR
    /// being prepared and Home is still in front — a user who moved to
    /// another tab meanwhile is not pulled away from it.
    fn finish_opening_pr(
        &mut self,
        key: (String, u64),
        path: PathBuf,
        pr: Result<kagi_domain::github::PullRequest, kagi_git::github::PrFetchError>,
        cx: &mut Context<Self>,
    ) {
        if self.home_github.work.opening.as_ref() != Some(&key) {
            return;
        }
        self.home_github.work.opening = None;
        self.home_github.work.version += 1;
        cx.notify();
        match pr {
            Ok(pr) if self.home_in_front() => {
                if self.open_repository(path, cx) {
                    self.show_pr_mode(cx);
                    self.pr_mode_open(&pr, cx);
                }
            }
            Ok(_) => klog!("home: open pr {}#{} dropped: Home left", key.0, key.1),
            Err(error) => {
                klog!("home: open pr {}#{} failed: {error}", key.0, key.1);
                let message = Msg::HomeWorkOpenFailed
                    .t()
                    .replacen("{}", &key.1.to_string(), 1)
                    .replacen("{}", &error.to_string(), 1);
                self.push_toast(super::ToastKind::Error, message, cx);
            }
        }
    }

    /// An issue opens in its clone's Issues mode, which reads and writes the
    /// repository `gh` resolves for the clone (`gh repo set-default`, else
    /// `origin`). Only when that is the issue's own repository: otherwise the
    /// same number there is another issue, and a reply would go to it.
    fn open_issue_when_addressed(
        &mut self,
        key: (String, u64),
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        klog!("home: open issue {}#{}", key.0, key.1);
        let dir = path.clone();
        let read =
            cx.background_spawn(async move { kagi_git::github_fetch::repository_identity(&dir) });
        cx.spawn_in(window, async move |app, acx| {
            let addressed = read.await;
            let _ = app.update_in(acx, |app, window, cx| {
                if app.home_github.work.opening.as_ref() != Some(&key) {
                    return;
                }
                app.home_github.work.opening = None;
                app.home_github.work.version += 1;
                cx.notify();
                let error = match addressed {
                    Ok(id) if id.eq_ignore_ascii_case(&key.0) => {
                        if !app.home_in_front() {
                            klog!("home: open issue {}#{} dropped: Home left", key.0, key.1);
                        } else if app.open_repository(path, cx) {
                            app.show_issues_mode(cx);
                            app.load_github_issue_detail(key.1, window, cx);
                        }
                        return;
                    }
                    Ok(id) => Msg::HomeWorkOtherRepo
                        .t()
                        .replacen("{}", &key.1.to_string(), 1)
                        .replacen("{}", &key.0, 1)
                        .replacen("{}", &id, 1),
                    Err(error) => Msg::HomeWorkOpenFailed
                        .t()
                        .replacen("{}", &key.1.to_string(), 1)
                        .replacen("{}", &error.to_string(), 1),
                };
                klog!("home: open issue {}#{} refused: {error}", key.0, key.1);
                app.push_toast(super::ToastKind::Error, error, cx);
            });
        })
        .detach();
    }

    /// Home's Repositories / Pull requests / Issues switch, with counts. A
    /// count not known yet is a spinner, never 0.
    pub(super) fn render_home_panes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let work = &self.home_github.work;
        let repos = match &self.home_github.repos {
            GithubRepos::Loaded { sections, .. } => Some(
                sections
                    .iter()
                    .filter_map(|s| s.list.as_ref().ok())
                    .map(|l| l.repos.len())
                    .sum(),
            ),
            _ => None,
        };
        let repos_loading = matches!(
            self.home_github.repos,
            GithubRepos::NotLoaded | GithubRepos::Loading
        );
        let count = |kinds: &[WorkKind]| {
            work.lists
                .as_ref()
                .map(|lists| kinds.iter().map(|&k| lists.get(k).items.len()).sum())
        };
        let cells = [
            (
                HomePane::Repos,
                Msg::HomePaneRepos.t(),
                repos,
                repos_loading,
            ),
            (
                HomePane::Prs,
                Msg::HomePanePrs.t(),
                count(&[WorkKind::MyPrs, WorkKind::ReviewRequests]),
                work.reading,
            ),
            (
                HomePane::Issues,
                Msg::HomePaneIssues.t(),
                count(&[WorkKind::AssignedIssues]),
                work.reading,
            ),
        ];
        let active = work.pane;
        let mut row = div()
            .id("home-panes")
            .role(gpui::Role::TabList)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p(theme::scaled_px(3.))
            .rounded_lg()
            .bg(rgb(theme().panel));
        for (pane, label, count, loading) in cells {
            row = row.child(pane_cell(pane, label, count, loading, pane == active, cx));
        }
        row.into_any_element()
    }
}

/// One cell of the switch: label and count (or a spinner while the count is
/// not known), the selected one raised like the workspace-mode switch.
fn pane_cell(
    pane: HomePane,
    label: &'static str,
    count: Option<usize>,
    loading: bool,
    active: bool,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let id = match pane {
        HomePane::Repos => "home-pane-repos",
        HomePane::Prs => "home-pane-prs",
        HomePane::Issues => "home-pane-issues",
    };
    let name = match count {
        Some(count) => format!("{label} {count}"),
        None => label.to_string(),
    };
    let fg = if active {
        theme().accent_text_on(theme().surface)
    } else {
        theme().text_muted
    };
    let badge = match count {
        Some(count) => Some(
            div()
                .text_xs()
                .px(theme::scaled_px(6.))
                .rounded_md()
                .bg(rgb(if active {
                    theme().selected
                } else {
                    theme().surface
                }))
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(count.to_string()))
                .into_any_element(),
        ),
        None if loading => Some(super::e2e::measure_control(
            format!("{id}-spinner"),
            super::render_overlay::sync_spinner(
                12.,
                theme().text_muted,
                SharedString::from(format!("{id}-spinner")),
            ),
        )),
        None => None,
    };
    let cell = div()
        .id(id)
        .role(gpui::Role::Tab)
        .aria_label(SharedString::from(name))
        .aria_selected(active)
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py(theme::scaled_px(5.))
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .text_color(rgb(fg))
        .when(active, |el| {
            el.bg(rgb(theme().surface))
                .font_weight(gpui::FontWeight::MEDIUM)
        })
        .when(!active, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
            app.home_github.work.pane = pane;
            cx.notify();
        }))
        .child(SharedString::from(label))
        .children(badge)
        .min_w(px(0.));
    super::e2e::measure_control(id, cell)
}
