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
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_git::github_repos::{OwnerRepos, RepoListing};
use kagi_git::ops::{plan_clone, CloneRequest};
use kagi_git::OperationPlan;

use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
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
    generation: u64,
    /// The user's own list is on screen and the organizations' are still
    /// being read.
    pub orgs_loading: bool,
    /// The virtualized list of rows, and what it was last built for: the
    /// rows are measured once per build, so a change resets it.
    list: Option<gpui::ListState>,
    list_key: Option<ListKey>,
}

/// What the list's rows depend on besides the data itself.
#[derive(Clone, PartialEq, Eq)]
struct ListKey {
    query: String,
    generation: u64,
    cloning: Option<String>,
    local: usize,
    sections: usize,
    orgs_loading: bool,
}

/// One entry of the virtualized list.
#[derive(Clone)]
enum HomeItem {
    Heading(String),
    Note(String),
    Repo(RepoListing, &'static str),
    /// The organizations are still being read: a spinner row at the end.
    Loading,
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

    /// Read the lists again: the user's own (with the local clones) first,
    /// shown as soon as it lands, then the organizations', read in parallel
    /// and appended. Waiting for every owner before drawing anything made
    /// Home look stuck (user report).
    pub fn reload_home_github(&mut self, cx: &mut Context<Self>) {
        self.home_github.generation += 1;
        let generation = self.home_github.generation;
        self.home_github.repos = GithubRepos::Loading;
        self.home_github.orgs_loading = false;
        let started = std::time::Instant::now();
        let mut paths = super::tabs::recent_repos();
        paths.extend(
            self.tabs
                .iter()
                .filter(|t| t.remote.is_none())
                .map(|t| t.path.clone()),
        );
        let own = cx.background_spawn(async move {
            let workdir = home_dir();
            let list = kagi_git::github_repos::list_repos(&workdir, None);
            let local: HashMap<String, PathBuf> = paths
                .into_iter()
                .filter_map(|p| kagi_git::github_repos::origin_identity(&p).map(|id| (id, p)))
                .collect();
            (list, local)
        });
        cx.spawn(async move |app, acx| {
            let (list, local) = own.await;
            let loaded = app
                .update(acx, |app, cx| {
                    if app.home_github.generation != generation {
                        return false;
                    }
                    let loaded = match list {
                        Ok(list) => {
                            app.home_github.repos = GithubRepos::Loaded {
                                sections: vec![OwnerRepos {
                                    owner: None,
                                    list: Ok(list),
                                }],
                                local,
                            };
                            app.home_github.orgs_loading = true;
                            true
                        }
                        Err(error) => {
                            klog!("home: github failed: {error}");
                            app.home_github.repos = GithubRepos::Failed(error.to_string());
                            false
                        }
                    };
                    cx.notify();
                    loaded
                })
                .unwrap_or(false);
            if !loaded {
                return;
            }
            let orgs = acx
                .background_spawn(async move {
                    let workdir = home_dir();
                    let logins = kagi_git::github_repos::list_org_logins(&workdir);
                    kagi_git::github_repos::list_org_repos(&workdir, &logins)
                })
                .await;
            let _ = app.update(acx, |app, cx| {
                if app.home_github.generation != generation {
                    return;
                }
                app.home_github.orgs_loading = false;
                if let GithubRepos::Loaded { sections, local } = &mut app.home_github.repos {
                    sections.extend(orgs);
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
                }
                cx.notify();
            });
        })
        .detach();
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
                    .flex_1()
                    .text_lg()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(Msg::HomeGithubTitle.t())),
            )
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

    /// The rows, built once per change of the data or the filter and drawn
    /// through `gpui::list`, which lays out only what is on screen.
    fn render_github_list(&mut self, query: String, cx: &mut Context<Self>) -> AnyElement {
        let GithubRepos::Loaded { sections, local } = &self.home_github.repos else {
            return div().into_any_element();
        };
        let cloning = self.home_github.cloning.as_ref().map(|r| r.source.clone());
        let mut items = github_items(sections, local, &query, cloning.as_deref());
        let orgs_loading = self.home_github.orgs_loading;
        if orgs_loading {
            items.push(HomeItem::Loading);
        }
        let key = ListKey {
            query,
            generation: self.home_github.generation,
            cloning,
            local: local.len(),
            sections: sections.len(),
            orgs_loading,
        };
        let state = self
            .home_github
            .list
            .get_or_insert_with(|| {
                gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(600.))
            })
            .clone();
        if self.home_github.list_key.as_ref() != Some(&key) || state.item_count() != items.len() {
            state.reset(items.len());
            self.home_github.list_key = Some(key);
        }
        let app = cx.entity();
        gpui::list(state, move |i, _window, _cx| match items.get(i) {
            Some(HomeItem::Heading(title)) => section_heading(title).into_any_element(),
            Some(HomeItem::Note(text)) => muted(text.clone()),
            Some(HomeItem::Loading) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .pt_4()
                .pb_2()
                .child(super::render_overlay::sync_spinner(
                    14.,
                    theme().text_muted,
                    "home-github-orgs-loading",
                ))
                .child(muted_inline(Msg::HomeGithubOrgsLoading.t()))
                .into_any_element(),
            Some(HomeItem::Repo(listing, state)) => super::e2e::measure_control(
                format!("home-gh-{}", listing.name_with_owner),
                github_row(listing.clone(), state, &app),
            ),
            None => div().into_any_element(),
        })
        .flex_1()
        .min_h(px(0.))
        .into_any_element()
    }
}

