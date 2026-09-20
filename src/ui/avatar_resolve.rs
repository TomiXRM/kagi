//! Avatar resolution wiring on `KagiApp` (ADR-0123) — the UI half that feeds
//! [`super::avatar_fetch`]: incremental email collection from the loaded rows,
//! the background spawn, and the merge of resolved images into the
//! [`super::avatar::AvatarStore`]. Sibling module per ADR-0121 (keep new
//! feature wiring out of `mod.rs`).

use gpui::{prelude::*, Context};

use super::{avatar_fetch, KagiApp};
use kagi_ui_core::klog;

impl KagiApp {
    /// W11-AVATAR (ADR-0037): start avatar resolution for the current repo.
    ///
    /// Resolution runs entirely on a background thread (`cx.background_spawn`):
    /// it determines the GitHub `(owner, repo)` from the repo's remotes, then
    /// resolves each distinct author email to an avatar image (noreply parse →
    /// Commits API batch → Gravatar / user search (ADR-0123) → disk/network
    /// fetch).  When it completes the resolved images are merged into
    /// `self.avatars.images` on the main thread and a `cx.notify()` repaints
    /// rows/inspector with real avatars.
    ///
    /// ADR-0123: incremental — new emails appearing on a reload / load more /
    /// tab switch are resolved by a follow-up pass; the per-frame call is one
    /// `view_epoch` comparison once a view has been scanned.
    pub(crate) fn ensure_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };

        // ADR-0123: incremental resolution. Reset the attempted set when the
        // active repo changes (an email unresolved in one repo can resolve via
        // the next repo's Commits API map); within a repo, re-scan the rows
        // only when the view data changed — reload / load more / tab-load all
        // bump `view_epoch` — so the per-frame call is one comparison.
        if self.avatars.fetch_for.as_deref() != Some(repo_path.as_path()) {
            self.avatars.fetch_for = Some(repo_path.clone());
            self.avatars.attempted.clear();
            self.avatars.scan_epoch = None;
        }
        if self.avatars.scan_epoch == Some(self.view_epoch) {
            return;
        }
        self.avatars.scan_epoch = Some(self.view_epoch);

        // Distinct author emails not yet attempted (nor already resolved).
        // The candidate list is collected first: `view()` borrows the whole
        // `KagiApp` (the read model lives in the session store, not in a field
        // of its own), so the `attempted` insert cannot run inside the loop.
        let candidates: Vec<String> = self
            .view()
            .rows
            .iter()
            .map(|row| row.author_email.clone())
            .filter(|email| !email.is_empty())
            .collect();
        let mut emails: Vec<String> = Vec::new();
        for email in candidates {
            if self.avatars.images.contains_key(&email) {
                continue;
            }
            if self.avatars.attempted.insert(email.clone()) {
                emails.push(email);
            }
        }
        if emails.is_empty() {
            return;
        }

        let offline = avatar_fetch::offline();

        // Determine GitHub coordinates (read-only via Backend). ADR-0123: a
        // non-GitHub repo only skips the Commits API step — the public lookups
        // (Gravatar / user search) still run when online. Offline + no coords
        // has nothing to do, so keep the synchronous pending-only line the
        // headless harness sees today.
        let coords = avatar_fetch::repo_github_coords(&repo_path);
        if coords.is_none() && offline {
            eprintln!(
                "[kagi] avatar: resolved=0 pending={} offline={}",
                emails.len(),
                offline
            );
            return;
        }

        let task =
            cx.background_spawn(async move { avatar_fetch::resolve_avatars(coords, &emails) });
        cx.spawn(async move |this, acx| {
            let outcome = task.await;
            let _ = this.update(acx, |app, cx| {
                for (email, img) in outcome.images {
                    app.avatars.images.insert(email, img);
                }
                // Emails skipped by the search-budget cap retry on the next
                // incremental pass (ADR-0123).
                for email in &outcome.deferred {
                    app.avatars.attempted.remove(email);
                }
                eprintln!(
                    "[kagi] avatar: resolved={} pending={} offline={}",
                    outcome.resolved, outcome.pending, offline
                );
                cx.notify();
            });
        })
        .detach();
    }
    fn ensure_github_login_avatars(
        &mut self,
        candidates: Vec<String>,
        source: &'static str,
        cx: &mut Context<Self>,
    ) {
        if avatar_fetch::offline() {
            return;
        }
        let mut logins: Vec<String> = Vec::new();
        for login in candidates {
            if login.is_empty() || self.avatars.images.contains_key(&login) {
                continue;
            }
            if self.avatars.attempted.insert(login.clone()) {
                logins.push(login);
            }
        }
        if logins.is_empty() {
            return;
        }

        let task = cx.background_spawn(async move {
            let mut out: Vec<(String, std::sync::Arc<gpui::Image>)> = Vec::new();
            for login in logins {
                let url = avatar_fetch::avatar_url_for_username(&login);
                if let Some(image) =
                    avatar_fetch::fetch_avatar_bytes(&url).and_then(avatar_fetch::image_from_bytes)
                {
                    out.push((login, image));
                }
            }
            out
        });
        cx.spawn(async move |this, acx| {
            let images = task.await;
            let _ = this.update(acx, |app, cx| {
                if images.is_empty() {
                    return;
                }
                let n = images.len();
                for (login, image) in images {
                    app.avatars.images.insert(login, image);
                }
                klog!("avatar: {} logins resolved={}", source, n);
                cx.notify();
            });
        })
        .detach();
    }

    /// Avatars for the GitHub **logins** on the PR page (ADR-0200): its author,
    /// its reviewers and assignees, and everyone in its conversation.
    ///
    /// Keyed by login in the same `avatars.images` map the commit rows use -
    /// a login has no `@`, a commit author's email does, so the two key spaces
    /// cannot collide. The URL is GitHub's own avatar CDN, which needs no API
    /// call and no token: one GET per login, disk-cached by
    /// [`super::avatar_fetch`], attempted once per process.
    pub(crate) fn ensure_pr_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.pr_mode() else {
            return;
        };
        let Some(tab) = mode.active.and_then(|ix| mode.tabs.get(ix)) else {
            return;
        };
        let mut candidates: Vec<String> = Vec::new();
        candidates.push(tab.pr.author.clone());
        candidates.extend(tab.pr.reviewers.iter().cloned());
        candidates.extend(tab.pr.assignees.iter().cloned());
        candidates.extend(tab.reviews.iter().map(|r| r.author.clone()));
        candidates.extend(tab.comments.iter().map(|c| c.author.clone()));
        candidates.extend(tab.line_comments.iter().map(|c| c.author.clone()));
        self.ensure_github_login_avatars(candidates, "pr", cx);
    }

    /// Resolve the viewer, Issue authors, and the selected conversation's
    /// authors through the same login-keyed cache and CDN path as PR avatars.
    pub(crate) fn ensure_issue_avatars(&mut self, cx: &mut Context<Self>) {
        let mut candidates = Vec::new();
        candidates.extend(self.github_login.iter().cloned());
        let ui = self.ui();
        candidates.extend(ui.github_issues.iter().map(|issue| issue.author.clone()));
        if let Some(issue) = ui
            .selected_github_issue
            .and_then(|number| ui.github_issue_details.get(&number))
        {
            candidates.push(issue.author.clone());
            candidates.extend(issue.comments.iter().map(|comment| comment.author.clone()));
        }
        self.ensure_github_login_avatars(candidates, "issue", cx);
    }
}
