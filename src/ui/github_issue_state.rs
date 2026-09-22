//! Issue-workspace state transitions on [`TabUiState`]: the first-page
//! refresh, the cursor-paginated append (#752), and the detail read.
//!
//! Split out of `tab_view.rs` (at the 800 LOC ceiling) on the boundary that
//! already existed inside it: every transition here is keyed by one of the two
//! Issue request generations, and none of them consults the active tab.
//! Visibility is unchanged — this is a *sibling* of `tab_view`, so `pub(super)`
//! still means "the `ui` module" and the existing call sites in
//! `github_issues.rs` keep compiling.
//!
//! The pagination contract in one place, because every guard below only makes
//! sense against it:
//!
//! * `github_issues_cursor` is the `next_cursor` of the **last accepted**
//!   response. `None` means "no further page" — either the final page landed or
//!   the list is being refreshed.
//! * `github_issues_loading_more` is the single append slot. A refresh empties
//!   it; one append at a time fills it.
//! * A refresh bumps `github_issues_gen`, which is what makes an in-flight
//!   append's completion unacceptable: the rows it would extend are gone.
//! * An append failure keeps the rows *and* the cursor, so a retry resumes from
//!   the same position. Nothing here retries by itself; clearing the error is
//!   the caller's gate for the next attempt.

use super::tab_view::TabUiState;
use kagi_domain::github::IssueListSnapshot;
use kagi_git::github::PrFetchError;
use std::collections::HashSet;

impl TabUiState {
    /// Begin a first-page list request and return the generation frozen into
    /// its task.
    ///
    /// A refresh restarts pagination: the cursor and the append slot describe
    /// the list this request is about to replace, so both are dropped here
    /// rather than at the call site.
    pub(super) fn begin_github_issues_request(&mut self) -> u64 {
        self.github_issues_gen = self.github_issues_gen.wrapping_add(1);
        self.github_issues_loading = true;
        self.github_issues_loading_more = false;
        self.github_issues_cursor = None;
        self.github_issues_error = None;
        self.github_issues_list.reset(0);
        if self.issue_composer.base_repo.is_none() {
            self.issue_composer.repo_loading = true;
            self.issue_composer.repo_error = None;
        }
        self.github_issues_gen
    }

    /// Settle a list request only when no later request superseded it.
    ///
    /// Failure records its reason but deliberately retains the last successful
    /// list. `Ok(vec![])` is the only empty answer that replaces it. A failed
    /// refresh therefore shows stale rows with no cursor: resuming pagination
    /// from a window that was not re-read would interleave two different lists,
    /// and the retry that restores the rows restores the cursor with them.
    pub(super) fn finish_github_issues_request(
        &mut self,
        generation: u64,
        result: Result<IssueListSnapshot, PrFetchError>,
    ) -> bool {
        if generation != self.github_issues_gen {
            return false;
        }
        self.github_issues_loading = false;
        match result {
            Ok(snapshot) => {
                self.github_issues_list.reset(0);
                self.github_issues = snapshot.issues;
                self.github_issue_mentions = snapshot.mentioned_numbers;
                self.github_issues_cursor = snapshot.next_cursor;
                self.issue_composer.base_repo = Some(snapshot.base_repo);
                self.issue_composer.repo_loading = false;
                self.github_issues_page = 1;
                self.issue_composer.repo_error = None;
                klog!(
                    "github: issues page={} loaded={} has_more={}",
                    self.github_issues_page,
                    self.github_issues.len(),
                    self.github_issues_cursor.is_some()
                );
                self.github_issues_loaded = true;
                self.github_issues_error = None;
            }
            Err(error) => {
                let error = error.to_string();
                self.github_issues_error = Some(error.clone());
                self.issue_composer.repo_loading = false;
                if self.issue_composer.base_repo.is_none() {
                    self.issue_composer.repo_error = Some(error);
                }
            }
        }
        true
    }

    /// Claim the next page of the list this session already holds, returning
    /// `(generation, cursor, frozen base repository)` for the task.
    ///
    /// `None` refuses the request instead of queueing it. There is nothing to
    /// append to while the first page is loading or has never landed; the
    /// append slot admits one request; and a page needs both the cursor the
    /// last accepted response returned and the write destination frozen with
    /// it. The generation is *not* bumped — an append extends the list that
    /// generation identifies rather than replacing it, and reusing it is what
    /// lets the next refresh invalidate this completion.
    ///
    /// Clearing the error is what makes a failed append retryable: the caller
    /// gates viewport-driven loads on `github_issues_error`, so the flag must
    /// not survive the attempt it triggered.
    pub(super) fn begin_github_issues_page_request(&mut self) -> Option<(u64, String, String)> {
        if self.github_issues_loading
            || self.github_issues_loading_more
            || !self.github_issues_loaded
        {
            return None;
        }
        let cursor = self.github_issues_cursor.clone()?;
        let base_repo = self.issue_composer.base_repo.clone()?;
        self.github_issues_loading_more = true;
        self.github_issues_error = None;
        Some((self.github_issues_gen, cursor, base_repo))
    }

