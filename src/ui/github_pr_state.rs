//! Page acceptance extends the session-owned Open and Closed/All collections.
use std::collections::HashSet;

use kagi_domain::github::{inherit_pr_details, IssueState, PrListSnapshot, PullRequest};
use kagi_domain::list_filter::{ChecksFilter, DraftFilter, StateFilter};
use kagi_git::github::{apply_pr_fetch, PrFetchError, PrFetchOutcome};

use super::github_pr_strip::{PrPageRequest, PrPageState};
use super::tab_view::TabUiState;

fn validate_page(
    snapshot: &PrListSnapshot,
    state: StateFilter,
    base_repo: Option<&str>,
    cursor: Option<&str>,
) -> Result<(), PrFetchError> {
    if snapshot.base_repo.trim().is_empty()
        || base_repo.is_some_and(|repo| repo != snapshot.base_repo)
        || snapshot.prs.iter().any(|pr| {
            pr.base_repo != snapshot.base_repo
                || match state {
                    StateFilter::Open => pr.state != IssueState::Open,
                    StateFilter::Closed => pr.state != IssueState::Closed,
                    StateFilter::All => false,
                }
        })
    {
        return Err(PrFetchError::Invalid(
            "pull request page repository/state mismatch".into(),
        ));
    }
    if snapshot.prs.is_empty() && snapshot.next_cursor.is_some() {
        return Err(PrFetchError::Invalid(
            "empty unfinished pull request page".into(),
        ));
    }
    if snapshot
        .next_cursor
        .as_deref()
        .is_some_and(|next| next.trim().is_empty() || Some(next) == cursor)
    {
        return Err(PrFetchError::Invalid(
            "pull request page cursor did not advance".into(),
        ));
    }
    Ok(())
}

impl PrPageState {
    pub(super) fn invalidate(&mut self) {
        self.cursor = None;
        self.append = None;
    }
}

impl TabUiState {
    pub(super) fn begin_github_prs_request(&mut self) -> u64 {
        self.github_prs_gen = self.github_prs_gen.wrapping_add(1);
        self.github_prs_loading = true;
        self.github_prs_paging.invalidate();
        if let Some(mode) = &self.pr_mode {
            if self.github_prs_strip.rows.is_none() {
                mode.dashboard_scroll
                    .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
            }
        }
        self.github_prs_gen
    }

