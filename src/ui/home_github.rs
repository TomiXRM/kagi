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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, Entity, PathPromptOptions, SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::Sizable as _;
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
        /// Listed identity (`host/owner/repo`) → a local clone of it.
        local: HashMap<String, PathBuf>,
    },
    Failed(String),
}

/// Home's GitHub state, window-wide like Home itself.
#[derive(Default)]
pub struct HomeGithub {
    pub repos: GithubRepos,
    pub filter: Option<Entity<InputState>>,
    /// The clone running now (one per window, ADR-0219 decision 7).
    pub cloning: Option<CloneRequest>,
    /// Bumped per read, so an older read landing late is dropped.
    pub(super) generation: u64,
    /// The user's own list is on screen and the organizations' are still
    /// being read.
    pub orgs_loading: bool,
    /// The list on screen is the saved one (or the previous read) and a
    /// fresh read is running.
    pub refreshing: bool,
    /// The virtualized list of rows, and what it was last built for: the
    /// rows are measured once per build, so a change resets it.
    pub(super) list: Option<gpui::ListState>,
    pub(super) list_key: Option<super::home_github_list::ListKey>,
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

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
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

    /// Read the lists again. The last saved list (or the one on screen) is
    /// shown at once while the fresh read runs; with none, the user's own
    /// list is shown as soon as it lands and the organizations' are appended.
    /// The own and the organizations' reads run at the same time.
    pub fn reload_home_github(&mut self, cx: &mut Context<Self>) {
        self.home_github.generation += 1;
        let generation = self.home_github.generation;
        self.home_github.orgs_loading = false;
        let started = std::time::Instant::now();
        let cache = cache_file();
        if !matches!(self.home_github.repos, GithubRepos::Loaded { .. }) {
            self.home_github.repos = match cache
                .as_deref()
                .and_then(kagi_git::github_repos_cache::load)
            {
                Some(sections) => GithubRepos::Loaded {
                    sections,
                    local: HashMap::new(),
                },
                None => GithubRepos::Loading,
            };
        }
        self.home_github.refreshing = matches!(self.home_github.repos, GithubRepos::Loaded { .. });
        let mut paths = super::tabs::recent_repos();
        paths.extend(
            self.tabs
                .iter()
                .filter(|t| t.remote.is_none())
                .map(|t| t.path.clone()),
        );
        // Which listed repositories are already cloned: local reads only, so
        // it lands well before `gh` and a saved list on screen gets its
        // Open chips at once.
        let local = cx.background_spawn(async move {
            paths
                .into_iter()
                .filter_map(|p| kagi_git::github_repos::origin_identity(&p).map(|id| (id, p)))
                .collect::<HashMap<String, PathBuf>>()
        });
        let own = cx
            .background_spawn(async move { kagi_git::github_repos::list_repos(&home_dir(), None) });
        let orgs = cx.background_spawn(async move {
            let workdir = home_dir();
            let logins = kagi_git::github_repos::list_org_logins(&workdir);
            kagi_git::github_repos::list_org_repos(&workdir, &logins)
        });
        cx.spawn(async move |app, acx| {
            let local = local.await;
            let _ = app.update(acx, |app, cx| {
                if app.home_github.generation != generation {
                    return;
                }
                if let GithubRepos::Loaded { local: shown, .. } = &mut app.home_github.repos {
                    *shown = local.clone();
                    cx.notify();
                }
            });
            let list = own.await;
            let own = app
                .update(acx, |app, cx| {
                    app.land_own_list(generation, list, &local, cx)
                })
                .ok()
                .flatten();
            let Some(own) = own else {
                return;
            };
            let mut sections = vec![own];
            sections.extend(orgs.await);
            let saved = sections.clone();
            let landed = app
                .update(acx, |app, cx| {
                    if app.home_github.generation != generation {
                        return false;
                    }
                    let repos: usize = sections
                        .iter()
                        .filter_map(|s| s.list.as_ref().ok())
                        .map(|l| l.repos.len())
                        .sum();
                    klog!(
                        "home: github owners={} repos={} local={} ms={}",
                        sections.len(),
                        repos,
                        local.len(),
                        started.elapsed().as_millis()
                    );
                    app.home_github.repos = GithubRepos::Loaded { sections, local };
                    app.home_github.orgs_loading = false;
                    app.home_github.refreshing = false;
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if let (true, Some(path)) = (landed, cache) {
                let saved = acx
                    .background_spawn(
                        async move { kagi_git::github_repos_cache::save(&path, &saved) },
                    )
                    .await;
                if let Err(error) = saved {
                    eprintln!("home: could not save the GitHub list cache: {error}");
                }
            }
        })
        .detach();
    }

    /// The user's own list has landed. With nothing on screen it is shown
    /// at once (the organizations follow); over a saved list it waits for
    /// the organizations and the whole list is swapped when complete. A
    /// failed read keeps a saved list and says so; with none it is the error.
    fn land_own_list(
        &mut self,
        generation: u64,
        list: Result<kagi_git::github_repos::RepoList, kagi_git::GitError>,
        local: &HashMap<String, PathBuf>,
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
                if !self.home_github.refreshing {
                    self.home_github.repos = GithubRepos::Loaded {
                        sections: vec![own.clone()],
                        local: local.clone(),
                    };
                    self.home_github.orgs_loading = true;
                }
                Some(own)
            }
            Err(error) => {
                klog!("home: github failed: {error}");
                if self.home_github.refreshing {
                    self.home_github.refreshing = false;
                    self.push_toast(
                        super::ToastKind::Error,
                        Msg::HomeGithubRefreshFailed
                            .t()
                            .replace("{}", &error.to_string()),
                        cx,
                    );
                } else {
                    self.home_github.repos = GithubRepos::Failed(error.to_string());
                }
                None
            }
        }
    }

