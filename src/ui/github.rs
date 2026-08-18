//! GitHub Phase 1 — sidebar PULL REQUESTS section plumbing.
//!
//! The data comes from `kagi_git::github` (a `gh pr list` shell-out); this
//! module owns the periodic refresh and the row actions. Read-only end to end.

use std::time::Duration;

use gpui::{Context, SharedString};
use kagi_domain::github::PullRequest;

use super::i18n::Msg;
use super::types::ToastKind;
use super::{CompareTarget, CompareView, FooterStatus, KagiApp};

/// Refresh cadence. `gh pr list` is one API call; a minute keeps CI status
/// fresh without hammering the rate limit.
const GITHUB_REFRESH_SECS: u64 = 60;

impl KagiApp {
    /// One-shot `gh pr list` refresh for the current repo. Safe to call
    /// often: results are stamped with the repo they were fetched for and
    /// dropped if the tab switched mid-flight. Called by the ticker and on
    /// every tab switch (`switch_repo`) — without the latter, a switch left
    /// the new tab at 0 PRs until the ticker's next 60s tick (user report).
    pub fn refresh_github_prs(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = self.repo_path.clone() else {
            return;
        };
        if !kagi_git::github::gh_available() {
            return;
        }
        cx.spawn(async move |this, acx| {
            let repo_for_task = repo.clone();
            let result = acx
                .background_executor()
                .spawn(async move { kagi_git::github::list_open_prs(&repo_for_task) })
                .await;
            let _ = this.update(acx, |app, cx| {
                // The tab may have switched while we were fetching.
                if app.repo_path.as_ref() != Some(&repo) {
                    return;
                }
                match result {
                    Ok(prs) => {
                        if app.github_prs != prs || app.github_prs_for.as_ref() != Some(&repo) {
                            klog!("github: prs={}", prs.len());
                            app.github_prs = prs;
                            app.github_prs_for = Some(repo.clone());
                            app.github_prs_epoch = app.github_prs_epoch.wrapping_add(1);
                            cx.notify();
                        }
                    }
                    Err(e) => klog!("github: error: {}", e),
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
        if !kagi_git::github::gh_available() {
            return;
        }
        self.github_ticker_alive = true;
        klog!("github: ticker start ({}s)", GITHUB_REFRESH_SECS);
        cx.spawn(async move |this, acx| {
            // Who am I? Once per ticker; the grouping is best-effort without it.
            let login = acx
                .background_executor()
                .spawn(async { kagi_git::github::current_login() })
                .await;
            let _ = this.update(acx, |app, cx| {
                app.github_login = login;
                // The Mine/Others split depends on it — rebuild the rows.
                app.github_prs_epoch = app.github_prs_epoch.wrapping_add(1);
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
            .active_view
            .branches
            .iter()
            .any(|(name, _)| name == &pr.head);
        if local {
            self.jump_to_branch(&pr.head);
            return;
        }
        let remote = self
            .active_view
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
            self.active_view
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
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let base = repo.merge_base(&base_tip, &head_tip).unwrap_or(base_tip);
        match repo.compare_commits(&base, &head_tip) {
            Ok(files) => {
                klog!("pr-peek: #{} files={}", pr.number, files.len());
                if let Some(row) = self.row_for_commit_id(&head_tip) {
                    if self.selected != Some(row) {
                        self.select(row);
                    }
                }
                self.main_diff = None;
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
                    FooterStatus::Failed(SharedString::from(format!("PR peek failed: {}", e)));
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