    pub(super) fn finish_github_prs_request(
        &mut self,
        generation: u64,
        result: Result<PrListSnapshot, PrFetchError>,
    ) -> Option<PrFetchOutcome> {
        if generation != self.github_prs_gen || !self.github_prs_loading {
            return None;
        }
        self.github_prs_loading = false;
        let result = result.and_then(|mut snapshot| {
            validate_page(&snapshot, StateFilter::Open, None, None)?;
            for row in &mut snapshot.prs {
                if let Some(previous) = self
                    .pr_mode
                    .iter()
                    .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr))
                    .chain(self.github_prs_strip.rows.iter().flatten())
                    .find(|previous| {
                        previous.number == row.number
                            && previous.base_repo == row.base_repo
                            && previous.head_sha == row.head_sha
                    })
                {
                    inherit_pr_details(row, previous);
                }
            }
            self.github_prs_paging.base_repo = Some(snapshot.base_repo);
            self.github_prs_paging.cursor = snapshot.next_cursor;
            self.github_prs_paging.page = 1;
            Ok(snapshot.prs)
        });
        let outcome = apply_pr_fetch(&mut self.github_prs, result);
        if outcome.error.is_none()
            || outcome
                .error
                .as_ref()
                .is_some_and(PrFetchError::is_unavailable)
        {
            self.github_prs_loaded = true;
            self.github_prs_epoch = self.github_prs_epoch.wrapping_add(1);
        }
        Some(outcome)
    }

    pub(super) fn begin_pr_strip_request(&mut self, state: StateFilter) -> u64 {
        let strip = &mut self.github_prs_strip;
        strip.generation = strip.generation.wrapping_add(1);
        strip.request_state = state;
        strip.loading = true;
        strip.error = None;
        strip.paging.invalidate();
        strip.rows.get_or_insert_with(Vec::new);
        if let Some(mode) = &self.pr_mode {
            mode.dashboard_scroll
                .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        }
        strip.generation
    }

    pub(super) fn finish_pr_strip_request(
        &mut self,
        generation: u64,
        state: StateFilter,
        result: Result<PrListSnapshot, PrFetchError>,
    ) -> Option<PrFetchOutcome> {
        let strip = &self.github_prs_strip;
        if strip.generation != generation
            || !strip.loading
            || strip.rows.is_none()
            || strip.request_state != state
            || self.github_pr_filter.common.state != state
        {
            return None;
        }
        self.github_prs_strip.loading = false;
        let result = result.and_then(|mut snapshot| {
            validate_page(&snapshot, state, None, None)?;
            for row in &mut snapshot.prs {
                if let Some(cached) = self
                    .github_prs
                    .iter()
                    .chain(
                        self.pr_mode
                            .iter()
                            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
                    )
                    .find(|cached| {
                        cached.number == row.number
                            && cached.base_repo == row.base_repo
                            && cached.head_sha == row.head_sha
                    })
                {
                    inherit_pr_details(row, cached);
                }
            }
            self.github_prs_strip.paging.base_repo = Some(snapshot.base_repo);
            self.github_prs_strip.paging.cursor = snapshot.next_cursor;
            self.github_prs_strip.paging.page = 1;
            Ok(snapshot.prs)
        });
        let rows = self.github_prs_strip.rows.as_mut()?;
        let outcome = apply_pr_fetch(rows, result);
        self.github_prs_strip.error = outcome.error.as_ref().map(super::github::fetch_error_text);
        if outcome.error.is_none() {
            self.github_prs_epoch = self.github_prs_epoch.wrapping_add(1);
        }
        Some(outcome)
    }

    pub(super) fn begin_pr_page_request(&mut self) -> Option<PrPageRequest> {
        let state = self.github_pr_filter.common.state;
        let (generation, settled, paging) = if state == StateFilter::Open {
            (
                self.github_prs_gen,
                self.github_prs_loaded && !self.github_prs_loading,
                &mut self.github_prs_paging,
            )
        } else {
            let strip = &mut self.github_prs_strip;
            (
                strip.generation,
                strip.rows.is_some() && !strip.loading && strip.request_state == state,
                &mut strip.paging,
            )
        };
        if !settled || paging.append.is_some() || self.pr_mode.is_none() {
            return None;
        }
        paging.attempt = paging.attempt.wrapping_add(1);
        let request = PrPageRequest {
            generation,
            attempt: paging.attempt,
            visit: self.github_prs_visit,
            state,
            base_repo: paging.base_repo.clone()?,
            cursor: paging.cursor.clone()?,
        };
        paging.append = Some(request.clone());
        if state == StateFilter::Open {
            self.github_error = None;
        } else {
            self.github_prs_strip.error = None;
        }
        Some(request)
    }

    pub(super) fn finish_pr_page_request(
        &mut self,
        request: &PrPageRequest,
        result: Result<PrListSnapshot, PrFetchError>,
    ) -> bool {
        let generation = if request.state == StateFilter::Open {
            self.github_prs_gen
        } else {
            self.github_prs_strip.generation
        };
        if self.github_prs_visit != request.visit
            || self.pr_mode.is_none()
            || self.github_pr_filter.common.state != request.state
            || generation != request.generation
            || self.pr_list_paging().append.as_ref() != Some(request)
            || self.pr_list_paging().cursor.as_deref() != Some(request.cursor.as_str())
            || self.pr_list_paging().base_repo.as_deref() != Some(request.base_repo.as_str())
        {
            return false;
        }
        let result = result.and_then(|snapshot| {
            validate_page(
                &snapshot,
                request.state,
                Some(&request.base_repo),
                Some(&request.cursor),
            )?;
            Ok(snapshot)
        });
        let error = match result {
            Err(error) => Some(super::github::fetch_error_text(&error)),
            Ok(mut snapshot) => {
                for row in &mut snapshot.prs {
                    if let Some(cached) = self
                        .github_prs
                        .iter()
                        .chain(self.github_prs_strip.rows.iter().flatten())
                        .chain(
                            self.pr_mode
                                .iter()
                                .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
                        )
                        .find(|cached| {
                            cached.number == row.number
                                && cached.base_repo == row.base_repo
                                && cached.head_sha == row.head_sha
                        })
                    {
                        inherit_pr_details(row, cached);
                    }
                }
                let rows = if request.state == StateFilter::Open {
                    &mut self.github_prs
                } else {
                    // The slot above proves the strip collection still exists.
                    self.github_prs_strip
                        .rows
                        .as_mut()
                        .expect("accepted strip page")
                };
                let mut known: HashSet<_> = rows.iter().map(PullRequest::key).collect();
                rows.reserve(snapshot.prs.len());
                rows.extend(snapshot.prs.into_iter().filter(|pr| known.insert(pr.key())));
                let paging = self.pr_list_paging_mut();
                paging.cursor = snapshot.next_cursor;
                paging.page += 1;
                self.github_prs_epoch = self.github_prs_epoch.wrapping_add(1);
                klog!(
                    "github: prs page={} loaded={} has_more={}",
                    self.pr_list_paging().page,
                    self.pr_list_rows().len(),
                    self.pr_list_has_more()
                );
                None
            }
        };
        self.pr_list_paging_mut().append = None;
        if request.state == StateFilter::Open {
            self.github_error = error;
        } else {
            self.github_prs_strip.error = error;
        }
        true
    }

    pub(super) fn pr_client_membership_active(&self) -> bool {
        let filter = &self.github_pr_filter;
        !filter.common.labels.is_empty()
            || filter.common.author.is_some()
            || !filter.common.text.trim().is_empty()
            || filter.draft != DraftFilter::All
            || filter.checks != ChecksFilter::All
    }
}

#[cfg(test)]
#[path = "github_pr_state_tests.rs"]
mod tests;