    /// A listed repository was clicked: open its local clone, or review a
    /// clone of it.
    pub fn home_github_pick(&mut self, listing: RepoListing, cx: &mut Context<Self>) {
        let local = match &self.home_github.repos {
            GithubRepos::Loaded { local, .. } => local.get(&listing.identity()).cloned(),
            _ => None,
        };
        match local.filter(|p| p.is_dir()) {
            Some(path) => {
                self.open_repository(path, cx);
            }
            None => self.open_clone_card(listing, cx),
        }
        cx.notify();
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
    /// the spot while another clone of this window is running.
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
        if self.home_github.cloning.is_some() {
            self.push_toast(super::ToastKind::Error, Msg::CloneBusy.t(), cx);
            return;
        }
        // The card stays up as the clone's progress: pressing Clone must
        // visibly do something at once (user report: a silent 3 s looked like
        // a missed click or a freeze).
        self.set_clone_modal(CloneModal {
            started: Some(std::time::Instant::now()),
            ..modal
        });
        self.tick_clone_card(cx);
        klog!(
            "clone: start {} -> {}",
            request.source,
            request.dest.display()
        );
        self.home_github.cloning = Some(request.clone());
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
            if let GithubRepos::Loaded { local, .. } = &mut self.home_github.repos {
                if let Some(id) = kagi_git::github_repos::origin_identity(&request.dest) {
                    local.insert(id, request.dest.clone());
                }
            }
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
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(Msg::HomeGithubTitle.t())),
            )
            // A saved list is on screen while the fresh read runs.
            .when(self.home_github.refreshing, |el| {
                el.child(super::render_overlay::sync_spinner(
                    12.,
                    theme().text_muted,
                    "home-github-refreshing",
                ))
                .child(muted_inline(Msg::HomeGithubUpdating.t()))
            })
            .child(div().flex_1())
            .child(super::e2e::measure_control(
                "home-github-refresh",
                Button::new("home-github-refresh")
                    .ghost()
                    .small()
                    .label(Msg::HomeGithubRefresh.t())
                    .on_click(refresh),
            ));
        let query = self
            .home_github
            .filter
            .as_ref()
            .map(|f| f.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let body = match &self.home_github.repos {
            GithubRepos::NotLoaded | GithubRepos::Loading => div()
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
            GithubRepos::Failed(error) => div()
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
            GithubRepos::Loaded { .. } => self.render_github_list(query, cx),
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
