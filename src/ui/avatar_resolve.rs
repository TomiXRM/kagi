//! Avatar resolution wiring on `KagiApp` (ADR-0122) — the UI half that feeds
//! [`super::avatar_fetch`]: incremental email collection from the loaded rows,
//! the background spawn, and the merge of resolved images into the
//! [`super::avatar::AvatarStore`]. Sibling module per ADR-0121 (keep new
//! feature wiring out of `mod.rs`).

use gpui::{prelude::*, Context};

use super::{avatar_fetch, KagiApp};

impl KagiApp {
    /// W11-AVATAR (ADR-0037): start avatar resolution for the current repo.
    ///
    /// Resolution runs entirely on a background thread (`cx.background_spawn`):
    /// it determines the GitHub `(owner, repo)` from the repo's remotes, then
    /// resolves each distinct author email to an avatar image (noreply parse →
    /// Commits API batch → Gravatar / user search (ADR-0122) → disk/network
    /// fetch).  When it completes the resolved images are merged into
    /// `self.avatars.images` on the main thread and a `cx.notify()` repaints
    /// rows/inspector with real avatars.
    ///
    /// ADR-0122: incremental — new emails appearing on a reload / load more /
    /// tab switch are resolved by a follow-up pass; the per-frame call is one
    /// `view_epoch` comparison once a view has been scanned.
    pub(crate) fn ensure_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };

        // ADR-0122: incremental resolution. Reset the attempted set when the
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
        let mut emails: Vec<String> = Vec::new();
        for row in &self.active_view.rows {
            let email = &row.author_email;
            if email.is_empty() || self.avatars.images.contains_key(email) {
                continue;
            }
            if self.avatars.attempted.insert(email.clone()) {
                emails.push(email.clone());
            }
        }
        if emails.is_empty() {
            return;
        }

        let offline = avatar_fetch::offline();

        // Determine GitHub coordinates (read-only via Backend). ADR-0122: a
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
                // incremental pass (ADR-0122).
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
}
