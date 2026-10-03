//! Drawing Home's GitHub list (#923, ADR-0219): the search field, the owner
//! sections flattened into one virtualized list, and its rows. Split out of
//! `home_github.rs`, which owns reading the list and cloning.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{div, prelude::*, px, rgb, AnyElement, Context, Entity, SharedString};
use gpui_component::input::{Input, InputState};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_git::github_repos::{OwnerRepos, RepoListing};
use kagi_git::github_repos_cache::{WorkItem, WorkKind};

use super::home_github::GithubRepos;
use super::home_work::HomePane;
use super::home_work_list::{work_items, work_row, WorkRowState};
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// Everything the list's entries are built from (#937): the filter, the
/// pane, the versions of the repositories / local clones and of the pull
/// request / issue lists, the clone running and whether organizations are
/// still being read. The entries are built again only when it changes.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ListKey {
    query: String,
    pane: HomePane,
    data_version: u64,
    work_version: u64,
    cloning: Option<String>,
    orgs_loading: bool,
}

/// One entry of the virtualized list.
#[derive(Clone)]
pub(super) enum HomeItem {
    Heading(String),
    Note(String),
    Repo(RepoListing, &'static str),
    /// Something is still being read: a spinner row with its label and the
    /// spinner's id.
    Loading(&'static str, &'static str),
    /// The organizations could not be listed at all, and why.
    OrgsFailed(String),
    /// A pull request or issue (#928), and what a click on it does.
    Work(WorkKind, WorkItem, WorkRowState),
    /// A pull request / issue list could not be read, and why.
    WorkFailed(String),
}

impl KagiApp {
    /// The rows, built once per change of the data or the filter (not per
    /// frame — a spinner animating or a key typed redraws without them)
    /// and drawn through `gpui::list`, which lays out only what is on
    /// screen.
    pub(super) fn render_github_list(
        &mut self,
        query: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane = self.home_github.work.pane;
        if pane == HomePane::Repos && !matches!(self.home_github.repos, GithubRepos::Loaded { .. })
        {
            return div().into_any_element();
        }
        let key = ListKey {
            query,
            pane,
            data_version: self.home_github.data_version,
            work_version: self.home_github.work.version,
            cloning: self.home_github.cloning.as_ref().map(|r| r.source.clone()),
            orgs_loading: self.home_github.orgs_loading,
        };
        let state = self
            .home_github
            .list
            .get_or_insert_with(|| {
                gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(600.))
            })
            .clone();
        if self.home_github.list_key.as_ref() != Some(&key) {
            let items = self.build_home_items(&key);
            state.reset(items.len());
            self.home_github.items = items.into();
            self.home_github.list_key = Some(key);
        }
        let items = self.home_github.items.clone();
        let app = cx.entity();
        let avatars = self.avatars.images.clone();
        gpui::list(state, move |i, _window, _cx| match items.get(i) {
            Some(HomeItem::Heading(title)) => section_heading(title).into_any_element(),
            Some(HomeItem::Note(text)) => muted(text.clone()),
            Some(HomeItem::OrgsFailed(text)) => {
                super::e2e::measure_control("home-github-orgs-failed", muted(text.clone()))
            }
            Some(HomeItem::Loading(text, spinner)) => super::e2e::measure_control(
                *spinner,
                div()
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
                        *spinner,
                    ))
                    .child(muted_inline(text)),
            ),
            Some(HomeItem::Work(kind, item, state)) => {
                work_row(*kind, item, *state, &avatars, &app)
            }
            Some(HomeItem::WorkFailed(text)) => super::e2e::measure_control(
                "home-work-failed",
                div()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .child(safe_text(text)),
            ),
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

    /// The entries for `key`: the open pane's sections, filtered.
    fn build_home_items(&self, key: &ListKey) -> Vec<HomeItem> {
        super::e2e::note_home_items_built();
        let home = &self.home_github;
        let (query, cloning) = (key.query.as_str(), key.cloning.as_deref());
        match (key.pane, &home.repos) {
            (HomePane::Prs | HomePane::Issues, _) => {
                work_items(&home.work, key.pane, &home.local, query, cloning)
            }
            (
                HomePane::Repos,
                GithubRepos::Loaded {
                    sections,
                    orgs_error,
                },
            ) => {
                let mut items =
                    github_items(sections, orgs_error.as_deref(), &home.local, query, cloning);
                if key.orgs_loading {
                    items.push(HomeItem::Loading(
                        Msg::HomeGithubOrgsLoading.t(),
                        "home-github-orgs-loading",
                    ));
                }
                items
            }
            (HomePane::Repos, _) => Vec::new(),
        }
    }
}

/// The list's entries: per owner a heading, its notes (empty, unreadable,
/// truncated) and the repositories matching `query`; an owner without a
/// match is left out while filtering. When the organizations could not be
/// listed at all, a section says so instead of there being none.
fn github_items(
    sections: &[OwnerRepos],
    orgs_error: Option<&str>,
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
    if let Some(error) = orgs_error.filter(|_| query.is_empty()) {
        items.push(HomeItem::Heading(Msg::HomeGithubOrgs.t().to_string()));
        items.push(HomeItem::OrgsFailed(
            Msg::HomeGithubOrgsFailed.t().replace("{}", error),
        ));
    }
    if shown == 0 && !query.is_empty() {
        items.push(HomeItem::Note(Msg::HomeGithubNoMatch.t().to_string()));
    }
    items
}

/// A large, borderless search field in a soft rounded box with a magnifier
/// — the search look of current desktop apps, not a form input.
pub(super) fn search_field(input: &Entity<InputState>) -> AnyElement {
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

pub(super) fn muted(text: String) -> AnyElement {
    div()
        .px_3()
        .py_1()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// Muted text without its own padding, for use beside a spinner.
pub(super) fn muted_inline(text: &str) -> impl IntoElement {
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
    meta = meta.child(super::e2e::measure_control(
        format!("home-gh-{}:{state}", listing.name_with_owner),
        chip(state).text_color(rgb(theme().color_branch)),
    ));
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
