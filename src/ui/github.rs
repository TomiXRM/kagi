//! GitHub Phase 1 — sidebar PULL REQUESTS section plumbing.
//!
//! The data comes from `kagi_git::github` (a `gh pr list` shell-out); this
//! module owns the periodic refresh and the row actions. Read-only end to end.

use std::time::Duration;

use gpui::{Context, SharedString};
use kagi_domain::github::PullRequest;

use super::i18n::{self, Msg};
use super::operations::RunPresentation;
use super::types::ToastKind;
use super::{CompareTarget, CompareView, FooterStatus, KagiApp, OpOutcome};

/// Refresh cadence. `gh pr list` is one API call; a minute keeps CI status
/// fresh without hammering the rate limit.
const GITHUB_REFRESH_SECS: u64 = 60;

/// A localized (EN/JA) sentence for a classified fetch failure, with `gh`'s own
/// wording kept as the detail (#506). The `[kagi]` log line keeps the raw,
/// English `Display` form — this is the human-facing half.
pub(super) fn fetch_error_text(e: &kagi_git::github::PrFetchError) -> String {
    use kagi_git::github::PrFetchError as E;
    let msg = match e {
        E::Unavailable(_) => Msg::PrGithubUnavailable,
        E::Auth(_) => Msg::PrFetchAuth,
        E::Network(_) => Msg::PrFetchNetwork,
        E::Invalid(_) => Msg::PrFetchInvalid,
        E::RateLimited(_) | E::NotFound(_) | E::Unknown(_) => Msg::PrFetchUnknown,
    };
    let detail = e.detail();
    if detail.is_empty() {
        msg.t().to_string()
    } else {
        format!("{}: {}", msg.t(), detail)
    }
}

