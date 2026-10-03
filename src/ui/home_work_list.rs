//! Drawing Home's pull request and issue panes (#928): their sections as
//! entries of the same virtualized list as the repositories, and their rows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{div, prelude::*, px, rgb, AnyElement, Entity, SharedString};
use kagi_git::github_repos_cache::{WorkItem, WorkKind};
use kagi_ui_core::avatar::AvatarImages;

use super::home_github_list::HomeItem;
use super::home_work::{HomePane, HomeWork};
use super::i18n::Msg;
use super::render_helpers::safe_text;
use super::theme::{self, theme};
use super::KagiApp;

/// What a click on a row does, shown at its end.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkRowState {
    Open,
    Clone,
    Cloning,
    /// The PR's refs are being read before it opens; clicks are ignored.
    Opening,
}

const AVATAR: f32 = 20.;

fn heading(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::MyPrs => Msg::HomeWorkMine.t(),
        WorkKind::ReviewRequests => Msg::HomeWorkReview.t(),
        WorkKind::AssignedIssues => Msg::IssuesAssignedToMe.t(),
    }
}

fn empty(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::MyPrs => Msg::HomeWorkNoPrs.t(),
        WorkKind::ReviewRequests => Msg::HomeWorkNoReview.t(),
        WorkKind::AssignedIssues => Msg::HomeWorkNoIssues.t(),
    }
}

fn matches(item: &WorkItem, query: &str) -> bool {
    query.is_empty()
        || item.title.to_lowercase().contains(query)
        || item.name_with_owner.to_lowercase().contains(query)
        || format!("#{}", item.number).starts_with(query)
}

/// The pane's entries: per list a heading, why its last read failed, its
/// rows matching `query`, and a note when it is empty or cut at the search
/// limit. A list without a match is left out while filtering. Before any
/// list has been read or loaded from the cache, one spinner row.
pub(super) fn work_items(
    work: &HomeWork,
    pane: HomePane,
    local: &HashMap<String, PathBuf>,
    query: &str,
    cloning: Option<&str>,
) -> Vec<HomeItem> {
    let Some(lists) = &work.lists else {
        return vec![HomeItem::Loading(
            Msg::HomeWorkLoading.t(),
            "home-work-loading",
        )];
    };
    let kinds: &[WorkKind] = match pane {
        HomePane::Issues => &[WorkKind::AssignedIssues],
        _ => &[WorkKind::MyPrs, WorkKind::ReviewRequests],
    };
    let mut items = Vec::new();
    let mut shown = 0usize;
    for &kind in kinds {
        let list = lists.get(kind);
        let rows: Vec<HomeItem> = list
            .items
            .iter()
            .filter(|item| matches(item, query))
            .map(|item| {
                let identity = item.identity();
                let state = if work.opening.as_ref() == Some(&(identity.clone(), item.number)) {
                    WorkRowState::Opening
                } else if local.contains_key(&identity) {
                    WorkRowState::Open
                } else if cloning == Some(item.repo().clone_source().as_str()) {
                    WorkRowState::Cloning
                } else {
                    WorkRowState::Clone
                };
                HomeItem::Work(kind, item.clone(), state)
            })
            .collect();
        if rows.is_empty() && !query.is_empty() {
            continue;
        }
        shown += rows.len();
        items.push(HomeItem::Heading(heading(kind).to_string()));
        if let Some(error) = work.errors.get(&kind) {
            items.push(HomeItem::WorkFailed(
                Msg::HomeWorkFailed.t().replace("{}", error),
            ));
        }
        if list.items.is_empty() && !work.errors.contains_key(&kind) {
            items.push(HomeItem::Note(empty(kind).to_string()));
        }
        items.extend(rows);
        if list.truncated {
            items.push(HomeItem::Note(Msg::HomeWorkTruncated.t().to_string()));
        }
    }
    if shown == 0 && !query.is_empty() {
        items.push(HomeItem::Note(Msg::HomeWorkNoMatch.t().to_string()));
    }
    items
}

/// One PR or issue: its kind as an icon, the title over `owner/repo #N` and
/// the last update; a review request also shows who asked, with their
/// avatar. At the end what a click does, or a spinner while it opens.
pub(super) fn work_row(
    kind: WorkKind,
    item: &WorkItem,
    state: WorkRowState,
    avatars: &Arc<AvatarImages>,
    app: &Entity<KagiApp>,
) -> AnyElement {
    let (icon, color) = match (kind.is_pr(), item.is_draft) {
        (true, true) => ("icons/git-pull-request-draft.svg", theme().text_muted),
        (true, false) => ("icons/git-pull-request.svg", theme().color_success),
        (false, _) => ("icons/circle-dot.svg", theme().color_success),
    };
    let key = format!("home-work-{}-{}", item.name_with_owner, item.number);
    let mut detail = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(format!(
            "{} #{}",
            item.name_with_owner, item.number
        )));
    if kind == WorkKind::ReviewRequests && !item.author.is_empty() {
        detail = detail
            .child(kagi_ui_core::commit_header::avatar_circle_with_initials(
                AVATAR,
                &item.author,
                &item.author,
                avatars,
            ))
            .child(safe_text(&item.author));
    }
    if let Some(day) = item.updated_at.get(..10) {
        detail = detail.child(SharedString::from(day.to_string()));
    }
    if item.is_draft {
        detail = detail.child(chip(Msg::PrDraft.t(), theme().text_sub));
    }
    let end = match state {
        WorkRowState::Opening => super::e2e::measure_control(
            format!("{key}-opening"),
            super::render_overlay::sync_spinner(
                14.,
                theme().text_muted,
                SharedString::from(format!("{key}-opening")),
            ),
        ),
        WorkRowState::Open => chip(Msg::HomeGithubOpen.t(), theme().color_branch),
        WorkRowState::Clone => chip(Msg::HomeGithubClone.t(), theme().color_branch),
        WorkRowState::Cloning => chip(Msg::HomeGithubCloning.t(), theme().color_branch),
    };
    let app = app.clone();
    let picked = item.clone();
    let row = div()
        .id(SharedString::from(key.clone()))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .when(state != WorkRowState::Opening, |el| {
            el.cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| s.bg(rgb(theme().surface)))
        })
        .on_click(move |_, window, cx| {
            app.update(cx, |app, cx| {
                app.home_work_pick(kind, picked.clone(), window, cx)
            });
        })
        .child(
            gpui::svg()
                .path(icon)
                .flex_shrink_0()
                .size(theme::scaled_px(16.))
                .text_color(rgb(color)),
        )
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
                        .child(safe_text(&item.title)),
                )
                .child(detail),
        )
        .child(end);
    super::e2e::measure_control(key, row)
}

fn chip(text: &str, color: u32) -> AnyElement {
    div()
        .flex_shrink_0()
        .px_2()
        .rounded_md()
        .bg(rgb(theme().surface))
        .text_xs()
        .text_color(rgb(color))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}
