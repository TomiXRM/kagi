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
/// pane and what that pane shows — the repositories / local clones, the clone
/// running and whether organizations are still being read for Repositories;
/// the pull request / issue lists for the other two — and the language their
/// text is in. The entries are built again only when it changes. The panes
/// not on screen are not part of it: a read landing for one of them must not
/// rebuild (and so scroll back to the top) the list being read (#942 review).
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ListKey {
    query: String,
    pane: HomePane,
    lang: super::i18n::Lang,
    /// `data_version` for Repositories, the work lists' `version` otherwise.
    version: u64,
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
        let repos = pane == HomePane::Repos;
        let key = ListKey {
            query,
            pane,
            lang: super::i18n::lang(),
            version: if repos {
                self.home_github.data_version
            } else {
                self.home_github.work.version
            },
            cloning: repos
                .then(|| {
                    self.home_github
                        .cloning
                        .as_ref()
                        .map(|card| card.listing.clone_source())
                })
                .flatten(),
            orgs_loading: repos && self.home_github.orgs_loading,
        };
        let state = self
            .home_github
            .list
            .get_or_insert_with(|| {
                gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(600.))
            })
            .clone();
        if self.home_github.list_key.as_ref() != Some(&key) {
            // The same list read again (a refresh landing, another language)
            // keeps its place; another pane or filter starts at the top.
            let same_view = self
                .home_github
                .list_key
                .as_ref()
                .is_some_and(|old| old.pane == key.pane && old.query == key.query);
            let top = state.logical_scroll_top();
            let items = self.build_home_items(&key);
            state.reset(items.len());
            if same_view && top.item_ix < items.len() {
                state.scroll_to(top);
            }
            // A row's place among the rows (headings and notes are not
            // rows), so assistive technology learns the whole length of a
            // list it only sees part of (#944). Built with the entries, not
            // on every frame (#937).
            let mut rows = 0;
            let places: Vec<Option<usize>> = items
                .iter()
                .map(|item| {
                    matches!(item, HomeItem::Repo(..) | HomeItem::Work(..)).then(|| {
                        rows += 1;
                        rows - 1
                    })
                })
                .collect();
            self.home_github.items = items.into();
            self.home_github.places = places.into();
            self.home_github.row_count = rows;
            self.home_github.list_key = Some(key);
        }
        let items = self.home_github.items.clone();
        let (positions, rows) = (self.home_github.places.clone(), self.home_github.row_count);
        let app = cx.entity();
        let avatars = self.avatars.images.clone();
        let list = gpui::list(state, move |i, _window, _cx| match items.get(i) {
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
                let place = (positions[i].unwrap_or(0), rows);
                work_row(*kind, item, *state, place, &avatars, &app)
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
                github_row(
                    listing.clone(),
                    state,
                    (positions[i].unwrap_or(0), rows),
                    &app,
                ),
            ),
            None => div().into_any_element(),
        })
        .flex_1()
        .min_h(px(0.));
        let name = match pane {
            HomePane::Repos => Msg::HomePaneRepos.t(),
            HomePane::Prs => Msg::HomePanePrs.t(),
            HomePane::Issues => Msg::HomePaneIssues.t(),
        };
        super::list_a11y::plain_list(
            HOME_LIST,
            div().id(HOME_LIST).flex().flex_col().flex_1().min_h(px(0.)),
            name,
        )
        .child(list)
        .into_any_element()
    }

    /// The entries for `key`: the open pane's sections, filtered.
    fn build_home_items(&self, key: &ListKey) -> Vec<HomeItem> {
        super::e2e::note_home_items_built();
        let home = &self.home_github;
        let (query, cloning) = (key.query.as_str(), key.cloning.as_deref());
        match (key.pane, &home.repos) {
            (HomePane::Prs | HomePane::Issues, _) => work_items(&home.work, key.pane, query),
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

/// The list's id, which its rows name as theirs.
pub(super) const HOME_LIST: &str = "home-list";

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
/// It is row `place.0` of the `place.1` in the list, and Tab reaches it.
fn github_row(
    listing: RepoListing,
    state: &'static str,
    place: (usize, usize),
    app: &Entity<KagiApp>,
) -> AnyElement {
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
    let label = format!("{}, {state}", listing.name_with_owner);
    let app = app.clone();
    super::keyboard_nav::focusable_row(super::list_a11y::list_item(
        HOME_LIST,
        div().id(id),
        place.0,
        place.1,
        label,
    ))
    .w_full()
    .flex()
    .flex_row()
    .items_center()
    .gap_4()
    .px(super::keyboard_nav::inset(12.))
    .py(super::keyboard_nav::inset(8.))
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

/// Tier A hooks for the list's place (#942 review).
#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Scroll Home's list so entry `ix` is at its top.
    pub fn scroll_home_list_for_e2e(&self, ix: usize) {
        if let Some(state) = &self.home_github.list {
            state.scroll_to(gpui::ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// The entry at the top of Home's list.
    pub fn home_list_top_for_e2e(&self) -> Option<usize> {
        self.home_github
            .list
            .as_ref()
            .map(|state| state.logical_scroll_top().item_ix)
    }

    /// Read only the pull request / issue lists again, as a Refresh does.
    pub fn reload_home_work_for_e2e(&mut self, cx: &mut Context<Self>) {
        self.reload_home_work(cx);
    }
}
