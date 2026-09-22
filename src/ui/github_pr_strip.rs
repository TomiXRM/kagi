//! Closed/All PR reads belong to the workspace, never to shared open evidence.
use gpui::Context;
use kagi_domain::{github::PullRequest, list_filter::StateFilter};

use super::{tab_view::TabUiState, KagiApp};

#[derive(Default)]
pub(super) struct PrStripRead {
    pub rows: Option<Vec<PullRequest>>,
    pub generation: u64,
    pub loading: bool,
    pub error: Option<String>,
}

impl TabUiState {
    pub(super) fn pr_list_rows(&self) -> &[PullRequest] {
        self.github_prs_strip
            .rows
            .as_deref()
            .unwrap_or(&self.github_prs)
    }

    pub(super) fn pr_list_revision(&self) -> (u64, u64) {
        (self.github_prs_gen, self.github_prs_strip.generation)
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

    pub(super) fn reset_pr_strip(&mut self) {
        self.github_prs_strip.generation = self.github_prs_strip.generation.wrapping_add(1);
        self.github_prs_strip.rows = None;
        self.github_prs_strip.loading = false;
        self.github_prs_strip.error = None;
        self.github_pr_filter.common.state = StateFilter::Open;
    }

    pub(super) fn leave_pr_mode(&mut self) {
        self.pr_mode = None;
        self.filter_controls.menu = None;
        self.reset_pr_strip();
    }
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
        let injected: Option<
            gpui::Task<Result<Vec<PullRequest>, kagi_git::github::PrFetchError>>,
        > = None;
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        if injected.is_none() && !kagi_git::github::gh_available() {
            ui.github_prs_strip.error = Some(super::i18n::Msg::PrGithubUnavailable.t().into());
            cx.notify();
            return;
        }
        ui.github_prs_strip.generation = ui.github_prs_strip.generation.wrapping_add(1);
        let generation = ui.github_prs_strip.generation;
        ui.github_prs_strip.loading = true;
        ui.github_prs_strip.error = None;
        cx.notify();
        cx.spawn(async move |this, acx| {
            let fetch_repo = repo.clone();
            let result = match injected {
                Some(task) => task.await,
                None => {
                    acx.background_executor()
                        .spawn(async move { kagi_git::github::list_prs(&fetch_repo, state) })
                        .await
                }
            };
            let _ = this.update(acx, |app, cx| {
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if ui.github_prs_strip.generation != generation
                    || ui.github_pr_filter.common.state != state
                {
                    return;
                }
                ui.github_prs_strip.loading = false;
                let Some(rows) = ui.github_prs_strip.rows.as_mut() else {
                    return;
                };
                let outcome = kagi_git::github::apply_pr_fetch(rows, result);
                ui.github_prs_strip.error =
                    outcome.error.as_ref().map(super::github::fetch_error_text);
                let moved = if outcome.error.is_none() {
                    for row in rows {
                        let cached = ui
                            .github_prs
                            .iter()
                            .chain(
                                ui.pr_mode
                                    .iter()
                                    .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
                            )
                            .find(|cached| {
                                cached.number == row.number
                                    && cached.base_repo == row.base_repo
                                    && cached.head_sha == row.head_sha
                            });
                        if let Some(cached) = cached {
                            kagi_domain::github::inherit_pr_details(row, cached);
                        }
                    }
                    ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
                    super::github_pr_detail::sync_open_pr_tabs(ui)
                } else {
                    Vec::new()
                };
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
}
