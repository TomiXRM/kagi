//! Home's GitHub section (#923 PR3, ADR-0219 decisions 2, 3 and 7): the
//! signed-in user's repositories from `gh repo list`, each one opened when a
//! local clone is known and cloned through the recorded clone write
//! otherwise.
//!
//! The list is read once per window (Refresh reads it again) on a background
//! thread, together with the `origin` of every recently opened repository so
//! a listed repository can be matched with its local clone.
//!
//! A clone goes plan → card (confirm) → `execute_clone` (preflight, `gh repo
//! clone`, verify, oplog) on a background thread, one at a time per window.
//! On success the new folder opens as a tab — Home itself becomes that tab
//! when it is in front.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, Entity, PathPromptOptions, SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{Disableable as _, Sizable as _};
use kagi_git::github_repos::{OwnerRepos, RepoListing};
use kagi_git::ops::{plan_clone, CloneRequest};
use kagi_git::OperationPlan;

use super::home_github_list::{muted, muted_inline, search_field};
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::theme;
use super::KagiApp;

/// The GitHub list as Home knows it.
#[derive(Clone, Debug, Default)]
pub enum GithubRepos {
    #[default]
    NotLoaded,
    Loading,
    Loaded {
        /// The user's own section first, then one per organization.
        sections: Vec<OwnerRepos>,
        /// The organizations could not be listed (`gh api user/orgs`
        /// failed): said in the list rather than shown as none.
        orgs_error: Option<String>,
    },
    Failed(String),
}

/// Home's GitHub state, window-wide like Home itself.
#[derive(Default)]
pub struct HomeGithub {
    pub repos: GithubRepos,
    /// Listed identity (`host/owner/repo`) → a local clone of it: the
    /// recently opened repositories and the open tabs whose `origin` names
    /// one. Kept apart from the list so either can land first.
    pub local: HashMap<String, PathBuf>,
    /// The paths already matched into [`Self::local`], so an open tab is
    /// read once, not on every frame.
    local_checked: HashSet<PathBuf>,
    pub filter: Option<Entity<InputState>>,
    /// The clone running now (one per window, ADR-0219 decision 7), as its
    /// card: the card is its progress, and it can be sent to the background
    /// and brought back.
    pub cloning: Option<CloneModal>,
    /// Bumped per read, so an older read landing late is dropped.
    pub(super) generation: u64,
    /// The user's own list is on screen and the organizations' are still
    /// being read.
    pub orgs_loading: bool,
    /// The list on screen is the saved one (or the previous read) and a
    /// fresh read is running.
    pub refreshing: bool,
    /// The `gh` account (`<host>/<login>`) the list on screen was read as.
    /// A list read as another account is not kept across a refresh.
    shown_account: Option<String>,
    /// The virtualized list of rows, and what it was last built for: the
    /// rows are measured once per build, so a change resets it.
    pub(super) list: Option<gpui::ListState>,
    pub(super) list_key: Option<super::home_github_list::ListKey>,
    /// The list's entries as last built for [`Self::list_key`] (#937):
    /// rebuilt only when the key changes, not on every frame.
    pub(super) items: std::rc::Rc<[super::home_github_list::HomeItem]>,
    /// Each entry's place among the rows (headings and notes are not rows)
    /// and the number of rows, for assistive technology (#944). Built with
    /// [`Self::items`], not on every frame.
    pub(super) places: std::rc::Rc<[Option<usize>]>,
    pub(super) row_count: usize,
    /// Bumped whenever the listed repositories or the local clones change,
    /// so the entries built from them are built again (#937).
    pub(super) data_version: u64,
    /// The user's open pull requests and issues, and which pane of Home's
    /// switch is showing (#928).
    pub work: super::home_work::HomeWork,
}

/// The clone card: which repository, and — once the user has chosen a folder —
/// the clone planned for it. Kagi never picks the folder itself (user ruling,
/// ADR-0219 decision 7): the card opens with none, and Clone stays off until
/// one is chosen.
#[derive(Clone, Debug)]
pub struct CloneModal {
    pub listing: RepoListing,
    pub target: Option<CloneTarget>,
    /// Set once Clone is pressed: the card stays up showing the clone in
    /// progress (and for how long) until it ends or is sent to the background.
    pub started: Option<std::time::Instant>,
}