    /// Merge one page into the list it was requested from.
    ///
    /// Refused unless the same generation is still current, the append slot is
    /// still held, and `cursor` is still the position being fetched — which is
    /// what makes a duplicated or replayed delivery a no-op instead of a second
    /// append of the same rows.
    ///
    /// Accepted rows extend the list by *new* issue numbers only, keeping the
    /// order and the values already on screen, and mention membership is a
    /// union so a page that mentions nothing cannot un-mention page 1. A
    /// failure keeps both the rows and the cursor, so the retry resumes here.
    pub(super) fn finish_github_issues_page_request(
        &mut self,
        generation: u64,
        cursor: &str,
        result: Result<IssueListSnapshot, PrFetchError>,
    ) -> bool {
        if generation != self.github_issues_gen
            || !self.github_issues_loading_more
            || self.github_issues_cursor.as_deref() != Some(cursor)
        {
            return false;
        }
        self.github_issues_loading_more = false;
        match result {
            Ok(snapshot) => self.merge_github_issues_page(snapshot),
            // The rows and the cursor stay: this is the same position, not a
            // shorter list.
            Err(error) => self.github_issues_error = Some(error.to_string()),
        }
        true
    }

    /// Append the rows of an accepted page, or refuse a page that describes
    /// another repository.
    ///
    /// The page must describe the repository frozen with the list. A mismatched
    /// response is an error, not evidence that the current cursor is exhausted.
    fn merge_github_issues_page(&mut self, snapshot: IssueListSnapshot) {
        if self.issue_composer.base_repo.as_deref() != Some(snapshot.base_repo.as_str()) {
            self.github_issues_error = Some(
                PrFetchError::Invalid(format!(
                    "issue page came from {} instead of {}",
                    snapshot.base_repo,
                    self.issue_composer.base_repo.as_deref().unwrap_or("?")
                ))
                .to_string(),
            );
            return;
        }
        let mut known: HashSet<u64> = HashSet::with_capacity(self.github_issues.len());
        known.extend(self.github_issues.iter().map(|issue| issue.number));
        self.github_issues.reserve(snapshot.issues.len());
        for issue in snapshot.issues {
            if known.insert(issue.number) {
                self.github_issues.push(issue);
            }
        }
        for number in snapshot.mentioned_numbers {
            if !self.github_issue_mentions.contains(&number) {
                self.github_issue_mentions.push(number);
            }
        }
        self.github_issues_cursor = snapshot.next_cursor;
        self.github_issues_page += 1;
        self.github_issues_error = None;
        klog!(
            "github: issues page={} loaded={} has_more={}",
            self.github_issues_page,
            self.github_issues.len(),
            self.github_issues_cursor.is_some()
        );
    }

    /// Select an issue and begin a detail request for it.
    pub(super) fn begin_github_issue_detail_request(&mut self, number: u64) -> u64 {
        self.selected_github_issue = Some(number);
        self.github_issue_detail_gen = self.github_issue_detail_gen.wrapping_add(1);
        self.github_issue_detail_loading = Some(number);
        self.github_issue_detail_error = None;
        self.github_issue_detail_gen
    }

    /// Return to the Composer-only home without discarding a cached Thread or
    /// Reply draft. Advancing the generation prevents an in-flight detail read
    /// from reviving the selection after the user left it.
    pub(super) fn clear_github_issue_selection(&mut self) {
        self.github_issue_detail_gen = self.github_issue_detail_gen.wrapping_add(1);
        self.selected_github_issue = None;
        self.github_issue_detail_loading = None;
        self.github_issue_detail_error = None;
        for editor in self.issue_composer.editors.values_mut() {
            editor.focused = false;
        }
    }

    /// Settle only the newest detail request. A failure leaves any successfully
    /// cached detail intact, including the cached value for the same number.
    pub(super) fn finish_github_issue_detail_request(
        &mut self,
        generation: u64,
        number: u64,
        result: Result<kagi_domain::github::Issue, PrFetchError>,
    ) -> bool {
        if generation != self.github_issue_detail_gen
            || self.github_issue_detail_loading != Some(number)
        {
            return false;
        }
        self.github_issue_detail_loading = None;
        match result {
            Ok(issue) => {
                self.github_issue_details.insert(number, issue);
                self.github_issue_detail_error = None;
            }
            Err(error) => self.github_issue_detail_error = Some(error.to_string()),
        }
        true
    }
}

/// The transitions above are the whole Issue request contract, so their
/// regression tests are the bulk of this feature; they live in their own file
/// to keep both halves inside the LOC budget.
#[cfg(test)]
#[path = "github_issue_state_tests.rs"]
mod tests;
