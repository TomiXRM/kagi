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
use super::{build_tab_view, FooterStatus, KagiApp, WipDiffStat, COMMIT_PAGE_STEP};

impl KagiApp {
    /// Reload all display data from the repository at `repo_path`.
    ///
    /// Called at the tail of nearly every mutation (checkout, commit, discard,
    /// merge, …) to update the commit list, header, branch list, and badges
    /// without restarting the application.
    ///
    /// #288: this now runs the heavy git read (open + full snapshot + per-branch
    /// ahead/behind + every linked worktree's status + wip diffstat + reflog
    /// seed) on a **background** thread and applies the result on the UI thread,
    /// so a 10k-commit repo no longer freezes the window at the end of an op.
    /// The apply is guarded by the reload epoch (#287) so an op's authoritative
    /// reload wins over an in-flight FS-watcher reload. `external = false`: the
    /// op already set its own footer, so stay quiet (same surface as the old
    /// synchronous `reload()`, minus the frozen frame).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.reload_async(false, cx);
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
        let Some(session) = self.active_session() else {
            return;
        };
        let view = build_tab_view(&snap, &repo_name);
        self.selected = None;
        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        self.publish_tab_view(session, view);
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
        let Some(session) = self.active_session() else {
            return Ok(());
        };
        // #287: starting a read bumps this owner's read revision, so an
        // FS-watcher reload still in flight is refused on completion — this
        // synchronous, user-initiated refresh is authoritative. (Manual Cmd+R /
        // settings toggle path; stays sync so the caller can surface a
        // repo-open/snapshot error.)
        let key = self.reads.begin(session);
        let want_panel = self.conflict_merge_pending;
        let want_reflog = self.operation_history.is_empty();
        let data = match read_reload_data(&repo_path, self.commit_limit, want_panel, want_reflog) {
            Ok(d) => d,
            Err(msg) => {
                self.reads.fail(key);
                klog!("reload: {}", msg);
                return Err(msg);
            }
        };
        self.apply_reload_data(key, repo_path, data, false, cx);
        Ok(())
    }

    /// Apply an already-read [`ReloadData`] to `self` on the UI thread. This is
    /// the whole "fold a fresh snapshot into the view" step, shared verbatim by
    /// the synchronous [`reload_checked`] and the background [`reload_async`] so
    /// the two can never drift (modal resets, overlay refresh, reflog seed,
    /// conflict re-detect, continued-merge panel). The heavy git I/O already
    /// happened in [`read_reload_data`]; nothing here opens the repo except the
    /// rare continued-merge fallback.
    ///
    /// `external = true` emits the `refreshed (external change)` contract line
    /// and resets the footer (FS-watcher path); `false` stays quiet (op tail /
    /// manual refresh, which set their own footer).
    fn apply_reload_data(
        &mut self,
        key: crate::app::ReadKey,
        repo_path: std::path::PathBuf,
        data: ReloadData,
        external: bool,
        cx: &mut Context<Self>,
    ) {
        let session = key.session();
        let ReloadData {
            snap,
            wip_diffstat,
            repo_name,
            reflog,
            panel,
        } = data;

        // Capture the CommitId of the currently-selected row before we rebuild,
        // so we can re-select it after (survives selection across a reload).
        let prev_commit_id: Option<CommitId> = self
            .selected
            .and_then(|idx| self.view().details.get(idx))
            .map(|detail| CommitId(detail.full_sha.to_string()));

        // #482 stage 2: hand the rebuilt read model to its owner first. A
        // superseded reload (a newer read, or a mutation admitted while this one
        // was in flight) writes nothing and produces no display side effect at
        // all — the epoch/generation pair this used to compare is gone.
        let view = build_tab_view(&snap, &repo_name);
        // Whether this reload is the read that *first* fills the tab — i.e. it
        // superseded the load the tab switch started. The placeholder clears
        // itself (`loading_tab()` is derived), but the `Loading …` footer that
        // switch set is stateful and has to be settled here, or a Cmd+R during
        // the first load leaves the status bar Busy forever.
        let first_read = !self.reads.has_read(session);
        if !self.accept_tab_view(key, view) {
            return;
        }
        self.app_sessions.read_applied(session);
        // The owner is not the tab on screen: its data is refreshed, but none of
        // the display folding below (modals, selection, conflict, panels)
        // belongs to it.
        if self.active_session() != Some(session) {
            return;
        }

        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        // ADR-0119 follow-up: the full-screen Analyze + File History overlays are
        // HEAD-versioned and refreshed *in place* after the snapshot is applied
        // (see `refresh_overlays_after_reload`), only when HEAD actually moved.
        self.clear_plan_modal();
        self.clear_pull_modal();
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

        // ADR-0119 follow-up: refresh (never close) the HEAD-versioned overlays.
        self.refresh_overlays_after_reload(self.view().head_oid.clone(), cx);

        // ADR-0084: seed the undo/redo history from the branch reflog when it is
        // empty (freshly-opened repo / post-branch-switch) so Cmd+Z works
        // immediately. Only seed when empty — never clobber the in-session stack.
        // (`want_reflog` was captured at read time; re-check emptiness here in
        // case an in-session op recorded history while the read was in flight.)
        if let Some(reflog) = reflog {
            if self.operation_history.is_empty() {
                self.apply_reflog_seed(reflog);
            }
        }

        // Baseline for the FS watcher's working-tree path (skip-if-unchanged).
        self.last_working_status = Some(snap.status.clone());

        // Re-resolve selection by CommitId after the graph rebuild.
        self.selected = None;
        if let Some(ref cid) = prev_commit_id {
            if let Some(&new_idx) = self.view().commit_row_index.get(cid) {
                self.selected = Some(new_idx);
            }
        }

        // W30-CONFLICT-UI / ADR-0056: re-detect Conflict Mode every reload so a
        // conflict produced by the GUI's own operation OR by external CLI (the
        // watcher path runs through here now too) puts the app into / out of
        // Conflict Mode. Force re-detection by invalidating the run-once guard.
        self.conflict_detected_for = None;
        self.detect_conflict_mode(cx);

        // Re-resolve the continued-merge flow after detection.
        if was_merge_commit_pending {
            if self.merge_commit_ready {
                // Still a resolved merge awaiting its commit: keep the commit
                // panel up (refresh the staged list from the index) and keep the
                // pre-filled / user-edited merge message entity untouched.
                // `panel` was read in the background when the merge was pending at
                // spawn; fall back to a (rare) sync read if it wasn't.
                let mut panel = panel.unwrap_or_else(|| CommitPanelState::from_repo(&repo_path));
                // #473: never overwrite a panel that belongs to another
                // worktree with THIS repo's staging lists — leave it alone.
                if let Some(entity) = self
                    .commit_panel
                    .clone()
                    .filter(|e| e.read(cx).repo_path == repo_path)
                {
                    entity.update(cx, |v, _| {
                        panel.tree_view = v.state.tree_view;
                        v.state = panel;
                    });
                } else if self.commit_panel.is_none() {
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

        // ADR-0128 follow-up: Branch Cleanup classification is re-armed by
        // `render` off `scans_stale`, which `accept_tab_view` set above.

        if first_read && matches!(self.status_footer, FooterStatus::Busy(_)) {
            self.status_footer =
                FooterStatus::Idle(SharedString::from(super::i18n::Msg::Ready.t()));
        }

        if external {
            klog!("refreshed (external change)");
            self.status_footer =
                FooterStatus::Idle(SharedString::from("[kagi] refreshed (external change)"));
        }

        // #309: open the "drop the kept stash?" prompt AFTER the modal-clearing
        // sweep above, so it survives this async reload (opening it before reload
        // would have it wiped by `clear_stash_drop_modal`). Just plans + sets the
        // modal — no further reload.
        self.present_stash_followup(cx);

        cx.notify();
    }

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
        self.amend_tab_view(session, view);
        klog!(
            "load more: limit={} rows={}",
            self.commit_limit,
            self.view().rows.len()
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
        let bg_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(session) = self.active_session() else {
            return;
        };
        // #287 / #482 stage 2: starting the read bumps this owner's read
        // revision. The result is refused on completion if anything moved it
        // since — a *later* reload (typically an op's own authoritative reload)
        // wins over this in-flight one, and an admitted mutation invalidates it.
        // This is what stops an FS-watcher snapshot that read a mid-write tree
        // from clobbering the correct post-op view (#287). The tab-switch half
        // of the old guard is now structural: the key names its owner.
        let key = self.reads.begin(session);
        let commit_limit = self.commit_limit;
        let want_panel = self.conflict_merge_pending;
        let want_reflog = self.operation_history.is_empty();
        let apply_path = bg_path.clone();
        // ADR-0104 / #288: move the whole heavy git read (open + full snapshot +
        // per-branch ahead/behind + every linked worktree's status + wip diffstat
        // + reflog seed + continued-merge panel) off the UI thread. Every field
        // of `ReloadData` is pure/`Send`; the view + gpui entities are built on
        // the UI thread in `apply_reload_data`.
        let task = cx.background_spawn(async move {
            read_reload_data(&bg_path, commit_limit, want_panel, want_reflog).ok()
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some(data) = result else {
                    // Open or snapshot failed — settle the read so the same one
                    // can be asked for again, then log and bail without nuking
                    // the existing view (better to show stale data than none).
                    if !app.reads.fail(key) || app.active_session() != Some(session) {
                        return;
                    }
                    klog!("reload_external: snapshot failed (non-fatal)");
                    app.status_footer = FooterStatus::Idle(SharedString::from(
                        "[kagi] refresh skipped (snapshot failed)",
                    ));
                    cx.notify();
                    return;
                };
                app.apply_reload_data(key, apply_path, data, external, cx);
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
        let Some(session) = self.active_session() else {
            return;
        };
        let bg_path = repo_path.clone();
        // #287: capture (but do NOT bump) this owner's read revision. A full
        // reload bumps it; if one lands while this cheaper working-tree read is
        // in flight, its fresh baseline is authoritative and this stale status is
        // dropped — otherwise a slow working-tree read could overwrite the
        // post-reload `last_working_status` baseline with an older snapshot. We
        // must not bump here: a working-tree refresh is a subset of a full reload
        // and must not invalidate one, so it takes no request slot either.
        // (ponytail: same-revision worktree-vs-worktree ordering is still
        // unguarded — one WIP read superseding another has no observable
        // difference, both being a status count of the same tree.)
        let key = self.reads.current_key(session);
        let task = cx.background_spawn(async move {
            let backend = kagi_git::Backend::open(&bg_path).ok()?;
            let status = backend.working_tree_status().ok()?;
            let wip_diffstat = KagiApp::wip_diffstat_from_backend(&backend);
            Some((status, wip_diffstat))
        });
        cx.spawn(async move |this, acx| {
            let refreshed = task.await;
            let _ = this.update(acx, |app, cx| {
                // The watcher event was for this session's worktree; if the
                // user switched tabs while we read it, these counts belong to
                // another repo and would be written into this tab's status bar
                // and WIP row. #287: and a full reload (or an admitted mutation)
                // superseding this read drops the stale status.
                if app.active_session() != Some(session) || !app.reads.is_fresh(key) {
                    return;
                }
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
                // #482 stage 2: a status-only change updates the owner's read
                // model **in place** — the commit rows, details and ref lists it
                // does not touch are not copied.
                let view = app.view_mut();
                view.status_summary.is_dirty = new_status.is_dirty();
                view.status_summary.staged = new_status.staged.len();
                view.status_summary.unstaged = new_status.unstaged.len();
                view.status_summary.untracked = new_status.untracked.len();
                view.status_summary.conflict_count = new_status.conflicted.len();
                view.is_dirty = new_status.is_dirty();
                app.last_working_status = Some(new_status);
                app.wip_diffstat = Some(wip_diffstat);
                // Refresh the open commit panel's lists in place (keeps it open).
                // ADR-0118 (correction #6c): update the entity, never rebuild via
                // a parent render read.
                // #473: reload from the PANEL's own repo, not the tab's — a
                // panel showing a linked worktree would otherwise be silently
                // refilled with the open repository's files on the next tick.
                // (The watcher only watches the open working tree, so a worktree
                // panel simply does not auto-refresh; re-clicking its WIP row
                // reloads it.)
                if let Some(entity) = app.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        let rp = v.repo_path.clone();
                        v.state.reload_status(&rp);
                    });
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Owned, `Send` result of the heavy git read behind a reload (#288). Built by
/// [`read_reload_data`] on either the UI thread (sync `reload_checked`) or a
/// background thread (`reload_async`), then folded into the view by
/// [`KagiApp::apply_reload_data`]. Deliberately holds no gpui types.
struct ReloadData {
    snap: kagi_git::RepoSnapshot,
    wip_diffstat: WipDiffStat,
    repo_name: String,
    /// `Some` only when the reflog was read (history was empty at spawn); the
    /// inner `Result` is the reflog read outcome. `None` = seeding skipped.
    reflog: Option<Result<Vec<kagi_git::HistoryEntry>, String>>,
    /// `Some` only when a continued-merge was pending at spawn (so the commit
    /// panel's file lists were pre-read off the UI thread).
    panel: Option<CommitPanelState>,
}

/// The heavy git read behind every reload, isolated so it can run either
/// synchronously ([`KagiApp::reload_checked`]) or on a background thread
/// ([`KagiApp::reload_async`], #288). Returns owned, `Send` data — no gpui, no
/// view build. `want_reflog` / `want_panel` gate the two optional extra reads so
/// we don't pay for them when they can't matter. On error the `String` is an
/// already-formatted message (`"repo open error: …"` / `"snapshot error: …"`).
fn read_reload_data(
    repo_path: &std::path::Path,
    commit_limit: usize,
    want_panel: bool,
    want_reflog: bool,
) -> Result<ReloadData, String> {
    let mut backend =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("repo open error: {e}"))?;
    let snap = backend
        .snapshot(commit_limit)
        .map_err(|e| format!("snapshot error: {e}"))?;
    let wip_diffstat = KagiApp::wip_diffstat_from_backend(&backend);
    let repo_name = repo_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| repo_path.display().to_string());
    let reflog = if want_reflog {
        Some(backend.history_from_reflog().map_err(|e| e.to_string()))
    } else {
        None
    };
    let panel = if want_panel {
        Some(CommitPanelState::from_repo(repo_path))
    } else {
        None
    };
    Ok(ReloadData {
        snap,
        wip_diffstat,
        repo_name,
        reflog,
        panel,
    })
}
