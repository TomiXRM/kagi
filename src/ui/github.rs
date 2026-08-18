//! GitHub Phase 1 — sidebar PULL REQUESTS section plumbing.
//!
//! The data comes from `kagi_git::github` (a `gh pr list` shell-out); this
//! module owns the periodic refresh and the row actions. Read-only end to end.

use std::time::Duration;

use gpui::{Context, SharedString};
use kagi_domain::github::PullRequest;

use super::i18n::Msg;
use super::types::ToastKind;
use super::KagiApp;

/// Refresh cadence. `gh pr list` is one API call; a minute keeps CI status
/// fresh without hammering the rate limit.
const GITHUB_REFRESH_SECS: u64 = 60;

impl KagiApp {
    /// Lazily spawn the PR refresh ticker (called from `render`, like the
    /// auto-fetch ticker). Runs one refresh immediately, then every
    /// `GITHUB_REFRESH_SECS`. Exits when the repo closes. No-op without `gh`.
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
                let repo = match this.read_with(acx, |app, _| app.repo_path.clone()) {
                    Ok(Some(p)) => p,
                    _ => break,
                };
                let repo_for_task = repo.clone();
                let result = acx
                    .background_executor()
                    .spawn(async move { kagi_git::github::list_open_prs(&repo_for_task) })
                    .await;
                let keep = this.update(acx, |app, cx| {
                    // The tab may have switched while we were fetching.
                    if app.repo_path.as_ref() != Some(&repo) {
                        return true;
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