impl KagiApp {
    /// One-shot `gh pr list` refresh for the current repo. Safe to call
    /// often: completion is routed to the session that started it, even when
    /// another tab is active. Called by the ticker and on every tab switch
    /// (`switch_repo`) so a new tab does not wait for the next 60s tick.
    pub fn refresh_github_prs(&mut self, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        #[cfg(feature = "gui-e2e")]
        let injected = super::e2e::take_github_pr_fetch();
        #[cfg(not(feature = "gui-e2e"))]
        let injected: Option<
            gpui::Task<
                Result<Vec<kagi_domain::github::PullRequest>, kagi_git::github::PrFetchError>,
            >,
        > = None;
        if injected.is_none() && !kagi_git::github::gh_available() {
            return;
        }
        cx.spawn(async move |this, acx| {
            let result = match injected {
                Some(task) => task.await,
                None => {
                    acx.background_executor()
                        .spawn(async move { kagi_git::github::list_open_prs(&repo) })
                        .await
                }
            };
            let _ = this.update(acx, |app, cx| {
                let owner_is_active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                // #506: only a fetch that actually answered may replace the
                // list. `apply_pr_fetch` holds that rule for both PR callers —
                // an expired token or an offline machine keeps the last good
                // data instead of being shown as an empty inbox.
                let outcome = kagi_git::github::apply_pr_fetch(&mut ui.github_prs, result);
                match &outcome.error {
                    None => {
                        ui.github_error = None;
                        ui.github_unavailable = false;
                    }
                    // No GitHub remote: a defined "nothing here" state, not a
                    // failure to report on every 60s tick.
                    Some(e) if e.is_unavailable() => {
                        ui.github_error = None;
                        ui.github_unavailable = true;
                    }
                    Some(e) => {
                        // Recorded, not toasted: this also runs on a 60s ticker,
                        // and a toast per tick would be its own bug. The PR home
                        // screen reads it so a failed fetch stops looking like
                        // "you have no pull requests" (user-visible lie when the
                        // token expires or the machine is offline).
                        let text = fetch_error_text(e);
                        // Once per *distinct* failure: this runs every 60s and
                        // a logged-out session would otherwise repeat the same
                        // contract line forever.
                        if ui.github_error.as_deref() != Some(text.as_str()) {
                            klog!("github: error: {}", e);
                        }
                        ui.github_error = Some(text);
                        ui.github_unavailable = false;
                        if owner_is_active {
                            cx.notify();
                        }
                        return;
                    }
                }
                if outcome.changed || !ui.github_prs_loaded {
                    klog!("github: prs={}", ui.github_prs.len());
                    ui.github_prs_loaded = true;
                    ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
                }
                if owner_is_active {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Refresh the read-only Issue list for the session that starts the
    /// request. A later request for that session supersedes this completion;
    /// switching tabs never redirects it to the active owner.
    pub fn refresh_github_issues(&mut self, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let generation = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            ui.begin_github_issues_request()
        };
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result = acx
                .background_executor()
                .spawn(async move { kagi_git::github::list_issues(&repo) })
                .await;
            let _ = this.update(acx, |app, cx| {
                let owner_is_active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if ui.finish_github_issues_request(generation, result) && owner_is_active {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Load the selected Issue's body and comments through the same
    /// session-owned background boundary as the list.
    pub fn load_github_issue_detail(&mut self, number: u64, cx: &mut Context<Self>) {
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let generation = {
            let Some(ui) = self.ui.get_mut(&owner) else {
                return;
            };
            ui.begin_github_issue_detail_request(number)
        };
        cx.notify();
        cx.spawn(async move |this, acx| {
            let result = acx
                .background_executor()
                .spawn(async move { kagi_git::github::issue_detail(&repo, number) })
                .await;
            let _ = this.update(acx, |app, cx| {
                let owner_is_active = app.active_session() == Some(owner);
                let Some(ui) = app.ui.get_mut(&owner) else {
                    return;
                };
                if ui.finish_github_issue_detail_request(generation, number, result)
                    && owner_is_active
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Lazily spawn the PR refresh ticker (called from `render`, like the
    /// auto-fetch ticker). Fetches the login once, refreshes immediately,
    /// then every `GITHUB_REFRESH_SECS`. Exits when the repo closes.
    pub fn ensure_github_ticker(&mut self, cx: &mut Context<Self>) {
        if self.github_ticker_alive || self.repo_path.is_none() {
            return;
        }
        // `gh_available()` spawns `gh --version` — 18-24ms of process fork on
        // the UI thread, and this runs from `ensure_startup_repo_io` before the
        // first frame. The check moved inside the task; the flag is still set
        // here so a second call cannot start a second ticker while the first is
        // still deciding.
        self.github_ticker_alive = true;
        cx.spawn(async move |this, acx| {
            let available = acx
                .background_executor()
                .spawn(async { kagi_git::github::gh_available() })
                .await;
            if !available {
                let _ = this.update(acx, |app, _| app.github_ticker_alive = false);
                return;
            }
            klog!("github: ticker start ({}s)", GITHUB_REFRESH_SECS);
            // Who am I? Once per ticker; the grouping is best-effort without it.
            let login = acx
                .background_executor()
                .spawn(async { kagi_git::github::current_login() })
                .await;
            let _ = this.update(acx, |app, cx| {
                app.github_login = login;
                // The Mine/Others split depends on this global capability.
                // Every attached session may own cached rows, including an
                // inactive one whose epoch happens to equal the active tab's.
                for ui in app.ui.values_mut() {
                    ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
                }
                cx.notify();
            });
            loop {
                let keep = this.update(acx, |app, cx| {
                    if app.repo_path.is_none() {
                        return false;
                    }
                    app.refresh_github_prs(cx);
                    true
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
                acx.background_executor()
                    .timer(Duration::from_secs(GITHUB_REFRESH_SECS))
                    .await;
            }
            let _ = this.update(acx, |app, _| app.github_ticker_alive = false);
        })
        .detach();
    }

    /// Sidebar PR row click: jump the graph to the PR's head branch (local
    /// branch first, then `origin/<head>`), the same jump the branch rows do.
    pub fn jump_to_pr_head(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        let local = self
            .view()
            .branches
            .iter()
            .any(|(name, _)| name == &pr.head);
        if local {
            self.jump_to_branch(&pr.head);
            return;
        }
        let remote = self
            .view()
            .remote_branches
            .iter()
            .find(|rb| rb.name == pr.head)
            .map(|rb| rb.target.clone());
        match remote {
            Some(target) => self.jump_to_commit(&target),
            None => self.push_toast(
                ToastKind::Info,
                SharedString::from(format!("{}: {}", Msg::PrBranchNotFetched.t(), pr.head)),
                cx,
            ),
        }
    }

    /// Read-only PR peek: Compare pane over merge-base(base, head) → head using
    /// the fetched remote tips. Both branches must exist as `origin/…`.
    pub fn open_pr_peek(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        let tip = |name: &str| {
            self.view()
                .remote_branches
                .iter()
                .find(|rb| rb.name == name)
                .map(|rb| rb.target.clone())
        };
        let (Some(base_tip), Some(head_tip)) = (tip(&pr.base), tip(&pr.head)) else {
            self.push_toast(
                ToastKind::Info,
                SharedString::from(format!(
                    "{}: {} / {}",
                    Msg::PrBranchNotFetched.t(),
                    pr.base,
                    pr.head
                )),
                cx,
            );
            return;
        };
        let Some(session) = self.ui().repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let base = repo.merge_base(&base_tip, &head_tip).unwrap_or(base_tip);
        match repo.compare_commits(&base, &head_tip) {
            Ok(files) => {
                klog!("pr-peek: #{} files={}", pr.number, files.len());
                if let Some(row) = self.row_for_commit_id(&head_tip) {
                    if self.ui().selected != Some(row) {
                        self.select(row);
                    }
                }
                if let Some(ui) = self.ui_mut() {
                    ui.main_diff = None;
                }
                let view = CompareView {
                    base,
                    target: CompareTarget::Commit(head_tip),
                    files,
                    title: SharedString::from(format!("#{} {}", pr.number, pr.head)),
                };
                self.show_compare(view, cx);
            }
            Err(e) => {
                klog!("pr-peek: error: {}", e);
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(i18n::op_failed(i18n::Op::PrPeek, e)));
            }
        }
    }

    pub fn open_pr_in_browser(&mut self, pr: &PullRequest) {
        klog!("github: open #{}", pr.number);
        let _ = std::process::Command::new("open").arg(&pr.url).spawn();
    }

    pub fn copy_pr_url(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(pr.url.clone()));
        self.push_toast(
            ToastKind::Info,
            SharedString::from(format!("#{} URL", pr.number)),
            cx,
        );
    }
}

// ────────────────────────────────────────────────────────────
// PR merge — plan → confirm → execute → oplog (GitHub Phase 2)
// ────────────────────────────────────────────────────────────

impl KagiApp {
    /// Build the merge plan and open the confirmation modal. Pure over the PR
    /// snapshot, so it opens instantly; `start_pr_merge` executes.
    pub fn open_pr_merge_modal(
        &mut self,
        pr: &kagi_domain::github::PullRequest,
        method: kagi_git::github::MergeMethod,
        delete_branch: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(owner) = self.repo_path.clone() {
            if self.reject_transport_hold(&owner, &format!("pr-merge #{}", pr.number)) {
                cx.notify();
                return;
            }
        }
        let head_summary = self.view().status_summary.branch.clone();
        let plan = kagi_git::github::plan_pr_merge(pr, method, delete_branch, head_summary);
        klog!(
            "plan: pr-merge #{} blockers={} warnings={}",
            pr.number,
            plan.blockers.len(),
            plan.warnings.len()
        );
        self.set_pr_merge_modal(crate::ui::modals::PrMergeModal {
            plan: std::sync::Arc::new(plan),
            error: None,
            number: pr.number,
            head_sha: pr.head_sha.clone(),
            method,
            delete_branch,
        });
        cx.notify();
    }

    pub fn cancel_pr_merge_modal(&mut self) {
        self.clear_pr_merge_modal();
    }

    /// Execute the merge through `gh pr merge`, then refresh the PR list and
    /// record the outcome in the oplog.
    pub fn start_pr_merge(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.pr_merge_modal().cloned() else {
            return;
        };
        let plan = modal.plan.clone();
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if self.reject_transport_hold(&repo_path, &format!("pr-merge #{}", modal.number)) {
            self.clear_pr_merge_modal();
            self.present_app_notice();
            cx.notify();
            return;
        }
        // Concurrency gate (#402, bug class #283–#289): a PR merge is a write
        // op like any other, so it must take the single in-flight latch and
        // route completion through the stale-tab-aware shell.
        if self.reject_if_busy(cx) {
            return;
        }
        // Defence in depth: never execute a blocked plan (the button is
        // hidden too, but the Enter-to-confirm path shares this method).
        if !plan.blockers.is_empty() {
            klog!("refused: pr-merge plan has blockers, not executing");
            // A blocked plan never reaches the transport, so the UI stays the
            // recorder for Refused — `record_op` persists that outcome (#501).
            self.record_op(
                "pr-merge",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_pr_merge_modal();
            return;
        }
        let (number, method, delete_branch) = (modal.number, modal.method, modal.delete_branch);
        let head_sha = modal.head_sha.clone();
        // ADR-0196 Wave 3: a PR merge is a write, so it rides the run family
        // like every migrated one — `finish_run` admits it (lease + owner
        // stamp) and hands the completion to `apply`, which releases the lease
        // on a known termination and, when the receipt is `Unknown`, keeps it
        // and parks a reconcile entry. The confirmation is only discarded once
        // that admission succeeded (it can refuse: Identity / NeedsReconcile).
        let rp = repo_path.clone();
        let bg_plan = plan.clone();
        let dispatched = self.finish_run(
            cx,
            "pr-merge",
            i18n::Op::Merge,
            plan.clone(),
            repo_path,
            move || {
                #[cfg(feature = "gui-e2e")]
                if let Some(report) = crate::ui::e2e::pr_merge_terminal_fault(&rp, number, &bg_plan)
                {
                    return Ok(report);
                }
                Ok(kagi_git::github::merge_pr(
                    &rp,
                    number,
                    method,
                    delete_branch,
                    &head_sha,
                    &bg_plan,
                ))
            },
            |_| None,
            move |done| match done {
                // The recorded outcome decided what happened, not the raw `gh`
                // exit. Indeterminate outcomes are already settled by the
                // transport hold or reconcile entry.
                Ok(kagi_git::OperationOutcome::PrMerge {
                    detail, confirmed, ..
                }) => {
                    klog!("executed: pr-merge #{}", number);
                    if *confirmed {
                        RunPresentation::none().github_merge(number, detail.clone())
                    } else {
                        RunPresentation::none()
                    }
                }
                Ok(_) => RunPresentation::none(),
                Err(failure) => {
                    klog!("pr-merge failed: {}", failure.message);
                    RunPresentation::none()
                }
            },
        );
        if dispatched {
            self.clear_pr_merge_modal();
            self.status_footer = FooterStatus::Busy(SharedString::from(format!(
                "{} #{}…",
                Msg::PrModeMerge.t(),
                number
            )));
            cx.notify();
        }
    }

    /// Post the composer's text as a comment on the open PR (ADR-0200).
    ///
    /// The explicit click is the approval - there is no confirmation modal, for
    /// the same reason staging has none. Everything else is the write family:
    /// the transport hold and the in-flight latch gate it, `finish_run` admits
    /// it with an owner stamp, and `kagi_git::github::pr_comment` records the
    /// attempt at its own transport boundary, so the receipt exists even if the
    /// reader has switched tabs by the time `gh` answers.
    pub fn start_pr_comment(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let Some((number, body)) = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
            .map(|t| (t.pr.number, t.comment_draft.clone()))
        else {
            return;
        };
        if body.trim().is_empty() {
            return;
        }
        let Some(pr) = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
            .map(|t| t.pr.clone())
        else {
            return;
        };
        if self.reject_transport_hold(&repo_path, &format!("pr-comment #{number}")) {
            self.present_app_notice();
            cx.notify();
            return;
        }
        if self.reject_if_busy(cx) {
            return;
        }
        let plan = std::sync::Arc::new(kagi_git::github::plan_pr_comment(&pr, &body));
        if !plan.blockers.is_empty() {
            klog!("refused: pr-comment plan has blockers, not executing");
            self.record_op(
                "pr-comment",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        let rp = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_body = body.clone();
        let dispatched = self.finish_run(
            cx,
            "pr-comment",
            i18n::Op::PrComment,
            plan.clone(),
            repo_path,
            move || {
                Ok(kagi_git::github::pr_comment(
                    &rp, number, &bg_body, &bg_plan,
                ))
            },
            |_| None,
            move |done| match done {
                Ok(kagi_git::OperationOutcome::PrComment { .. }) => {
                    klog!("executed: pr-comment #{}", number);
                    RunPresentation::none().pr_comment(number)
                }
                Ok(_) => RunPresentation::none(),
                Err(failure) => {
                    klog!("pr-comment failed: {}", failure.message);
                    RunPresentation::none()
                }
            },
        );
        if dispatched {
            self.status_footer = FooterStatus::Busy(SharedString::from(format!(
                "{} #{}…",
                Msg::PrCommentPost.t(),
                number
            )));
            cx.notify();
        }
    }

    /// Submit a review on the open PR (ADR-0200, mock 7a): APPROVE or REQUEST
    /// CHANGES, with the composer's text as the review body.
    ///
    /// Same shape as [`Self::start_pr_comment`] - the click is the approval,
    /// the transport hold and the write latch gate it, and
    /// `kagi_git::github::pr_review` records the attempt at its own transport
    /// boundary. GitHub requires words on a "request changes" review; that is
    /// a plan blocker, not a silent no-op.
    pub fn start_pr_review(
        &mut self,
        verdict: kagi_domain::github::ReviewVerdict,
        cx: &mut Context<Self>,
    ) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let Some((pr, body)) = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
            .map(|t| (t.pr.clone(), t.comment_draft.clone()))
        else {
            return;
        };
        let number = pr.number;
        if self.reject_transport_hold(&repo_path, &format!("pr-review #{number}")) {
            self.present_app_notice();
            cx.notify();
            return;
        }
        if self.reject_if_busy(cx) {
            return;
        }
        let plan = std::sync::Arc::new(kagi_git::github::plan_pr_review(&pr, verdict, &body));
        if !plan.blockers.is_empty() {
            klog!("refused: pr-review plan has blockers, not executing");
            self.record_op(
                "pr-review",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        let rp = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_body = body.clone();
        let dispatched = self.finish_run(
            cx,
            "pr-review",
            i18n::Op::PrReview,
            plan.clone(),
            repo_path,
            move || {
                Ok(kagi_git::github::pr_review(
                    &rp, number, verdict, &bg_body, &bg_plan,
                ))
            },
            |_| None,
            move |done| match done {
                Ok(kagi_git::OperationOutcome::PrReview { .. }) => {
                    klog!("executed: pr-review #{}", number);
                    // A review carries the same text the composer held, so it
                    // clears and the thread re-reads exactly like a comment.
                    RunPresentation::none().pr_comment(number)
                }
                Ok(_) => RunPresentation::none(),
                Err(failure) => {
                    klog!("pr-review failed: {}", failure.message);
                    RunPresentation::none()
                }
            },
        );
        if dispatched {
            self.status_footer = FooterStatus::Busy(SharedString::from(format!(
                "{} #{}\u{2026}",
                Msg::PrReviewSubmit.t(),
                number
            )));
            cx.notify();
        }
    }
}
