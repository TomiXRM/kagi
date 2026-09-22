use super::*;
impl KagiApp {
    /// Make retained repository-derived state non-authoritative before an
    /// activation read (ADR-0197 決定 3 / #722 P2). "Not authoritative" means
    /// **stop**, not **destroy**: recomputable caches are dropped, but every
    /// pane entity stays alive, so undo/redo, the selected file/hunk and
    /// scroll all survive a tab round trip. What the panes lose is the right
    /// to *act*: `pane_revalidation` refuses their mutations until the read
    /// lands. `revalidate_retained_panes` then compares the new observation —
    /// unchanged panes are simply re-enabled, changed ones are rebuilt there
    /// and (for conflict) by the re-armed detection below.
    pub(crate) fn begin_session_revalidation(&mut self, session: crate::app::SessionId) {
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.cache_epoch = ui.cache_epoch.wrapping_add(1);
            ui.diff_caches.clear();
            ui.wip_diffstat = None;
            ui.last_working_status = None;
            ui.pane_revalidation = crate::ui::tab_ui_state_ops::PaneRevalidation::AwaitingRead;
            ui.conflict_merge_pending = false;
            // Re-arm detection: its outcome decides whether the retained
            // conflict pane is updated in place or replaced.
            ui.conflict_detected = false;
            // PR mode is dropped rather than re-checked. A `PrTab` is a snapshot
            // of the refs *and* of GitHub (reviews, merge status, conflict
            // preview) taken when it was opened, and nothing refreshes it while
            // the tab is away; rebuilding it on activation needs the PR list
            // that the activation is still fetching, races the loads started
            // before departure, and loses the user's position anyway. Carrying
            // PR mode across a switch is out of scope for #643 Wave 4 S6 — the
            // tab reopens the PR. The *ownership* stays session-scoped, so B
            // never sees A's PRs.
            ui.leave_pr_mode();
        }
    }

    /// Drop row-indexed caches for `session`.
    pub fn invalidate_caches_for_row_renumber(&mut self, session: crate::app::SessionId) {
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.cache_epoch = ui.cache_epoch.wrapping_add(1);
            ui.diff_caches.clear();
        }
        if self.active_session() != Some(session) {
            return;
        }
        // Main Diff / Compare are not dropped here: a published read queues
        // `revalidate_retained_panes`, which re-anchors them (#722).
        self.commit_menu = None;
        self.inspector_file_menu = None;
    }

    /// The read model on screen. The empty one on the Welcome screen, and while
    /// a tab's first read is still in flight ([`KagiApp::loading_tab`] is what
    /// says so).
    pub fn view(&self) -> &TabViewState {
        self.reads.get(self.active_session())
    }

    /// A merge whose conflicts are all resolved and which is waiting for its
    /// commit (ADR-0068's commit-panel route).
    ///
    /// Derived from the session's operation observation, never stored. As a
    /// `merge_commit_ready` flag it was set by conflict detection and cleared
    /// by the next one, so anything that dropped the conflict view — the
    /// `MergeResolvedReady` branch itself — decided the answer. #704: the
    /// repository is what knows, and it says so from the first accepted read.
    pub fn merge_commit_ready(&self) -> bool {
        self.view().operation.as_ref().is_some_and(|op| {
            op.kind()
                == kagi_domain::conflict_family::ConflictOperationKind::Repository(
                    kagi_domain::plan_note::InProgressOp::Merge,
                )
                && op.unmerged() == 0
        })
    }

    /// Read the active session's presentation state. Welcome rendering receives
    /// an immutable default; no writer can put a resource into that value.
    pub fn ui(&self) -> &TabUiState {
        self.active_session()
            .and_then(|session| self.ui.get(&session))
            .unwrap_or(&self.ui_default)
    }

    /// The active session's writer, or `None` when no session owns the screen —
    /// the Welcome screen, a failed repository open, or the gap between the last
    /// tab closing and the next opening. Resource-bearing state must never use a
    /// detached sink, so mutation is *rejected* by returning `None` rather than
    /// crashing (ADR-0197 決定 2): every caller writes only inside `if let Some`.
    /// Background completion must instead use its frozen owner with
    /// `ui.get_mut`, never this foreground accessor.
    pub fn ui_mut(&mut self) -> Option<&mut TabUiState> {
        let session = self.active_session()?;
        self.ui.get_mut(&session)
    }

    /// Apply `f` to the active session's state, or do nothing when no session
    /// owns the screen. The one-line form of [`KagiApp::ui_mut`]'s `Option` for
    /// a plain field write whose only no-owner behaviour is "don't write"
    /// (#722 P1). Anything that must also *act* on the no-owner case keeps the
    /// explicit `match` / `let else` on `ui_mut` instead.
    /// `R` is discarded, so a one-expression writer whose call returns a value
    /// (`HashMap::insert`) still needs no block.
    pub(crate) fn with_ui<R>(&mut self, f: impl FnOnce(&mut TabUiState) -> R) {
        if let Some(ui) = self.ui_mut() {
            f(ui);
        }
    }

    /// What "Branch from here" acts on: the selected commit, or HEAD when
    /// nothing is selected. The header button and the `branch.new` command had
    /// the same chain spelled out twice; they now ask the session once.
    pub fn selected_or_head_commit(&self) -> Option<CommitId> {
        let details = &self.view().details;
        self.ui()
            .selected
            .and_then(|row| details.get(row))
            .or_else(|| details.first())
            .map(|detail| CommitId(detail.full_sha.to_string()))
    }

    /// Open a display slot for `path` and give it its UI state — the attach half
    /// of ADR-0197 決定 2's single lifecycle seam.
    ///
    /// `Sessions::attach` unifies aliases, so it can hand back a session a tab
    /// already holds. `entry` is what keeps that case from clearing the existing
    /// tab's selection: an attach that resolves to a live session initializes
    /// nothing.
    pub fn attach_session(&mut self, path: std::path::PathBuf) -> crate::app::SessionId {
        let session = self.app_sessions.attach(path);
        self.ui.entry(session).or_default();
        session
    }

    /// Same tab slot, fresh incarnation (a remote re-snapshot). The old
    /// incarnation expires exactly as it would on close — including its UI
    /// state, which is why this goes through [`KagiApp::release_session`]
    /// instead of forgetting the read on its own.
    pub(crate) fn reattach_session(
        &mut self,
        session: crate::app::SessionId,
        path: std::path::PathBuf,
    ) -> crate::app::SessionId {
        self.release_session(session);
        let next = self.app_sessions.reattach(session, path);
        self.ui.entry(next).or_default();
        next
    }

    /// End a session and everything that owner retained.
    ///
    /// #482 stage 1 drops the conflict/follow-up payloads and the plan slot if
    /// this session owned one, while in-flight executions keep running
    /// (ADR-0175); stage 2 adds the read model, which has the same lifetime.
    /// #643 Wave 4 makes this the sole destruction boundary for the UI entry
    /// and all repository-bound resources it owns. Removing the read or UI
    /// anywhere else would violate `dom(ui) = attached sessions`.
    pub(crate) fn release_session(&mut self, session: crate::app::SessionId) {
        self.app_sessions.detach(session);
        self.reads.forget(session);
        self.ui.remove(&session);
        self.pending_pull_confirm.remove(&session);
        if let Some(flight) = &mut self.fetch_in_flight {
            flight.waiters.retain(|waiter| *waiter != session);
        }
    }

    /// `Some(label)` while the tab on screen is waiting for its **first** read —
    /// the `Loading <name>…` placeholder the main pane shows instead of an empty
    /// graph.
    ///
    /// Derived from the owner's request slot, never stored (#482 stage 2 review,
    /// item 2). As a field it was set by the tab switch and cleared by the load
    /// that switch started, so a reload during that first load — a perfectly
    /// legal Cmd+R — refused the first read and then had nothing that cleared
    /// the placeholder: it stayed forever. Whichever read settles, success or
    /// failure, empties the slot, so the placeholder cannot outlive the request
    /// that put it there.
    pub fn loading_tab(&self) -> Option<SharedString> {
        let session = self.active_session()?;
        let waiting = self.reads.is_loading(session) && !self.reads.has_read(session);
        let name = &self.tabs.get(self.active_tab)?.name;
        waiting.then(|| SharedString::from(crate::ui::i18n::loading_fmt(name)))
    }

    /// In-place update of the read model on screen — a status-only WIP refresh,
    /// a solo toggle, the branch-cleanup rows, the squash ghost edges. Not a new
    /// read: it mutates the owner's existing allocation rather than rebuilding
    /// it, which is why a staging keystroke no longer copies every commit row.
    pub fn view_mut(&mut self) -> &mut TabViewState {
        let session = self.active_session();
        self.reads.get_mut(session)
    }

    /// #482 stage 2: open the bootstrap tab for a launch that has no `Context`
    /// yet (CLI argument, offscreen E2E mount). Attaches the session, pushes the
    /// tab, then publishes the read it was built from — the same order every
    /// other open follows, so the read model never exists without an owner.
    pub(crate) fn open_initial_tab(
        &mut self,
        path: &std::path::Path,
        name: &str,
        is_worktree: bool,
        view: TabViewState,
    ) {
        let session = self.attach_session(path.to_path_buf());
        self.tabs.push(crate::ui::tabs::RepoTab {
            session,
            path: path.to_path_buf(),
            name: name.to_string(),
            remote: None,
            is_worktree,
            wt_color_idx: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.publish_tab_view(session, view);
    }

    /// The commit `session` has selected, taken **before** a new read replaces
    /// its rows.
    ///
    /// A row index is not stable across a rebuild, so a landing read has to
    /// carry the selection by `CommitId` or it silently re-points at whichever
    /// commit inherited that index. ADR-0197 決定 3: a retained value is not
    /// authoritative against a read it has not been revalidated by — and the
    /// owner of the read is not necessarily the tab on screen, so this is keyed
    /// by `session` rather than reached through [`KagiApp::ui`].
    fn selected_commit(&self, session: crate::app::SessionId) -> Option<CommitId> {
        let row = self.ui.get(&session)?.selected?;
        let detail = self.reads.get(Some(session)).details.get(row)?;
        Some(CommitId(detail.full_sha.to_string()))
    }

    /// Put `anchor` back on the read that just landed for `session`: the same
    /// commit's new row index, or no selection at all when that commit is gone
    /// from the graph.
    ///
    /// Runs for the **owner**, active or not. A background tab's revalidate
    /// renumbers its rows exactly as an active tab's does, and leaving its
    /// `selected` on the old index is how returning to it would show a
    /// different commit selected — and dispatch checkout / cherry-pick / revert
    /// at that one.
    fn reanchor_selection(&mut self, session: crate::app::SessionId, anchor: Option<CommitId>) {
        let row = anchor.and_then(|id| {
            self.reads
                .get(Some(session))
                .commit_row_index
                .get(&id)
                .copied()
        });
        if let Some(state) = self.ui.get_mut(&session) {
            state.selected = row;
        }
    }

    /// Amend the owner's read model **without** superseding a read in flight —
    /// commit-graph paging, which refines what is on screen rather than
    /// observing the repository afresh. A pending full reload still lands and
    /// still does its conflict re-detection and status baseline update.
    pub fn amend_tab_view(&mut self, session: crate::app::SessionId, view: TabViewState) {
        let anchor = self.selected_commit(session);
        self.reads.amend(session, view);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = crate::ui::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(session, anchor);
        self.on_view_published(session);
    }

    /// Publish a freshly-built read model for `session` (bootstrap, remote
    /// snapshot) — supersedes anything in flight for that owner.
    pub fn publish_tab_view(&mut self, session: crate::app::SessionId, view: TabViewState) {
        let anchor = self.selected_commit(session);
        self.reads.publish(session, view);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = crate::ui::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(session, anchor);
        self.on_view_published(session);
    }

    /// Publish the completion of a read that was started with
    /// [`crate::app::Reads::begin`]. `false` = superseded: nothing was written
    /// and the caller must drop the result without any display side effect.
    pub fn accept_tab_view(&mut self, key: crate::app::ReadKey, view: TabViewState) -> bool {
        let anchor = self.selected_commit(key.session());
        if !self.reads.accept(key, view) {
            return false;
        }
        if let Some(ui) = self.ui.get_mut(&key.session()) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = crate::ui::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(key.session(), anchor);
        self.on_view_published(key.session());
        true
    }

    /// The UI's reaction to a *different* read model being on screen — a new one
    /// published for the active owner, or a tab switch to another owner. Not
    /// called when a background tab's read lands: that owner's data is stored,
    /// but the active tab's diff panes and caches must not be touched.
    pub(crate) fn on_view_published(&mut self, session: crate::app::SessionId) {
        // #704: the in-progress operation is part of the read, so its owner
        // learns it the moment the read lands — for a background tab too, and
        // before any conflict editor exists. Admission used to wait for
        // `apply_conflict_detect` to observe it, which is why a repository
        // opened mid-merge had no abort until something built a `ConflictView`
        // that a resolved merge never gets. (`observe_conflict` declines while
        // a write of its own is in flight, so this cannot race one.)
        let observed = self
            .reads
            .get(Some(session))
            .operation
            .as_ref()
            .map(|operation| operation.observation.clone());
        self.app_sessions.observe_conflict(session, observed);
        if self.active_session() != Some(session) {
            return;
        }
        // T-PERF-RENDER-002: a fresh view may change branches/tags/stashes/
        // worktrees, so invalidate the sidebar-rows cache fingerprint.
        self.view_epoch = self.view_epoch.wrapping_add(1);
        // The background scans write into the read model (cleanup rows, squash
        // ghost edges) and a fresh view has neither — `build_tab_view` copies
        // `snap.cleanup_rows`, which the snapshot always leaves empty. Every
        // apply therefore erases whatever the last scan produced. Flagging it
        // here rather than at the call sites means no future apply site can
        // forget; `render` re-arms the scans on the next frame. (This method
        // has no `cx`, and one of its callers runs before a `cx` exists.)
        self.scans_stale = true;

        // Issue #286: `diff_caches` (and the commit inspector's changed-file
        // menu) are keyed by COMMIT ROW INDEX. A fresh view renumbers rows, so a
        // stale entry would show one commit's changed-file list under another
        // commit's row (and a right-click Discard/menu would hit the wrong file).
        // Centralize the invalidation here — the same reason `view_epoch` /
        // `scans_stale` live here — so no apply site can forget it. Callers still
        // re-resolve `selected` by CommitId (there is no `cx` here).
        self.invalidate_caches_for_row_renumber(session);

        // Tie a worktree tab's colour to its WIP-row colour: the WIP row uses
        // lane_color(rank-in-worktrees-list), so record the same rank on the tab.
        let wt_idx = self.view().worktrees.iter().position(|w| w.is_current);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.is_worktree {
                tab.wt_color_idx = wt_idx;
            }
        }
    }

    /// The read model on screen changed owner (a tab switch). Same UI reaction
    /// as publishing a new read for the active owner (row-index caches, sidebar
    /// fingerprint, background scans) — see [`KagiApp::on_view_published`] — but
    /// a switch re-activates an *existing* read whose commit rows are not
    /// renumbered, so the retained main diff / compare pane stay valid and must
    /// survive that method's row-renumber sweep (ADR-0197 決定 3). Their derived
    /// caches are revalidated separately by `begin_session_revalidation` plus
    /// the activation full read.
    pub(crate) fn on_view_switched(&mut self) {
        let Some(session) = self.active_session() else {
            return;
        };
        let main_diff = self.ui().main_diff.clone();
        let compare_view = self.ui().compare_view.clone();
        self.on_view_published(session);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.main_diff = main_diff;
            ui.compare_view = compare_view;
        }
    }
}
