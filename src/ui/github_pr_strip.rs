//! Closed/All PR reads belong to the workspace, never to shared open evidence.
use std::path::PathBuf;

use gpui::Context;
use kagi_domain::{
    github::{PrListSnapshot, PullRequest},
    list_filter::StateFilter,
};
use kagi_git::github::PrFetchError;

use super::{tab_view::TabUiState, KagiApp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PrPageRequest {
    pub generation: u64,
    pub attempt: u64,
    pub visit: u64,
    pub state: StateFilter,
    pub base_repo: String,
    pub cursor: String,
}

#[derive(Default)]
pub(super) struct PrPageState {
    pub base_repo: Option<String>,
    pub cursor: Option<String>,
    pub page: usize,
    pub append: Option<PrPageRequest>,
    pub attempt: u64,
}

#[derive(Default)]
pub(super) struct PrStripRead {
    pub rows: Option<Vec<PullRequest>>,
    pub generation: u64,
    pub request_state: StateFilter,
    pub loading: bool,
    pub error: Option<String>,
    pub paging: PrPageState,
}

impl TabUiState {
    pub(super) fn pr_list_rows(&self) -> &[PullRequest] {
        self.github_prs_strip
            .rows
            .as_deref()
            .unwrap_or(&self.github_prs)
    }

    pub(super) fn pr_list_revision(&self) -> (u64, u64, u64, u64) {
        (
            self.github_prs_gen,
            self.github_prs_strip.generation,
            self.github_prs_epoch,
            self.github_prs_visit,
        )
    }

    pub(super) fn pr_list_paging(&self) -> &PrPageState {
        if self.github_prs_strip.rows.is_some() {
            &self.github_prs_strip.paging
        } else {
            &self.github_prs_paging
        }
    }

    pub(super) fn pr_list_paging_mut(&mut self) -> &mut PrPageState {
        if self.github_prs_strip.rows.is_some() {
            &mut self.github_prs_strip.paging
        } else {
            &mut self.github_prs_paging
        }
    }

    pub(super) fn pr_list_has_more(&self) -> bool {
        self.pr_list_paging().cursor.is_some()
    }

    pub(super) fn pr_list_loading_more(&self) -> bool {
        self.pr_list_paging().append.is_some()
    }

    pub(super) fn pr_list_loading(&self) -> bool {
        if self.github_prs_strip.rows.is_some() {
            self.github_prs_strip.loading
        } else {
            self.github_prs_loading
        }
    }

    pub(super) fn pr_list_error(&self) -> Option<&str> {
        if self.github_prs_strip.rows.is_some() {
            self.github_prs_strip.error.as_deref()
        } else {
            self.github_error.as_deref()
        }
    }

    /// Repository tab departure keeps user intent and accepted membership.
    /// Only reads admitted by the old workspace visit lose publication rights.
    /// A revoked first read keeps its pending intent until activation starts
    /// a new ordinary read; it must not turn an unobserved collection into zero.
    pub(super) fn invalidate_pr_list_visit(&mut self) {
        self.github_prs_visit = self.github_prs_visit.wrapping_add(1);
        self.github_prs_paging.append = None;
        self.github_prs_strip.generation = self.github_prs_strip.generation.wrapping_add(1);
        self.github_prs_strip.paging.append = None;
    }

    pub(super) fn reset_pr_strip(&mut self) {
        self.invalidate_pr_list_visit();
        let generation = self.github_prs_strip.generation;
        self.github_prs_strip = PrStripRead {
            generation,
            ..Default::default()
        };
        self.github_pr_filter.common.state = StateFilter::Open;
    }

    pub(super) fn leave_pr_mode(&mut self) {
        self.pr_mode = None;
        self.filter_controls.menu = None;
        self.reset_pr_strip();
    }
}

pub(super) fn pr_list_task(
    repo: PathBuf,
    base_repo: Option<String>,
    cursor: Option<String>,
    state: StateFilter,
    injected: Option<gpui::Task<Result<PrListSnapshot, PrFetchError>>>,
    cx: &mut Context<KagiApp>,
) -> gpui::Task<Result<PrListSnapshot, PrFetchError>> {
    if let Some(task) = injected {
        return task;
    }
    cx.background_executor().spawn(async move {
        kagi_git::github::list_prs(&repo, base_repo.as_deref(), cursor.as_deref(), state)
    })
}

impl KagiApp {
    /// Explicit workspace refresh. The periodic/shared refresh always reads Open.
    pub(super) fn refresh_pr_strip(&mut self, cx: &mut Context<Self>) {
        let state = self.ui().github_pr_filter.common.state;
        if state == StateFilter::Open {
            self.refresh_github_prs(cx);
            return;
        }
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        #[cfg(feature = "gui-e2e")]
        let injected = super::e2e::take_github_pr_fetch();
        #[cfg(not(feature = "gui-e2e"))]
        let injected = None;
        if injected.is_none() && !kagi_git::github::gh_available() {
            return;
        }
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        let generation = ui.begin_pr_strip_request(state);
        let task = pr_list_task(repo.clone(), None, None, state, injected, cx);
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                let Some(outcome) = ui.finish_pr_strip_request(generation, state, result) else {
                    return;
                };
                let (moved, base_repos) = if outcome.error.is_none() {
                    (
                        super::github_pr_detail::sync_open_pr_tabs(ui),
                        super::github::distinct_base_repos(ui.pr_list_rows()),
                    )
                } else {
                    (Vec::new(), Vec::new())
                };
                for base_repo in &base_repos {
                    app.ensure_host_login(base_repo, cx);
                }
                let active = app.active_session() == Some(owner);
                app.reload_moved_pr_tabs(active, &moved, cx);
                app.refresh_pr_detail_targets(owner, repo, cx);
                if active {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A single continuation slot, frozen to the held collection and PR visit.
    pub(super) fn load_more_github_prs_for(
        &mut self,
        owner: crate::app::SessionId,
        repo: PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.active_session() != Some(owner) {
            return;
        }
        let Some(request) = self
            .ui
            .get_mut(&owner)
            .and_then(TabUiState::begin_pr_page_request)
        else {
            return;
        };
        #[cfg(feature = "gui-e2e")]
        let injected = super::e2e::take_github_pr_fetch();
        #[cfg(not(feature = "gui-e2e"))]
        let injected = None;
        let task = pr_list_task(
            repo,
            Some(request.base_repo.clone()),
            Some(request.cursor.clone()),
            request.state,
            injected,
            cx,
        );
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                let anchor = capture_page_anchor(ui);
                if !ui.finish_pr_page_request(&request, result) {
                    return;
                }
                if ui.pr_list_error().is_none() {
                    restore_page_anchor(ui, anchor);
                    // L1 append does not force L2/L3 refresh. Only the newly
                    // clipped visible range and already-opened PRs demand detail.
                    let moved = super::github_pr_detail::sync_open_pr_tabs(ui);
                    let active = app.active_session() == Some(owner);
                    app.reload_moved_pr_tabs(active, &moved, cx);
                }
                if app.active_session() == Some(owner) {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

struct PageAnchor {
    key: kagi_domain::github::PrKey,
    index: usize,
    height: gpui::Pixels,
    offset: gpui::Point<gpui::Pixels>,
}

fn projected_rows(ui: &TabUiState) -> Vec<usize> {
    kagi_domain::list_filter::apply_prs(ui.pr_list_rows(), &ui.github_pr_filter, |pr| {
        ui.pr_details
            .availability(pr, super::github_pr_detail::PrDetailStage::Status)
    })
}

fn capture_page_anchor(ui: &TabUiState) -> Option<PageAnchor> {
    let mode = ui.pr_mode.as_ref()?;
    if mode.active.is_some() {
        return None;
    }
    let order = projected_rows(ui);
    let scroll = mode.dashboard_scroll.0.borrow();
    if scroll.deferred_scroll_to_item.is_some() || order.is_empty() {
        return None;
    }
    let height = scroll.last_item_size?.contents.height / (order.len() + 1) as f32;
    if height <= gpui::px(0.) {
        return None;
    }
    let offset = scroll.base_handle.offset();
    let index = ((-offset.y / height).floor() as usize).min(order.len() - 1);
    Some(PageAnchor {
        key: ui.pr_list_rows()[order[index]].key(),
        index,
        height,
        offset,
    })
}

fn restore_page_anchor(ui: &TabUiState, anchor: Option<PageAnchor>) {
    let (Some(anchor), Some(mode)) = (anchor, ui.pr_mode.as_ref()) else {
        return;
    };
    let order = projected_rows(ui);
    if let Some(index) = order
        .iter()
        .position(|&source| ui.pr_list_rows()[source].is(&anchor.key))
    {
        if index != anchor.index {
            let mut offset = anchor.offset;
            offset.y -= anchor.height * (index as f32 - anchor.index as f32);
            mode.dashboard_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(offset);
        }
    }
}

/// The same bounded continuation control serves the table and navigator.
/// Only a genuinely clipped table tail may automatically demand a page.
pub(super) fn render_pr_page_tail(
    app: &KagiApp,
    automatic: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    use super::{i18n::Msg, theme::theme};
    use gpui::{div, prelude::*, px, rgb};
    let ui = app.ui();
    let mut tail = div()
        .id(if automatic {
            "pr-main-page-tail"
        } else {
            "pr-sidebar-page-tail"
        })
        .flex_shrink_0()
        .relative()
        .px_3()
        .py_2()
        .text_xs()
        // The main tail occupies one uniform table row. Keep retry and its
        // reason on one line; the complete error remains above the table.
        .when(automatic, |tail| tail.flex().items_center().gap_2());
    if ui.pr_list_loading_more() {
        tail = tail
            .text_color(rgb(theme().text_muted))
            .child(Msg::PrLoadingMore.t());
    } else if ui.pr_list_has_more() && !ui.pr_list_loading() {
        let (Some(owner), Some(repo)) = (app.active_session(), app.repo_path.clone()) else {
            return tail.into_any_element();
        };
        let retry = ui.pr_list_error().is_some();
        if let Some(error) = ui.pr_list_error() {
            tail = tail.child(
                div()
                    .text_color(rgb(theme().color_blocker))
                    .when(automatic, |error| error.flex_1().min_w(px(0.)).truncate())
                    .when(!automatic, |error| error.whitespace_normal())
                    .child(super::render_helpers::safe_text(error)),
            );
        }
        let id = if !automatic {
            "pr-sidebar-load-more"
        } else if retry {
            "pr-main-page-retry"
        } else {
            "pr-filter-load-more"
        };
        let revision = ui.pr_list_revision();
        let cursor = ui.pr_list_paging().cursor.clone();
        let click_repo = repo.clone();
        tail = tail.child(super::list_filter_strip::page_button(
            id,
            if retry {
                Msg::IssuesRetryLoadMore.t()
            } else {
                Msg::ListLoadMore.t()
            },
            cx,
            move |app, _, _, cx| {
                if app.active_session() == Some(owner) && app.ui().pr_list_revision() == revision {
                    app.load_more_github_prs_for(owner, click_repo.clone(), cx);
                }
            },
        ));
        if automatic && !retry && !ui.pr_client_membership_active() && !ui.pr_list_rows().is_empty()
        {
            let entity = cx.entity().downgrade();
            tail = tail.child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        if bounds.intersects(&window.content_mask().bounds) {
                            let entity = entity.clone();
                            let repo = repo.clone();
                            let cursor = cursor.clone();
                            cx.defer(move |cx| {
                                let _ = entity.update(cx, |app, cx| {
                                    if app.active_session() == Some(owner)
                                        && app.ui().pr_list_revision() == revision
                                        && app.ui().pr_list_paging().cursor == cursor
                                        && app.ui().pr_list_error().is_none()
                                        && !app.ui().pr_client_membership_active()
                                        && app.pr_mode().is_some_and(|mode| mode.active.is_none())
                                        && app.workspace_mode()
                                            == super::workspace_mode::WorkspaceMode::Prs
                                    {
                                        app.load_more_github_prs_for(owner, repo, cx);
                                    }
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
    }
    tail.into_any_element()
}