/// The clone planned for the chosen folder.
#[derive(Clone, Debug)]
pub struct CloneTarget {
    pub request: CloneRequest,
    pub plan: OperationPlan,
}

impl CloneTarget {
    /// Clone `listing` into a new folder named after it inside `parent`.
    fn new(listing: &RepoListing, parent: &Path) -> Self {
        let request = CloneRequest {
            source: listing.clone_source(),
            dest: parent.join(listing.name()),
            is_fork: listing.is_fork,
        };
        let plan = plan_clone(&request);
        Self { request, plan }
    }
}

/// The working directory `gh` runs in: the user's home (`USERPROFILE` on
/// Windows, which sets no `HOME`), else the temporary directory.
pub(super) fn home_dir() -> PathBuf {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Where the last read is saved: next to `settings.json`.
fn cache_file() -> Option<PathBuf> {
    super::settings::settings_path()
        .and_then(|p| p.parent().map(kagi_git::github_repos_cache::cache_path))
}

impl KagiApp {
    /// Read the list unless it is loaded or loading.
    pub(crate) fn ensure_home_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.home_github.filter.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder(Msg::HomeGithubFilter.t()));
            cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            self.home_github.filter = Some(input);
        }
        if matches!(self.home_github.repos, GithubRepos::NotLoaded) {
            self.reload_home_github(cx);
        }
    }

    /// A read is running. Refresh joins it rather than starting another: a
    /// superseded read is only ignored, its `gh` processes still run.
    pub fn home_github_reading(&self) -> bool {
        matches!(self.home_github.repos, GithubRepos::Loading)
            || self.home_github.refreshing
            || self.home_github.orgs_loading
            || self.home_github.work.reading
    }

    /// Read the lists again, unless a read is already running. Everything
    /// is read at once, off the UI thread: the saved list, the user's own,
    /// the organizations' and which listed repositories are cloned here.
    /// The saved list (or the one on screen) shows while the fresh read
    /// runs; with none, the user's own list shows as soon as it lands and
    /// the organizations' are appended.
    pub fn reload_home_github(&mut self, cx: &mut Context<Self>) {
        if self.home_github_reading() {
            return;
        }
        self.home_github.generation += 1;
        let generation = self.home_github.generation;
        // The pull request / issue lists are read with it (#928).
        self.reload_home_work(cx);
        let started = std::time::Instant::now();
        let shown = matches!(self.home_github.repos, GithubRepos::Loaded { .. });
        if shown {
            self.home_github.refreshing = true;
        } else {
            self.home_github.repos = GithubRepos::Loading;
            self.home_github.data_version += 1;
        }
        let mut paths = super::tabs::recent_repos();
        paths.extend(self.open_tab_paths());
        self.home_github.local_checked.clear();
        self.match_local_clones(paths, cx);
        let cache = cache_file();
        let saved_cache = cache.clone();
        // The account first (a local read), and the saved list only for it.
        let cached = cx.background_spawn(async move {
            let account = kagi_git::github_repos::active_account(&home_dir());
            let saved = match (&account, &cache) {
                (Some(account), Some(path)) if !shown => {
                    kagi_git::github_repos_cache::load(path, account)
                }
                _ => None,
            };
            (account, saved)
        });
        let own = cx
            .background_spawn(async move { kagi_git::github_repos::list_repos(&home_dir(), None) });
        let orgs = cx.background_spawn(async move {
            let workdir = home_dir();
            let logins = kagi_git::github_repos::list_org_logins(&workdir)?;
            Ok(kagi_git::github_repos::list_org_repos(&workdir, &logins))
        });
        cx.spawn(async move |app, acx| {
            // In this order, each step applied as soon as it is known: the
            // account and the saved list are local reads, `gh` is the network.
            let (account, saved) = cached.await;
            let _ = app.update(acx, |app, cx| {
                app.show_saved_list(generation, account.clone(), saved, cx)
            });
            let list = own.await;
            let own = app
                .update(acx, |app, cx| app.land_own_list(generation, list, cx))
                .ok()
                .flatten();
            let Some(own) = own else {
                // The organizations' read was started with the own one and
                // its `gh` keeps running: the read stays in flight (Refresh
                // joins it) until it ends, so repeated Refreshes cannot pile
                // up `gh` processes (#930 review).
                let _ = orgs.await;
                let _ = app.update(acx, |app, cx| {
                    if app.home_github.generation == generation {
                        app.home_github.refreshing = false;
                        cx.notify();
                    }
                });
                return;
            };
            let orgs = orgs.await;
            let save = app
                .update(acx, |app, cx| {
                    app.land_full_list(generation, own, orgs, started, cx)
                })
                .ok()
                .flatten();
            if let (Some(sections), Some(path), Some(account)) = (save, saved_cache, account) {
                let saved = acx
                    .background_spawn(async move {
                        kagi_git::github_repos_cache::save(&path, &account, &sections)
                    })
                    .await;
                if let Err(error) = saved {
                    eprintln!("home: could not save the GitHub list cache: {error}");
                }
            }
        })
        .detach();
    }

    /// The account this read is made as, and the list saved for it, both
    /// read off the UI thread. A list on screen read as another account
    /// (`gh auth switch`) is dropped rather than kept through the refresh;
    /// the saved list is shown while nothing newer is.
    fn show_saved_list(
        &mut self,
        generation: u64,
        account: Option<String>,
        saved: Option<Vec<OwnerRepos>>,
        cx: &mut Context<Self>,
    ) {
        if self.home_github.generation != generation {
            return;
        }
        // `data_version` moves only when the list on screen does (#942
        // review): a rebuild resets the list's scroll.
        if self.home_github.shown_account != account
            && matches!(self.home_github.repos, GithubRepos::Loaded { .. })
        {
            self.home_github.repos = GithubRepos::Loading;
            self.home_github.refreshing = false;
            self.home_github.data_version += 1;
        }
        self.home_github.shown_account = account;
        if let Some(sections) =
            saved.filter(|_| matches!(self.home_github.repos, GithubRepos::Loading))
        {
            self.home_github.repos = GithubRepos::Loaded {
                sections,
                orgs_error: None,
            };
            self.home_github.refreshing = true;
            self.home_github.data_version += 1;
        }
        cx.notify();
    }

    /// The user's own list has landed. With nothing on screen it is shown
    /// at once (the organizations follow); over a saved list it waits for
    /// the organizations and the whole list is swapped when complete. A
    /// failed read keeps a saved list and says so; with none it is the error.
    fn land_own_list(
        &mut self,
        generation: u64,
        list: Result<kagi_git::github_repos::RepoList, kagi_git::GitError>,
        cx: &mut Context<Self>,
    ) -> Option<OwnerRepos> {
        if self.home_github.generation != generation {
            return None;
        }
        cx.notify();
        match list {
            Ok(list) => {
                let own = OwnerRepos {
                    owner: None,
                    list: Ok(list),
                };
                // Over a list already on screen (a saved one, a refresh) it
                // waits for the organizations: nothing on screen changes,
                // so the list keeps its scroll (#942 review).
                if !self.home_github.refreshing {
                    self.home_github.repos = GithubRepos::Loaded {
                        sections: vec![own.clone()],
                        orgs_error: None,
                    };
                    self.home_github.orgs_loading = true;
                    self.home_github.data_version += 1;
                }
                Some(own)
            }
            Err(error) => {
                klog!("home: github failed: {error}");
                if self.home_github.refreshing {
                    self.push_toast(
                        super::ToastKind::Error,
                        Msg::HomeGithubRefreshFailed
                            .t()
                            .replace("{}", &error.to_string()),
                        cx,
                    );
                } else {
                    self.home_github.repos = GithubRepos::Failed(error.to_string());
                    self.home_github.data_version += 1;
                }
                // Still reading until the organizations' `gh` ends.
                self.home_github.refreshing = true;
                None
            }
        }
    }

    /// The organizations' lists have landed after the user's own: the whole
    /// list replaces what is on screen. Returns what to save — only a
    /// complete list, so a failed organization listing does not erase the
    /// saved organizations.
    fn land_full_list(
        &mut self,
        generation: u64,
        own: OwnerRepos,
        orgs: Result<Vec<OwnerRepos>, String>,
        started: std::time::Instant,
        cx: &mut Context<Self>,
    ) -> Option<Vec<OwnerRepos>> {
        if self.home_github.generation != generation {
            return None;
        }
        self.home_github.data_version += 1;
        let mut sections = vec![own];
        let orgs_error = match orgs {
            Ok(orgs) => {
                sections.extend(orgs);
                None
            }
            Err(error) => {
                klog!("home: github orgs failed: {error}");
                Some(error)
            }
        };
        let repos: usize = sections
            .iter()
            .filter_map(|s| s.list.as_ref().ok())
            .map(|l| l.repos.len())
            .sum();
        klog!(
            "home: github owners={} repos={} local={} ms={}",
            sections.len(),
            repos,
            self.home_github.local.len(),
            started.elapsed().as_millis()
        );
        let save = orgs_error.is_none().then(|| sections.clone());
        self.home_github.repos = GithubRepos::Loaded {
            sections,
            orgs_error,
        };
        self.home_github.orgs_loading = false;
        self.home_github.refreshing = false;
        // A repository opened while the list was read is matched too.
        self.match_open_tabs(cx);
        cx.notify();
        save
    }

    /// The local repositories open as tabs.
    fn open_tab_paths(&self) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter(|t| t.remote.is_none())
            .map(|t| t.path.clone())
            .collect()
    }

    /// Match the open tabs not matched yet — one opened (or cloned) after
    /// the list was read — so its row says Open instead of offering a
    /// second clone. Cheap per frame: a set lookup per tab.
    pub(super) fn match_open_tabs(&mut self, cx: &mut Context<Self>) {
        let paths = self.open_tab_paths();
        self.match_local_clones(paths, cx);
    }

    /// Read the `origin` of each path not matched yet, off the UI thread (an
    /// `ssh_config` alias asks `ssh -G`), and replace those paths' entries
    /// in [`HomeGithub::local`] with what they name now.
    fn match_local_clones(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| self.home_github.local_checked.insert(p.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }
        let found = cx.background_spawn(async move {
            let found: Vec<(String, PathBuf)> = paths
                .iter()
                .filter_map(|p| {
                    kagi_git::github_repos::origin_identity(p).map(|id| (id, p.clone()))
                })
                .collect();
            (paths, found)
        });
        cx.spawn(async move |app, acx| {
            let (paths, found) = found.await;
            let _ = app.update(acx, |app, cx| {
                let local = &mut app.home_github.local;
                let before = local.clone();
                local.retain(|_, p| !paths.contains(p));
                local.extend(found);
                // An unchanged match keeps the list (and its scroll).
                if *local != before {
                    app.home_github.data_version += 1;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A listed repository was clicked: open its local clone, or review a
    /// clone of it. While it is being cloned its row brings back the running
    /// clone's card, not a new one that could not start (#944).
    pub fn home_github_pick(&mut self, listing: RepoListing, cx: &mut Context<Self>) {
        let running = self
            .home_github
            .cloning
            .clone()
            .filter(|card| card.listing.clone_source() == listing.clone_source());
        if let Some(card) = running {
            if self.clone_modal().is_none() {
                self.modal_focus = Some(cx.focus_handle());
                self.set_clone_modal(card);
                self.tick_clone_card(cx);
            }
            cx.notify();
            return;
        }
        let local = self.home_github.local.get(&listing.identity()).cloned();
        match local.filter(|p| p.is_dir()) {
            Some(path) => {
                self.open_repository(path, cx);
            }
            None => self.open_clone_card(listing, cx),
        }
        cx.notify();
    }

    /// Where the focus goes after a repository row was pressed, by pointer or
    /// keyboard (#960 review): into the clone card it opened, else to the
    /// root (the row opened its clone in a tab). Never left on the row: the
    /// row stops the Enter it receives and presses itself on the key-up, so
    /// the card's Enter would never confirm it and would open a fresh card
    /// in its place.
    pub(super) fn focus_after_pick(&self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let card = self
            .clone_modal()
            .is_some()
            .then(|| self.modal_focus.clone())
            .flatten();
        if let Some(focus) = card.or_else(|| self.root_focus.clone()) {
            window.focus(&focus, cx);
        }
    }

    /// Ask where to clone `listing`: the card opens with no folder chosen.
    pub fn open_clone_card(&mut self, listing: RepoListing, cx: &mut Context<Self>) {
        self.modal_focus = Some(cx.focus_handle());
        klog!("clone: card {}", listing.clone_source());
        self.set_clone_modal(CloneModal {
            listing,
            target: None,
            started: None,
        });
    }

    /// Choose the folder to clone into (the system folder dialog); the card
    /// is planned for it.
    pub fn change_clone_parent(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from(Msg::CloneChooseFolder.t())),
        });
        cx.spawn(async move |app, acx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            let _ = app.update(acx, |app, cx| {
                app.replan_clone(&parent, cx);
            });
        })
        .detach();
    }

    /// Plan the open card for a clone inside `parent`.
    pub fn replan_clone(&mut self, parent: &Path, cx: &mut Context<Self>) {
        let Some(modal) = self.clone_modal().cloned() else {
            return;
        };
        if modal.started.is_some() {
            return;
        }
        let target = CloneTarget::new(&modal.listing, parent);
        klog!(
            "clone: plan {} -> {} blockers={}",
            target.request.source,
            target.request.dest.display(),
            target.plan.blockers.len()
        );
        self.set_clone_modal(CloneModal {
            listing: modal.listing,
            target: Some(target),
            started: None,
        });
        cx.notify();
    }

    /// Close the card. A clone already running keeps going in the background
    /// (its row reads "Cloning…", and its result arrives as usual).
    pub fn cancel_clone(&mut self) {
        self.clear_clone_modal();
    }

    /// Confirm the card: run the planned clone in the background. Refused on
    /// the spot while any operation is latched — another clone, or a write
    /// or plan in a repository tab (the shared `op_latched` gate).
    pub fn start_clone(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.clone_modal().cloned() else {
            return;
        };
        let Some(CloneTarget { request, plan }) = modal.target.clone() else {
            return;
        };
        if modal.started.is_some() || !plan.blockers.is_empty() {
            return;
        }
        if self.op_latched() {
            let busy = if self.home_github.cloning.is_some() {
                Msg::CloneBusy.t()
            } else {
                Msg::OpInProgress.t()
            };
            self.push_toast(super::ToastKind::Error, busy, cx);
            return;
        }
        // The card stays up as the clone's progress: pressing Clone must
        // visibly do something at once (user report: a silent 3 s looked like
        // a missed click or a freeze).
        let card = CloneModal {
            started: Some(std::time::Instant::now()),
            ..modal
        };
        self.set_clone_modal(card.clone());
        self.tick_clone_card(cx);
        klog!(
            "clone: start {} -> {}",
            request.source,
            request.dest.display()
        );
        self.home_github.cloning = Some(card);
        let run = request.clone();
        let work = cx.background_spawn(async move { kagi_git::ops::execute_clone(&run, &plan) });
        cx.spawn(async move |app, acx| {
            let report = work.await;
            let _ = app.update(acx, |app, cx| app.finish_clone(request, report, cx));
        })
        .detach();
        cx.notify();
    }

    /// Redraw the running clone's card once a second, for its elapsed time,
    /// while it is on screen.
    fn tick_clone_card(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |app, acx| loop {
            acx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            let running = app
                .update(acx, |app, cx| {
                    let running = app.clone_modal().is_some_and(|m| m.started.is_some());
                    if running {
                        cx.notify();
                    }
                    running
                })
                .unwrap_or(false);
            if !running {
                break;
            }
        })
        .detach();
    }

    fn finish_clone(
        &mut self,
        request: CloneRequest,
        report: kagi_git::backend::recording::RunReport,
        cx: &mut Context<Self>,
    ) {
        self.home_github.cloning = None;
        // The card was the clone's progress; its result now arrives as a
        // toast and in Operation Log (and, on success, as the new tab).
        if self.clone_modal().is_some_and(|m| m.started.is_some()) {
            self.clear_clone_modal();
        }
        klog!(
            "clone: done {} ok={}",
            request.dest.display(),
            report.result.is_ok()
        );
        self.notice_recording_failure("clone", &report.recording, &request.dest);
        self.present_recorded(&report.recording, cx);
        if report.result.is_ok() {
            // The new tab is matched into the local index when Home draws
            // next (`match_open_tabs`), off the UI thread.
            self.open_repository(request.dest, cx);
        }
        cx.notify();
    }

    /// The GitHub section of Home's main column: a header and a search field,
    /// then one section per owner (the user first, then each organization)
    /// as a virtualized list filling the rest of the column.
    pub(crate) fn render_home_github(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let refresh = cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
            app.reload_home_github(cx);
            cx.notify();
        });
        self.match_open_tabs(cx);
        let reading = self.home_github_reading();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(self.render_home_panes(cx))
            // A saved list is on screen while the fresh read runs.
            .when(
                self.home_github.refreshing
                    || (self.home_github.work.reading && self.home_github.work.lists.is_some()),
                |el| {
                    el.child(super::e2e::measure_control(
                        "home-github-updating",
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(super::render_overlay::sync_spinner(
                                12.,
                                theme().text_muted,
                                "home-github-refreshing",
                            ))
                            .child(muted_inline(Msg::HomeGithubUpdating.t())),
                    ))
                },
            )
            .child(div().flex_1())
            .child(super::e2e::measure_control(
                "home-github-refresh",
                Button::new("home-github-refresh")
                    .ghost()
                    .small()
                    .label(Msg::HomeGithubRefresh.t())
                    .disabled(reading)
                    .on_click(refresh),
            ));
        let query = self
            .home_github
            .filter
            .as_ref()
            .map(|f| f.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let body = match (&self.home_github.repos, self.home_github.work.pane) {
            (_, super::home_work::HomePane::Prs | super::home_work::HomePane::Issues) => {
                self.render_github_list(query, cx)
            }
            (GithubRepos::NotLoaded | GithubRepos::Loading, _) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(super::render_overlay::sync_spinner(
                    14.,
                    theme().text_muted,
                    "home-github-loading",
                ))
                .child(muted_inline(Msg::HomeGithubLoading.t()))
                .into_any_element(),
            (GithubRepos::Failed(error), _) => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .child(safe_text(error)),
                )
                .child(muted(Msg::HomeGithubFailedHint.t().to_string()))
                .into_any_element(),
            (GithubRepos::Loaded { .. }, _) => self.render_github_list(query, cx),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_6()
            .flex_1()
            .min_h(px(0.))
            .child(header)
            .children(
                self.home_github
                    .filter
                    .as_ref()
                    .map(|f| div().flex_shrink_0().child(search_field(f))),
            )
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(name_with_owner: &str) -> RepoListing {
        RepoListing {
            name_with_owner: name_with_owner.to_string(),
            host: "github.com".to_string(),
            is_fork: false,
            is_private: false,
            description: String::new(),
            updated_at: String::new(),
        }
    }

    /// The card's destination is the chosen folder plus the repository's own
    /// name, cloned from the host the list came from.
    #[test]
    fn the_card_clones_into_a_folder_named_after_the_repository() {
        let parent = tempfile::tempdir().unwrap();
        let target = CloneTarget::new(&listing("acme/widgets"), parent.path());
        assert_eq!(target.request.dest, parent.path().join("widgets"));
        assert_eq!(target.request.source, "github.com/acme/widgets");
        assert!(
            target.plan.blockers.is_empty(),
            "{:?}",
            target.plan.blockers
        );
    }
}
