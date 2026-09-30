//! Commit-graph paging (the "load more" row), split from `reload.rs` (#487).
//! Same owner/revision guards as the reloads next door, but an *amend*: it
//! refines the page on screen and never supersedes a full reload in flight.

use gpui::{prelude::*, Context, SharedString};

use crate::ui::{build_tab_view, FooterStatus, KagiApp, ToastKind, COMMIT_PAGE_STEP};

impl KagiApp {
    /// Grow the commit graph by [`COMMIT_PAGE_STEP`] and re-snapshot.
    ///
    /// Triggered by the "load more" row at the bottom of the commit list, which
    /// only appears once the graph holds at least `commit_limit` commits (i.e.
    /// the walk may have been truncated). Unlike [`reload`], this is a
    /// view-only refresh: it **amends** this owner's read model at the new limit
    /// but leaves selection, scroll position, open panels and modals untouched.
    /// Existing rows keep their indices because the additional commits are older
    /// and append at the bottom of the topological order.
    ///
    /// Amends rather than publishes (#482 stage 2 review, item 3): paging is a
    /// refinement of what is already on screen, not a fresh observation of the
    /// repository, so it must not supersede a full reload in flight. A watcher
    /// reload started by an external merge conflict carries the Conflict Mode
    /// re-detection, the modal sweep and the working-tree baseline that paging
    /// has no way to reproduce — rejecting it would leave the old semantic state
    /// standing until something else refreshed.
    pub fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(session) = self.active_session() else {
            return;
        };
        let previous_limit = self.ui().commit_limit;
        let commit_limit = previous_limit.saturating_add(COMMIT_PAGE_STEP);
        // #487: the paging read runs off the UI thread. Its result is applied
        // only while (a) `key` is still the owner's current read revision —
        // a full reload or an admitted mutation that moved it carries its own
        // page (`reload_async` captures `commit_limit` *after* this raise), and
        // (b) `gen` is still the owner's latest paging request — a later click
        // supersedes this one, so two in-flight pages can never land out of
        // order and shrink the graph. `current_key`, not `begin`: paging must
        // not supersede a reload in flight (see above).
        let key = self.reads.current_key(session);
        let gen = match self.ui_mut() {
            Some(ui) => {
                ui.commit_limit = commit_limit;
                ui.load_more_gen = ui.load_more_gen.wrapping_add(1);
                ui.load_more_gen
            }
            None => return,
        };
        let bg_path = repo_path.clone();
        let read = self.begin_slow_read(session, None, cx);
        let task = cx.background_spawn(async move {
            let mut repo =
                kagi_git::Backend::open(&bg_path).map_err(|e| format!("repo open error: {e}"))?;
            let snap = repo
                .snapshot_repairing_stat_cache(commit_limit, read.probe())
                .map_err(|e| format!("snapshot error: {e}"))?;
            let repo_name = bg_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bg_path.display().to_string());
            Ok::<_, String>((snap, repo_name))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                let current_gen = app.ui.get(&session).map(|ui| ui.load_more_gen);
                if current_gen != Some(gen) || !app.reads.is_fresh(key) {
                    // Superseded by a later page, a full reload or a mutation:
                    // whichever landed (or will) owns the graph now.
                    return;
                }
                let (snap, repo_name) = match result {
                    Ok(read) => read,
                    Err(msg) => {
                        klog!("load more: {}", msg);
                        // Nothing was appended, so the graph is still the
                        // previous page: put the limit back so the "load
                        // more" row stays offered instead of vanishing.
                        if let Some(ui) = app.ui.get_mut(&session) {
                            ui.commit_limit = previous_limit;
                        }
                        if app.active_session() == Some(session) {
                            let msg = format!("Load more failed: {msg}");
                            app.status_footer =
                                FooterStatus::Failed(SharedString::from(msg.clone()));
                            app.push_toast(ToastKind::Error, msg, cx);
                            cx.notify();
                        }
                        return;
                    }
                };
                let view = build_tab_view(&snap, &repo_name);
                app.amend_tab_view(session, view);
                klog!(
                    "load more: limit={} rows={}",
                    commit_limit,
                    app.reads.get(Some(session)).rows.len()
                );
                cx.notify();
            });
        })
        .detach();
    }
}
