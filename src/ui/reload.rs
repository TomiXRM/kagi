//! Repository reload / refresh paths (ADR-0121 Phase A: behaviour-preserving
//! relocation out of `mod.rs`).
//!
//! Sync (`reload` / `reload_checked`), pre-launch (`reload_prelaunch`),
//! background (`reload_async` / `reload_external`, ADR-0104), the cheap
//! working-tree-only refresh (`refresh_working_tree_external`), the
//! HEAD-versioned overlay refresh (ADR-0119 follow-up), and commit-graph
//! paging (`load_more_commits`).

use gpui::{prelude::*, Context, SharedString};

use kagi_git::CommitId;

use super::commit_panel::{CommitPanelState, CommitPanelView};
use super::{build_tab_view, FooterStatus, KagiApp, COMMIT_PAGE_STEP};

impl KagiApp {
    /// Reload all display data from the repository at `repo_path`.
    ///
    /// Called after a successful checkout to update the commit list, header,
    /// branch list, and badges without restarting the application.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let _ = self.reload_checked(cx);
    }

    /// Pre-launch reload (headless `init_tab` / session restore). Runs before the
    /// gpui window exists, so there is no `Context` and no `Entity<KagiApp>` to
    /// hand a `ConflictView` — the conflict panel cannot be built here. Does the
    /// snapshot/view rebuild (so the commit list / header are populated and the
    /// `build_tab_view` `[kagi]` lines fire) but SKIPS conflict detection: the
    /// `conflict_detected_for` guard is left UNSET so the first cx-bearing detect
    /// at launch (`ensure_startup_repo_io` → `detect_conflict_mode_async`) builds
    /// the entity and emits the `conflict-mode:` line. ADR-0118 /
    /// T-ENTITY-CONFLICT-001.
    pub fn reload_prelaunch(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("reload: repo open error: {}", e);
                return;
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("reload: snapshot error: {}", e);
                return;
            }
        };
        let wip_diffstat = Self::wip_diffstat_from_backend(&repo);
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());
        let view = build_tab_view(&snap, &repo_name);
        self.selected = None;
        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        self.tab_cache.insert(repo_path.clone(), view.clone());
        self.apply_tab_view(view);
        self.seed_history_from_reflog(&repo);
        self.last_working_status = Some(snap.status.clone());
        // Conflict detection intentionally deferred to the launch-time
        // cx-bearing path (see the doc comment).
    }

    /// Like [`reload`] but reports failure. Returns `Err(msg)` when the repo
    /// can't be reopened or snapshotted (the current view is left intact), so a
    /// user-initiated refresh can surface the error instead of falsely reporting
    /// success. `Ok(())` also covers "no repo open" (nothing to refresh). The
    /// passive FS-watcher path uses [`reload_external`], which stays silent.
    pub fn reload_checked(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return Ok(()),
        };

        // Re-open and snapshot.
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("reload: repo open error: {}", e);
                return Err(e.to_string());
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("reload: snapshot error: {}", e);
                return Err(e.to_string());
            }
        };
        let wip_diffstat = Self::wip_diffstat_from_backend(&repo);

        // Derive repo name from path.
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());

        // W6-TABSPEED: rebuild the pure display data (same log output as before),
        // reset per-repo transient UI state, then fold the view in via
        // `apply_tab_view`.  ADR-0030 §5: reload() also refreshes the cache.
        let view = build_tab_view(&snap, &repo_name);

        // Per-repo transient state reset (unchanged behaviour).
        self.selected = None;
        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        // ADR-0119 follow-up: the full-screen Analyze + File History overlays are
        // HEAD-versioned and refreshed *in place* after the snapshot is applied
        // (see `refresh_overlays_after_reload`), only when HEAD actually moved.
        // They used to be dropped on EVERY reload, so a no-op auto-fetch (remote
        // refs only, HEAD unchanged) yanked the user out of a full-screen view
        // and discarded a ~minute-long Analyze mine.
        self.clear_plan_modal();
        self.clear_pull_modal();
        self.clear_undo_modal();
        self.clear_amend_modal();
        self.clear_pop_modal();
        self.clear_stash_drop_modal();
        self.clear_branch_plan_modal();
        self.clear_set_upstream_modal();
        self.clear_rename_branch_modal();
        self.clear_discard_modal();
        self.clear_create_branch_modal();
        self.clear_create_worktree_modal();
        self.modal_focus = None;
        self.clear_stash_push_modal();
        self.clear_stash_apply_modal();
        self.stash_push_focus = None;
        self.clear_cherry_pick_modal();
        self.clear_revert_modal();
        self.clear_conflict_continue_modal();
        // A merge that has been continued to the commit panel triggers its own
        // FS-watcher reload (staging writes the working tree + index). Preserve
        // the commit panel + merge message across that self-induced reload so the
        // user is not bounced out of the commit screen; the post-detect block
        // below confirms the merge is still pending (else it resets everything).
        let was_merge_commit_pending = self.conflict_merge_pending;
        self.commit_menu = None;
        self.file_menu = None;
        self.stash_menu = None;
        self.worktree_menu = None;
        if !was_merge_commit_pending {
            // ADR-0068: a reload after commit / abort ends any continued-merge flow.
            self.conflict_merge_pending = false;
            // T025/T026: drop the commit-panel entity (state + inputs + template)
            // so it reflects fresh status after reload (ADR-0118: one entity).
            self.commit_panel_open = false;
            self.commit_panel = None;
        }
        // commit_scroll_handle is preserved so the existing Rc<RefCell<...>> reference
        // wired into the uniform_list continues to work after reload.
        // status_footer is intentionally preserved across reloads so the last
        // operation result remains visible after the commit list refreshes.
        // sidebar_width / panel_width are also preserved so the user's resize
        // is not lost on checkout/reload (T023).
        // T-BP-004: the op_log entity (entries + expanded row + scroll handle)
        // persists across reloads/tab switches so the Operation Log keeps its
        // contents and UI state.
        // sidebar_collapsed / sidebar_filter are preserved so the user's
        // collapse + filter state survives reload.
        // W13-BRANCHTREE: branch_groups_collapsed is likewise preserved so the
        // user's per-group ▸/▾ state survives checkout/reload.

        // ADR-0030 §5: keep the stale-while-revalidate cache fresh.
        self.tab_cache.insert(repo_path.clone(), view.clone());

        // Fold the snapshot-derived data in (assignment only).
        self.apply_tab_view(view);

        // ADR-0119 follow-up: refresh (never close) the HEAD-versioned overlays.
        self.refresh_overlays_after_reload(self.active_view.head_oid.clone(), cx);

        // ADR-0084: seed the undo/redo history from the branch reflog when it is
        // empty (freshly-opened repo / post-branch-switch) so Cmd+Z works
        // immediately. Only seed when empty — never clobber the in-session stack.
        self.seed_history_from_reflog(&repo);

        // Baseline for the FS watcher's working-tree path (skip-if-unchanged).
        self.last_working_status = Some(snap.status.clone());

        // W30-CONFLICT-UI / ADR-0056: re-detect Conflict Mode every reload so a
        // conflict produced by the GUI's own operation OR by external CLI (the
        // watcher path runs through reload) puts the app into / out of Conflict
        // Mode.  Force re-detection by invalidating the run-once guard.
        self.conflict_detected_for = None;
        self.detect_conflict_mode(cx);

        // Re-resolve the continued-merge flow after detection.
        if was_merge_commit_pending {
            if self.merge_commit_ready {
                // Still a resolved merge awaiting its commit: keep the commit
                // panel up (refresh the staged list from the index) and keep the
                // pre-filled / user-edited merge message entity untouched.
                // ADR-0118: update the entity's `state` IN PLACE so its inputs /
                // template mode (the pre-filled merge message) survive the reload.
                let mut panel = CommitPanelState::from_repo(&repo_path);
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        panel.tree_view = v.state.tree_view;
                        v.state = panel;
                    });
                } else {
                    let weak_app = cx.weak_entity();
                    let entity =
                        cx.new(|_| CommitPanelView::new(panel, weak_app, repo_path.clone()));
                    self.commit_panel = Some(entity);
                }
                self.commit_panel_open = true;
                self.conflict = None;
                self.conflict_merge_pending = true;
            } else {
                // The merge commit was created (MERGE_HEAD gone) or aborted — end
                // the flow and drop the commit-panel entity.
                self.conflict_merge_pending = false;
                self.commit_panel_open = false;
                self.commit_panel = None;
            }
        }
        Ok(())
    }

    /// Grow the commit graph by [`COMMIT_PAGE_STEP`] and re-snapshot.
    ///
    /// Triggered by the "load more" row at the bottom of the commit list, which
    /// only appears once the graph holds at least `commit_limit` commits (i.e.
    /// the walk may have been truncated). Unlike [`reload`], this is a
    /// view-only refresh: it rebuilds `active_view` (and the tab cache) at the
    /// new limit but leaves selection, scroll position, open panels and modals
    /// untouched. Existing rows keep their indices because the additional
    /// commits are older and append at the bottom of the topological order.
    pub fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        self.commit_limit = self.commit_limit.saturating_add(COMMIT_PAGE_STEP);

        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("load more: repo open error: {}", e);
                return;
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("load more: snapshot error: {}", e);
                return;
            }
        };
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());

        let view = build_tab_view(&snap, &repo_name);
        self.tab_cache.insert(repo_path.clone(), view.clone());
        self.apply_tab_view(view);
        klog!(
            "load more: limit={} rows={}",
            self.commit_limit,
            self.active_view.rows.len()
        );
        cx.notify();
    }

    /// HEAD-versioned refresh of the long-lived full-screen overlays (Analyze +
    /// File History) after a repo reload (ADR-0119 follow-up). These views are
    /// NOT closed on every reload — that yanks the user out of a full-screen
    /// view and throws away a ~minute-long Analyze mine. Instead:
    ///
    /// - **HEAD unchanged** (an auto-fetch that only moved remote-tracking refs,
    ///   a working-tree edit, a no-op manual refresh): the mined / loaded data is
    ///   still valid → leave both views exactly as they are.
    /// - **HEAD moved** (new commit / checkout / pull / reset): the data is stale
    ///   → invalidate this repo's Analyze cache and re-mine *in place* if the
    ///   view is open (the app-owned mine seeds the open view on completion), and
    ///   reload the File History view *in place*. Neither view closes.
    fn refresh_overlays_after_reload(&mut self, new_head: Option<String>, cx: &mut Context<Self>) {
        let Some(repo) = self.repo_path.clone() else {
            return;
        };

        // ── Analyze (Code Ecosystem) ──
        // Staleness is keyed on the HEAD the cached mine reflects. A mine still
        // in flight (no cache entry yet) is left to finish; the next reload
        // re-checks it against the then-current HEAD.
        let eco_stale = self
            .ecosystem_cache
            .get(&repo)
            .is_some_and(|c| c.head != new_head);
        if eco_stale {
            self.ecosystem_cache.remove(&repo);
            // Clear the in-flight guard so a fresh mine can start below.
            if self.ecosystem_inflight.as_deref() == Some(repo.as_path()) {
                self.ecosystem_inflight = None;
            }
            // Re-mine only when the view is actually open; otherwise just drop
            // the stale entry (the next open will mine on demand).
            if self.ecosystem.is_some() {
                self.start_ecosystem_mine(repo.clone(), new_head.clone(), cx);
            }
        }

        // ── File History ──
        // Per-file history also reflects HEAD; reload it in place only when HEAD
        // moved, and never drop the view on an unrelated reload.
        if let Some(fh) = self.file_history.clone() {
            if self.file_history_head != new_head {
                fh.update(cx, |v, cx| v.reload(false, cx));
                self.file_history_head = new_head;
            }
        }
    }

    /// Reload triggered by an external git change (T029: FS watcher).
    ///
    /// Behaves identically to `reload()` but additionally:
    /// - Emits the required `[kagi] refreshed (external change)` log line.
    /// - Updates the status footer to show the refresh message.
    /// - Attempts to re-select the previously selected commit by CommitId;
    ///   if the commit no longer exists the selection is cleared.
    pub fn reload_external(&mut self, cx: &mut Context<Self>) {
        self.reload_async(true, cx);
    }

    /// Background snapshot + UI-thread apply (mechanics of ADR-0104).
    ///
    /// `external`:
    /// * `true`  — external git event: emits the `refreshed (external change)`
    ///   contract line and resets the footer (the user didn't ask for anything).
    /// * `false` — tail of a user-initiated background op (pull/push/fetch):
    ///   the op already set its own Success footer, so keep it and stay quiet —
    ///   same surface as the synchronous `reload()`, minus the frozen frame.
    pub fn reload_async(&mut self, external: bool, cx: &mut Context<Self>) {
        // Capture the CommitId of the currently selected row (if any) so we
        // can attempt to re-select it after the snapshot is refreshed.
        // `details[idx].full_sha` is the canonical commit hash string.
        let prev_commit_id: Option<CommitId> = self
            .selected
            .and_then(|idx| self.active_view.details.get(idx))
            .map(|detail| CommitId(detail.full_sha.to_string()));

        // ADR-0104 / performance: an external git event (HEAD/refs change from
        // a terminal, sibling worktree, or auto-fetch) is NOT user-initiated,
        // so freezing the UI frame for a full repo snapshot (topological walk,
        // full working-tree status scan, ahead/behind for every branch) is the
        // worst kind of jank — the user didn't ask for anything. Move the
        // heavy git2 work (open + snapshot + wip diffstat) onto a background
        // thread, then build the view data and apply it on the UI thread.
        // (The synchronous `reload()` is still used by user-initiated paths
        // where a short, expected wait is acceptable.)
        let bg_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Capture the switch generation so the background result is discarded
        // if the user switched tabs while the snapshot was in flight — without
        // this guard the snapshot would overwrite the NEW tab's freshly-loaded
        // view with the OLD tab's data (cross-review N4).
        let gen_at_spawn = self.switch_generation;
        let commit_limit = self.commit_limit;
        let task = cx.background_spawn(async move {
            let mut backend = kagi_git::Backend::open(&bg_path).ok()?;
            let snap = backend.snapshot(commit_limit).ok()?;
            let wip = KagiApp::wip_diffstat_from_backend(&backend);
            // RepoSnapshot is pure domain (Send); build_tab_view constructs
            // SharedString-bearing TabViewState, so we return the raw pieces
            // and build the view on the UI thread.
            let repo_name = bg_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bg_path.display().to_string());
            Some((snap, wip, repo_name))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // Generation guard (cross-review N4): if the user switched tabs
                // while the snapshot was in flight, drop the result — applying
                // it would clobber the now-current tab's view.
                if app.switch_generation != gen_at_spawn {
                    return;
                }
                let Some((snap, wip, repo_name)) = result else {
                    // Open or snapshot failed — log and bail without nuking
                    // the existing view (better to show stale data than none).
                    klog!("reload_external: snapshot failed (non-fatal)");
                    app.status_footer = FooterStatus::Idle(SharedString::from(
                        "[kagi] refresh skipped (snapshot failed)",
                    ));
                    cx.notify();
                    return;
                };
                // Build the view data on the UI thread (cheap; heavy git2 work
                // already done in the background) and apply it.
                let view = build_tab_view(&snap, &repo_name);
                app.apply_tab_view(view);
                app.diff_caches.clear();
                app.wip_diffstat = Some(wip);
                app.main_diff = None;
                app.compare_view = None;
                // ADR-0119 follow-up: refresh (never close) the HEAD-versioned
                // overlays in place — only when HEAD actually moved. An external
                // change that doesn't move HEAD (e.g. a sibling-worktree fetch)
                // leaves Analyze + File History untouched.
                app.refresh_overlays_after_reload(app.active_view.head_oid.clone(), cx);

                // Attempt to restore selection by CommitId.
                app.selected = None;
                if let Some(ref cid) = prev_commit_id {
                    if let Some(&new_idx) = app.active_view.commit_row_index.get(cid) {
                        app.selected = Some(new_idx);
                    }
                    // If the commit is no longer present, selected stays None.
                }

                // Emit the required log line and update the footer (external
                // events only; op tails keep their Success footer).
                if external {
                    klog!("refreshed (external change)");
                    app.status_footer = FooterStatus::Idle(SharedString::from(
                        "[kagi] refreshed (external change)",
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Working-tree change refresh (FS watcher, [`watcher::WatchEvent::WorkTree`]).
    ///
    /// Files changed on disk outside `.git` — so the WIP / working-tree status may
    /// have changed, but the commit graph did not. Computes the new status on a
    /// **background thread** and only does a (full) refresh if it actually differs
    /// from [`Self::last_working_status`]. This makes churn that doesn't affect the
    /// parent repo's status (e.g. writes inside a nested worktree, which
    /// `working_tree_status` treats as opaque) a cheap no-op — no UI-thread work,
    /// no reload storm — while real edits/adds/deletes update the WIP promptly.
    pub fn refresh_working_tree_external(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&bg_path).ok()?;
            let status = backend.working_tree_status().ok()?;
            let wip_diffstat = KagiApp::wip_diffstat_from_backend(&backend);
            Some((status, wip_diffstat))
        });
        cx.spawn(async move |this, acx| {
            let refreshed = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some((new_status, wip_diffstat)) = refreshed else {
                    return;
                };
                // T-WS-EDITOR-002 §4: nudge the Editor Workspace on every
                // worktree watch event, unconditionally — a content-only
                // edit to an already-`Modified` tracked file may not change
                // `WorkingTreeStatus` (no `ChangeKind` transition), so this
                // must not be gated behind the "status unchanged" early
                // return below.
                if let Some(ev) = app.editor_workspace.clone() {
                    ev.update(cx, |v, cx| v.on_worktree_changed(cx));
                }
                if app.last_working_status.as_ref() == Some(&new_status) {
                    if app.wip_diffstat != Some(wip_diffstat) {
                        app.wip_diffstat = Some(wip_diffstat);
                        cx.notify();
                    }
                    return; // working-tree status unchanged → nothing to do.
                }
                klog!("watcher: working-tree changed — refreshing WIP");
                // In-place WIP/status update — do NOT full-reload (that re-snapshots
                // the graph and closes the commit panel). Branch / ahead-behind are
                // unchanged by a working-tree edit, so only the dirty/count fields
                // and the commit panel's file lists need refreshing.
                app.active_view.status_summary.is_dirty = new_status.is_dirty();
                app.active_view.status_summary.staged = new_status.staged.len();
                app.active_view.status_summary.unstaged = new_status.unstaged.len();
                app.active_view.status_summary.untracked = new_status.untracked.len();
                app.active_view.status_summary.conflict_count = new_status.conflicted.len();
                app.active_view.is_dirty = new_status.is_dirty();
                app.last_working_status = Some(new_status);
                app.wip_diffstat = Some(wip_diffstat);
                // Refresh the open commit panel's lists in place (keeps it open).
                // ADR-0118 (correction #6c): update the entity, never rebuild via
                // a parent render read.
                if let (Some(entity), Some(rp)) = (app.commit_panel.clone(), app.repo_path.clone())
                {
                    entity.update(cx, |v, _| v.state.reload_status(&rp));
                }
                cx.notify();
            });
        })
        .detach();
    }
}