/// The list's entries: per owner a heading, its notes (empty, unreadable,
/// truncated) and the repositories matching `query`; an owner without a
/// match is left out while filtering.
fn github_items(
    sections: &[OwnerRepos],
    local: &HashMap<String, PathBuf>,
    query: &str,
    cloning: Option<&str>,
) -> Vec<HomeItem> {
    let mut items = Vec::new();
    let mut shown = 0usize;
    for section in sections {
        let title = section
            .owner
            .clone()
            .unwrap_or_else(|| Msg::HomeGithubMine.t().to_string());
        let list = match &section.list {
            Ok(list) => list,
            Err(error) => {
                if query.is_empty() {
                    items.push(HomeItem::Heading(title));
                    items.push(HomeItem::Note(
                        Msg::HomeGithubOwnerFailed.t().replace("{}", error),
                    ));
                }
                continue;
            }
        };
        let rows: Vec<HomeItem> = list
            .repos
            .iter()
            .filter(|l| {
                query.is_empty()
                    || l.name_with_owner.to_lowercase().contains(query)
                    || l.description.to_lowercase().contains(query)
            })
            .map(|l| {
                let state = if cloning == Some(l.clone_source().as_str()) {
                    Msg::HomeGithubCloning.t()
                } else if local.contains_key(&l.identity()) {
                    Msg::HomeGithubOpen.t()
                } else {
                    Msg::HomeGithubClone.t()
                };
                HomeItem::Repo(l.clone(), state)
            })
            .collect();
        if rows.is_empty() && !query.is_empty() {
            continue;
        }
        shown += rows.len();
        items.push(HomeItem::Heading(title));
        if list.repos.is_empty() {
            items.push(HomeItem::Note(Msg::HomeGithubEmpty.t().to_string()));
        }
        items.extend(rows);
        if list.truncated {
            items.push(HomeItem::Note(
                Msg::HomeGithubTruncated
                    .t()
                    .replace("{}", &list.repos.len().to_string()),
            ));
        }
    }
    if shown == 0 && !query.is_empty() {
        items.push(HomeItem::Note(Msg::HomeGithubNoMatch.t().to_string()));
    }
    items
}

/// A large, borderless search field in a soft rounded box with a magnifier
/// — the search look of current desktop apps, not a form input.
fn search_field(input: &Entity<InputState>) -> AnyElement {
    // A flex row so the input takes the box's whole width: as a plain child
    // its `size_full` did not resolve, leaving the text area zero-wide —
    // typed characters and the placeholder were drawn into nothing.
    div()
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .px_2()
        .py_1()
        .rounded_xl()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .child(
            Input::new(input)
                .appearance(false)
                .cleanable(true)
                .large()
                .flex_1()
                .prefix(
                    Icon::new(IconName::Search)
                        .size(theme::scaled_px(18.))
                        .text_color(rgb(theme().text_muted)),
                ),
        )
        .into_any_element()
}

fn section_heading(title: &str) -> impl IntoElement {
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(title.to_string()))
}

fn muted(text: String) -> AnyElement {
    div()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// Muted text without its own padding, for use beside a spinner.
fn muted_inline(text: &str) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text.to_string()))
}

/// One repository: its name over its description on the left; on the right
/// the last update, private / fork marks and what a click does, as a chip.
fn github_row(listing: RepoListing, state: &'static str, app: &Entity<KagiApp>) -> AnyElement {
    let chip = |text: &str| {
        div()
            .flex_shrink_0()
            .px_2()
            .rounded_md()
            .bg(rgb(theme().surface))
            .text_xs()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(text.to_string()))
    };
    let mut meta = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0();
    if let Some(day) = listing.updated_at.get(..10) {
        meta = meta.child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(day.to_string())),
        );
    }
    if listing.is_private {
        meta = meta.child(chip(Msg::HomeGithubPrivate.t()));
    }
    if listing.is_fork {
        meta = meta.child(chip(Msg::HomeGithubFork.t()));
    }
    meta = meta.child(chip(state).text_color(rgb(theme().color_branch)));
    let description = (!listing.description.is_empty()).then(|| {
        div()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .truncate()
            .child(safe_text(&listing.description))
    });
    let id = SharedString::from(format!("home-gh-row-{}", listing.name_with_owner));
    let name = listing.name().to_string();
    let app = app.clone();
    div()
        .id(id)
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_3()
        .py_2()
        .rounded_lg()
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(move |_, _, cx| {
            app.update(cx, |app, cx| app.home_github_pick(listing.clone(), cx));
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_base()
                        .text_color(rgb(theme().text_main))
                        .truncate()
                        .child(SharedString::from(name)),
                )
                .children(description),
        )
        .child(meta)
        .into_any_element()
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
