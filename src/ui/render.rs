//! Presentation / render layer for `KagiApp`.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 1, P1): the
//! `impl Render for KagiApp`, the `render_*` view-construction methods, and the
//! free `render_*` presentation helpers. Behaviour is unchanged — this is a pure
//! physical split. Per Rust's visibility rules a child module (`crate::ui::render`)
//! can access the private fields and private methods of `KagiApp` defined in its
//! ancestor module `crate::ui`, so these methods move with no visibility change.

#![allow(clippy::too_many_arguments)]

use super::*;

impl KagiApp {
    /// Typed external drops never participate in branch/file internal drags.
    /// The out-of-flow frame paints feedback without resizing workspace panes.
    pub(super) fn repository_drop_target(
        &self,
        content: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .relative()
            .size_full()
            .group("repository-drop")
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                this.open_external_paths(paths.paths(), cx);
            }))
            .child(content)
            .child(
                div()
                    .group("repository-drop-highlight")
                    .absolute()
                    .inset_0()
                    .size_full()
                    .border_2()
                    .border_color(gpui::rgba(0))
                    .can_drop(|drag, _, _| {
                        drag.downcast_ref::<gpui::ExternalPaths>()
                            .is_some_and(|paths| paths.paths().iter().any(|path| path.is_dir()))
                    })
                    .group_drag_over::<gpui::ExternalPaths>("repository-drop", |style| {
                        style.border_color(rgb(theme().ui_accent()))
                    }),
            )
            .child(
                gpui::canvas(
                    |_, _, _| (),
                    |_, _, window, _| {
                        // GPUI clears the drag on Exited but does not invalidate
                        // its last painted feedback. Observe without consuming it.
                        window.on_mouse_event(|event: &gpui::FileDropEvent, phase, window, _| {
                            if phase.bubble() && matches!(event, gpui::FileDropEvent::Exited) {
                                window.refresh();
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .into_any()
    }

    fn render_commit_menu_overlay(
        &self,
        state: CommitMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let detail = self.view().details.get(state.row_index)?;
        let target = self.commit_id_for_row(state.row_index)?;
        let ctx = self.menu_context_at(state.row_index, state.is_ancestor_of_head)?;
        let groups = context_menu::build_commit_menu(&ctx);
        let title = detail.full_message.as_ref().lines().next().unwrap_or("");
        let header = context_menu::short_title_header(detail.full_sha.as_ref(), title);
        Some(context_menu::render_commit_menu_overlay(
            state,
            target,
            header,
            groups,
            &self.menu_keys,
            self.window_active_for_render(window),
            window,
            cx,
        ))
    }

    fn render_branch_menu_overlay(
        &mut self,
        state: BranchMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        self.start_branch_menu_merge_check(cx);
        let ctx = self.branch_menu_context(&state);
        let groups = branch_menu::branch_context_menu_items(&ctx);
        let header = branch_menu::header(&ctx);
        Some(branch_menu::render_branch_menu_overlay(
            state,
            header,
            groups,
            &self.menu_keys,
            self.window_active_for_render(window),
            window,
            cx,
        ))
    }
    fn render_worktree_menu_overlay(
        &self,
        state: worktree_menu::WorktreeMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let groups =
            worktree_menu::build_worktree_menu(state.locked, state.is_main, state.path.as_deref());
        let header = SharedString::from(state.name.clone());
        Some(worktree_menu::render_worktree_menu_overlay(
            state,
            header,
            groups,
            &self.menu_keys,
            self.window_active_for_render(window),
            window,
            cx,
        ))
    }

    fn render_tag_menu_overlay(
        &self,
        state: tag_menu::TagMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let groups = tag_menu::build_tag_menu(state.remote.as_deref());
        let header = SharedString::from(state.name.clone());
        Some(tag_menu::render_tag_menu_overlay(
            state,
            header,
            groups,
            &self.menu_keys,
            self.window_active_for_render(window),
            window,
            cx,
        ))
    }

    fn render_stash_menu_overlay(
        &self,
        state: stash_menu::StashMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let groups = stash_menu::build_stash_menu();
        let header = SharedString::from(format!("stash@{{{}}}: {}", state.index, state.message));
        Some(stash_menu::render_stash_menu_overlay(
            state,
            header,
            groups,
            &self.menu_keys,
            self.window_active_for_render(window),
            window,
            cx,
        ))
    }
}

impl Render for KagiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window_presentation(window, cx);
        self.poll_app_jobs(cx);
        self.present_app_notice();
        // #914: modal button closures bypass root Enter/Escape. A real exit
        // left a pending release; when the last card leaves the slot, schedule
        // its owner-scoped Git plan after this render (never during painting).
        self.defer_pending_auto_release(cx);
        // ADR-0121 B2: promote a headless-staged diff (KAGI_OPEN_FIRST_FILE
        // runs before any gpui context exists) into the pane entity on the
        // first frame. Always `None` in the GUI paths.
        // KAGI_MD_PREVIEW: open the Editor on a Markdown file with preview on.
        if let Some(path) = self.pending_headless_md_preview.take() {
            self.open_editor_workspace(cx);
            if let Some(ev) = self.ui().editor_workspace.clone() {
                ev.update(cx, |v, cx| {
                    v.open_tab(path, cx);
                    v.set_markdown_preview(true, cx);
                });
            }
        }
        if let Some(view) = self.pending_headless_diff.take() {
            self.show_main_diff(view, cx);
        }
        // Same promotion for a headless-staged compare (KAGI_COMPARE_HEAD /
        // KAGI_COMPARE_WT run before any gpui context exists).
        if let Some(view) = self.pending_headless_compare.take() {
            self.show_compare(view, cx);
        }

        // A published read owes the retained panes a pass (#722; no-op unless
        // queued), and a replaced view re-arms the scans that decorate it.
        self.revalidate_retained_panes(cx);
        self.sync_oplog_draft_owner(cx);
        if self.scans_stale {
            self.scans_stale = false;
            self.start_branch_cleanup_scan(cx);
            self.start_squash_link_scan(cx);
            self.start_wip_diffstat_scan(cx);
        }

        // W27-UIPOLISH: apply the global UI zoom by scaling the window's rem
        // size. gpui's `text_*` helpers and rem-based lengths resolve through
        // `rem_size()`, so this zooms virtually all of kagi's text/layout like
        // a web-page zoom.
        //
        // This re-asserts the zoomed rem size every frame, by design. `KagiApp`
        // is a child view of `gpui_component::Root`, whose `Root::render` runs
        // first in gpui's interleaved render→layout walk and calls
        // `window.set_rem_size(cx.theme().font_size)` — a fixed `px(16.)` that is
        // NOT zoom-aware (kagi's `sync_gpui_component_theme` maps colours and font
        // families, never `font_size`). KagiApp::render runs immediately after,
        // so re-asserting here is what actually scales kagi's text. The earlier
        // `last_rem_size` guard (T-PERF-RENDER-002) skipped the re-assert on every
        // steady-state frame, letting Root's fixed 16px win — layout scaled via
        // `theme::scaled_px` but text stayed pinned at 16px (text/graph drifted
        // apart on zoom). Compare against the *live* window value (which Root just
        // reset) so the write is skipped only when genuinely a no-op (zoom == 1.0
        // ⇒ both already 16px).
        let rem_size = px(theme::rem_size_px());
        if window.rem_size() != rem_size {
            window.set_rem_size(rem_size);
        }

        // #454: publish this frame's window height so the modal list boxes can
        // cap themselves against the window instead of a fixed row count.
        theme::set_viewport_h(f32::from(window.viewport_size().height));
        theme::set_viewport_w(f32::from(window.viewport_size().width));
        self.sync_compact_layout(window, cx);

        // Auto-update (ADR-0082): kick the run-once background version check.
        self.ensure_update_check(cx);

        // W2-STATUS / ADR-0017: resolve the bottom-panel default height on
        // first render, once the viewport size is known (18% of viewport).
        if self.bottom_panel_height <= BOTTOM_PANEL_H_UNSET {
            let viewport_h = f32::from(window.viewport_size().height);
            let h = (viewport_h * BOTTOM_PANEL_DEFAULT_FRAC).max(BOTTOM_PANEL_MIN_H);
            self.bottom_panel_height = h;
            eprintln!(
                "[kagi] bottom-panel: default height={:.0} ({:.0}% of viewport {:.0})",
                h,
                BOTTOM_PANEL_DEFAULT_FRAC * 100.0,
                viewport_h
            );
        }
        self.window_active = self.window_active_for_render(window);
        theme::sync_window_selection_theme(self.window_active, cx);
        if self.window_activation_subscription.is_none() {
            self.window_activation_subscription =
                Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }

        // W11-AVATAR: kick off GitHub avatar resolution once per repo (no-op
        // for non-GitHub repos / offline / already-started).
        self.ensure_avatars(cx);

        // T-PERF-RENDER-001 (ADR-0116 Wave 2): conflict detection, reflog seeding,
        // and the auto-fetch ticker no longer run here.  `render()` must stay pure
        // (no synchronous Git/index/file I/O on the UI thread), so those moved to
        // the reload / tab-switch / app-init commit points via `background_spawn`
        // + marshal-back (`ensure_startup_repo_io`, armed by `switch_repo` and
        // `open_main_window`).  The watcher and post-operation paths still force
        // re-detection through the synchronous `reload()`.

        // W3-NOTIFY: the toast auto-dismiss ticker now lives on the
        // `ToastStack` entity and is (re)started by `push_notify`, so KagiApp's
        // render no longer needs to nudge it (ADR-0110 Phase 5).

        // Modal text inputs: lazy-create + sync (needs Window).
        // Input task frames share one persistent trap handle, including cards
        // whose opener historically relied only on InputState's own focus.
        if self.has_active_modal() && self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        self.sync_modal_inputs(window, cx);
        // The PR page's comment composer, for the same reason (ADR-0200).
        self.sync_pr_comment_input(window, cx);
        self.sync_pr_fields_input(window, cx);
        self.sync_issue_inputs(window, cx);
        self.sync_list_filter_input(window, cx);
        self.sync_sidebar_filter_placeholder(window, cx);
        self.sync_pending_focus(window, cx);
        // GPUI can retain focus on an input whose modal was just unmounted.
        // Only a handle tracked by the last drawn dispatch tree may hold the
        // queue; the next frame releases a head after an Escape dismissal.
        use gpui::Focusable as _;
        use gpui_component::WindowExt as _;
        self.op_queue.observed_input_focused = window.has_focused_input(cx)
            && window.focused_input(cx).is_some_and(|input| {
                let handle = input.read(cx).focus_handle(cx);
                handle.is_focused(window)
                    && self
                        .root_focus
                        .as_ref()
                        .is_some_and(|root| root.contains(&handle, window))
            });
        // Home's rows and its Repositories / Pull requests / Issues switch
        // are drawn only with Home in front. Every way off it (⌘W, a
        // repository tab, a row that opens its clone) leaves a focus on them
        // with nothing tracking it, out of the window's keys: the focus goes
        // to the window (#961 review). The row is still remembered for when
        // Home is back.
        if !self.home_in_front() {
            let fallback = self.root_focus.clone();
            self.home_github
                .row_focus
                .yield_focus(fallback.as_ref(), window, cx);
            self.home_github
                .recent
                .keyboard
                .yield_focus(fallback.as_ref(), window, cx);
            self.home_github
                .work
                .pane_focus
                .yield_focus(fallback.as_ref(), window, cx);
        } else {
            // The same for the Graph sidebar's rows and headers (#981).
            self.yield_sidebar_focus(window, cx);
        }
        // Share the actual body route with presentation lifetime retirement.
        // Preserve the Home/conflict short-circuit when there is no repo owner.
        let center = if self.home_in_front() || self.conflict_body_visible() {
            None
        } else {
            Some(
                super::workspace::resolve_workspace(&self.workspace_inputs(false, false, false))
                    .center,
            )
        };
        let workspace_changed = self.sync_workspace_motion_visibility(center, cx);
        // None of a retained pane's focus owners belong to the next dispatch
        // tree while hidden, including navigation and source-list controls.
        if workspace_changed
            && self.ui().conflict_hidden
            && self
                .ui()
                .conflict
                .as_ref()
                .is_some_and(|pane| pane.read(cx).contains_focused_any(window, cx))
        {
            if let Some(root) = &self.root_focus {
                root.focus(window, cx);
            }
        }
        // A hidden Graph stop must not strand focus after a workspace takeover.
        let graph_hidden = center != Some(super::workspace::CenterPane::CommitList);
        if graph_hidden
            && self
                .graph_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
        {
            if let Some(root) = &self.root_focus {
                root.focus(window, cx);
            }
        }
        if !self.pr_mode_visible() || self.home_in_front() {
            if let Some(mode) = self.pr_mode() {
                mode.detail_focus
                    .yield_focus(self.root_focus.as_ref(), window, cx);
            }
        }
        let active = self.active_session();
        let main_issues = !self.home_in_front()
            && self.workspace_mode() == super::workspace_mode::WorkspaceMode::Issues;
        let mut focused_pdf = None;
        for (owner, ui) in &self.ui {
            if let Some(editor) = &ui.editor_workspace {
                let visible =
                    Some(*owner) == active && center == Some(super::workspace::CenterPane::Editor);
                editor.update(cx, |editor, cx| {
                    editor.release_hidden_pdf_focus(visible, window, cx);
                });
                if let Some(focus) = editor.read(cx).focused_pdf_body(window) {
                    focused_pdf = Some(focus.clone());
                }
            }
            if let Some(diff) = &ui.main_diff {
                if let Some(focus) = diff.read(cx).focused_pdf_body(window, cx) {
                    focused_pdf = Some(focus);
                }
            }
            if Some(*owner) != active || self.home_in_front() {
                ui.inspector_model
                    .yield_focus(self.root_focus.as_ref(), window, cx);
            }
            let composer_focused = ui
                .issue_composer
                .editors
                .get(&None)
                .is_some_and(|editor| editor.focused);
            if Some(*owner) != active
                || !main_issues
                || ui.selected_github_issue.is_some()
                || composer_focused
            {
                ui.issue_rows
                    .yield_focus(self.root_focus.as_ref(), window, cx);
            }
        }
        if let Some(focus) = focused_pdf {
            // All retained owners participate, not only the foreground pane.
            // Closing/replacing either PDF surface may drop it before next render.
            self.track_retiring_focus(focus, window, cx);
        }
        // Before any tab list picks its Tab stop this frame (#968).
        super::keyboard_nav::forget_roving_without_focus(window);

        if std::env::var("KAGI_DEBUG_RENDER").as_deref() == Ok("1") {
            use std::sync::atomic::{AtomicU64, Ordering as O};
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, O::Relaxed) + 1;
            if n.is_multiple_of(50) {
                klog!("render: {} frames", n);
            }
        }

        // ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001 (corrections #1/#2): the
        // queued smart-commit message push AND the per-branch draft autosave moved
        // ONTO the `CommitPanelView` entity (`sync_inputs`), run from the entity's
        // own render path. The parent no longer reads the child's input each frame.

        // Keep the paging action outside the ListBox: it is a Button, not an
        // Option in the virtual commit list.
        let commit_limit = self.ui().commit_limit;
        let has_more_commits = commit_limit > 0 && self.view().rows.len() >= commit_limit;
        let row_count = self.view().rows.len();
        let selected = self.ui().selected;

        // W4-TABS / ADR-0028: a non-empty error string still shows the error
        // screen (genuine repo-open failure at startup; headless log compat).
        if let Some(err) = self.error.clone().filter(|e| !e.is_empty()) {
            // ── Error / usage state ──────────────────────────
            // Merge: keep the platform window shell (Linux titlebar/menu) from
            // our branch AND the bundled UI font from origin.
            return self.platform_window_shell(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .font_family(UI_FONT)
                    .bg(rgb(theme().bg_base))
                    .child(
                        div()
                            .text_xl()
                            .text_color(rgb(theme().text_main))
                            .child(err),
                    )
                    .into_any(),
                cx,
            );
        }

        // W4-TABS / ADR-0219: Home — in front of the tabs, or the whole window
        // when none is open. A remote read-only view (ADR-0089 Phase 2b) has no
        // local tab but still renders the workspace from its applied snapshot.
        if self.home_in_front() {
            self.panel_motion.reset_right();
            let home = self.render_home(window, cx);
            return self.platform_window_shell(home, cx);
        }

        // ADR-0089 Phase 2c: in a remote view, lazily load the selected commit's
        // changed files over SSH (once per row). The render trigger covers every
        // selection path (click / keyboard / jump) uniformly.
        if self.remote_view.is_some() {
            if let Some(i) = selected {
                if !self.ui().diff_caches.changed_files().contains_key(&i)
                    && !self.ui().diff_caches.remote_inflight.contains(&i)
                {
                    self.load_remote_changed_files(i, cx);
                }
            }
        } else if let Some(i) = selected {
            // ADR-0107 / perf: local view — lazily load the selected commit's
            // changed files + diffstat off the UI thread (once per row), so no
            // selection path (click / keyboard / jump) blocks the frame. `select`
            // only records the selection; this fires the async load.
            if !self.ui().diff_caches.changed_files().contains_key(&i)
                && !self.ui().diff_caches.local_inflight.contains(&i)
            {
                self.load_local_changed_files(i, cx);
            }
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected.and_then(|i| self.view().details.get(i)).cloned();
        // ADR-0121 B2: the changed-files / diffstat / badges / compare inputs
        // for the Inspector are re-derived by `workspace::InspectorItem` in
        // its render — render_body no longer takes them.
        let wip_diffstat = self.ui().wip_diffstat;

        // ADR-0121 B2 (merge): both clones gone — the scroll handle lives in
        // MainDiffPane, and the Inspector re-derives compare inputs itself.

        // Clone modal state for render.
        let is_dirty = self.view().is_dirty;
        // PERF-SIDEBAR-VIRT: the navigator data (branches/remotes/tags/…) is
        // not cloned for the sidebar renderer — it is flattened into
        // `self.sidebar.rows` below and read by the virtualized list processor.
        // The renderer derives its own inputs from `self` (ADR-0199), so only
        // the filter *text* is needed here, for the rebuild fingerprint.
        // The five Graph pane lists share these rows and rebuild their ranges
        // only when the fingerprint below changes (#864). Branch-row PR badges
        // read `github_prs` live when a visible row is drawn, so PR refreshes
        // never rebuild the rows.
        //
        // ADR-0116: owner + `view_epoch` and navigator inputs gate the
        // O(all-refs) clone+collect.
        let sidebar_filter_text: String = self
            .sidebar
            .filter
            .as_ref()
            .map(|ent| ent.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let sidebar_fingerprint = sidebar_rows::sidebar_rows_fingerprint(
            self.active_session(),
            self.view_epoch,
            self.view().branches.len(),
            self.view().remote_branches.len(),
            self.view().tags.len(),
            self.view().stashes.len(),
            self.view().worktrees.len(),
            &self.sidebar.collapsed,
            &self.ui().branch_groups_collapsed,
            &sidebar_filter_text,
        );
        if sidebar_fingerprint != self.sidebar.rows_fingerprint {
            let rows = sidebar_rows::build_sidebar_rows(
                &self.view().branches,
                &self.view().remote_branches,
                &self.view().tags,
                &self.view().stashes,
                &self.view().worktrees,
                // Read when the rows are rebuilt: a mode change shows at the
                // next rebuild (refresh), not on its own.
                crate::ui::settings::Settings::load().worktree_run_mode(),
                &self.sidebar.collapsed,
                &self.ui().branch_groups_collapsed,
                &sidebar_filter_text,
            );
            let owner = self.active_session();
            let ready = owner.is_some_and(|session| self.reads.has_read(session));
            self.sidebar
                .prepare_rows_motion(&rows, owner, &sidebar_filter_text, ready);
            self.sidebar.pane_ranges = sidebar_panes::pane_ranges(&rows);
            self.sidebar
                .focus
                .set_keys(&rows, &self.sidebar.pane_ranges);
            self.sidebar.rows = rows;
            self.sidebar.rows_fingerprint = sidebar_fingerprint;
        }
        self.sidebar.advance_motion(window, cx);
        let status_footer = self.status_footer.clone();
        // ADR-0118 / T-ENTITY-CONFLICT-001: the conflict body is its own
        // `Entity<ConflictView>`. The entity renders itself (`el.child(entity)`);
        // the banner is a free function fed a cloned `ConflictMode` read out of
        // the entity here so the entity is never rendered twice in one frame.
        let conflict_entity = self.ui().conflict.clone();
        // T-CONFLICT-FLOW-030: while a continued merge waits for its commit
        // message, show the normal body (commit panel) instead of the conflict
        // resolution body (ADR-0068). Conflict Mode is still active (MERGE_HEAD
        // present) but the editor is hidden behind the commit message panel.
        let conflict_body_visible = self.conflict_body_visible();
        // Conflict Mode replaces the body, the sidebar's panes with it; the
        // body's frames sync them as they lay the sidebar out (#981).
        if conflict_body_visible {
            self.panel_motion.reset_right();
            self.yield_sidebar_focus(window, cx);
        }
        // #985: a menu that just opened takes the focus; one that closed
        // gives it back. The selected row records where it is drawn anew.
        self.sync_menu_keys(window, cx);
        self.sync_file_menu_keys(window, cx);
        self.sync_pr_menu_keys(window, cx);
        self.context_anchor.clear();
        let commit_menu_overlay = self
            .commit_menu
            .clone()
            .filter(|_| self.commit_menu_visible())
            .and_then(|state| self.render_commit_menu_overlay(state, window, cx));
        let branch_menu_overlay = self
            .branch_menu
            .clone()
            .filter(|_| self.branch_menu_visible())
            .and_then(|state| self.render_branch_menu_overlay(state, window, cx));
        let tag_menu_overlay = self
            .tag_menu
            .clone()
            .filter(|_| self.tag_menu_visible())
            .and_then(|state| self.render_tag_menu_overlay(state, window, cx));
        let stash_menu_overlay = self
            .stash_menu
            .clone()
            .filter(|_| self.stash_menu_visible())
            .and_then(|state| self.render_stash_menu_overlay(state, window, cx));
        let worktree_menu_overlay = self
            .worktree_menu
            .clone()
            .filter(|_| self.worktree_menu_visible())
            .and_then(|state| self.render_worktree_menu_overlay(state, window, cx));
        // Per-file and Editor tree menus render on the root, not inside the
        // entity, so their actions can dispatch without leasing that entity.
        let conflict_file_menu_overlay = if self.conflict_file_menu_visible(cx) {
            conflict_entity.as_ref().and_then(|entity| {
                let view = entity.read(cx);
                let (Some((idx, pos)), Some(mode)) = (view.file_menu, view.mode.clone()) else {
                    return None;
                };
                Some(conflict_view::render_file_menu(
                    entity,
                    &mode,
                    idx,
                    pos,
                    self.window_active,
                    window,
                    cx,
                ))
            })
        } else {
            None
        };
        let editor_tree_menu_overlay = if self.editor_tree_menu_visible(cx) {
            self.ui().editor_workspace.as_ref().and_then(|entity| {
                let (target, pos) = entity.read(cx).tree_menu?;
                editor_tree_menu::render_editor_tree_menu(
                    entity,
                    target,
                    pos,
                    self.window_active,
                    window,
                    cx,
                )
            })
        } else {
            None
        };
        // T-HT-001: clone toolbar/summary state for header render.
        // W3-NOTIFY: while a background git op runs, disable every git button
        // so operations never overlap.
        let mut toolbar_state = self.view().toolbar_state.clone();
        if self.op_latched() {
            toolbar_state.pull_on = false;
            toolbar_state.push_on = false;
            toolbar_state.stash_on = false;
            toolbar_state.pop_on = false;
            toolbar_state.undo_on = false;
        }
        let status_summary = self.view().status_summary.clone();

        // T028: clone scroll handle for wiring into uniform_list via track_scroll.
        let commit_scroll_handle = self.ui().commit_scroll_handle.clone();

        // T023: divider drag-move handler — the full per-divider math lives in
        // `handle_divider_drag` (extracted in T-SPLIT-RENDER-001); placed on the
        // root div so it fires even when the mouse leaves the 4px divider strip.
        let divider_drag_move = cx.listener(
            move |this, event: &gpui::DragMoveEvent<DividerDrag>, window, cx| {
                this.handle_divider_drag(event, window, cx);
            },
        );

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.ui().commit_panel_open;
        let commit_panel = self.ui().commit_panel.clone();
        // T-SPLIT-HELPERS-001 / ADR-0116 Wave 3: commit_input + template mode/inputs
        // are read directly from `self` inside `render_commit_panel` (now a `&self`
        // method), so they no longer need to be hoisted/threaded through render_body.

        // The footer keeps its window-wide bottom-panel toggle.
        let bottom_panel_open = self.bottom_panel_open;
        // #950: the panel slides toward `bottom_panel_open`, whoever set it.
        // The side panes follow their layout in `render_body` (#955); the
        // next frame is requested below, once all three have been synced.
        self.panel_motion.bottom.sync(
            bottom_panel_open,
            kagi_ui_core::motion::now(),
            kagi_ui_core::motion::instant(),
        );

        // T-BP-002: cmd-j toggle action handler.
        let toggle_bottom_panel = cx.listener(|this, _: &ToggleBottomPanel, window, cx| {
            this.toggle_bottom_panel(window, cx);
        });

        // T-WS-EDITOR-002: Cmd-S saves the Editor Workspace's dirty buffer.
        // No-ops when the workspace is closed or the buffer is clean.
        // Diff bodies handle their own focused, revision-valid selection.
        // The root never reads another surface's saved text.
        let copy_diff_selection = cx.listener(|this, _: &CopyDiffSelection, window, cx| {
            // Only the visible, uncovered Graph owns the root Copy action.
            // Focused diff bodies have already handled their valid selections.
            if this.commit_list_has_focus(window, cx) {
                if let Some(i) = this.ui().selected {
                    this.copy_graph_selection(i, cx);
                    return;
                }
            }
            // Preserve gpui-component's focused TextView Copy path.
            cx.propagate();
        });
        let diff_selection_copied = cx.listener(
            |this, copied: &diff_selection::keyboard::SelectionCopied, _window, cx| {
                this.push_toast(
                    ToastKind::Info,
                    SharedString::from(i18n::diff_lines_copied(copied.lines)),
                    cx,
                );
                cx.notify();
            },
        );

        let save_editor_file = cx.listener(|this, _: &SaveEditorFile, _window, cx| {
            this.save_editor_file(cx);
        });

        // ── Normal state: header + body + bottom panel slot + status bar ─────
        // Outer layers are composed here rather than in attach_modal_overlays:
        // their pane/body and window-shell parents cannot be attached by the
        // shared modal collector. Keep that boundary pinned to Z_ORDER.
        debug_assert_eq!(
            &super::front_layer::Z_ORDER[..5],
            &[
                super::front_layer::LayerKind::ConflictFileMenu,
                super::front_layer::LayerKind::EditorTreeMenu,
                super::front_layer::LayerKind::CoauthorMenu,
                super::front_layer::LayerKind::WorkspaceMenus,
                super::front_layer::LayerKind::MenuOverlay,
            ]
        );
        debug_assert_eq!(
            super::front_layer::Z_ORDER.last(),
            Some(&super::front_layer::LayerKind::PlatformMenu)
        );
        // Protect document keyboard ownership even with tree/root focus. A
        // pending file classification may still resolve to a signature-only PDF;
        // do not admit Git Undo/Redo before its content type is known.
        // Size-limit placeholders remain read-only even when classification
        // stops before a PDF document can be constructed.
        let editor_context = self
            .ui()
            .editor_workspace
            .as_ref()
            .filter(|_| {
                center == Some(super::workspace::CenterPane::Editor) && !self.has_active_modal()
            })
            .and_then(|editor| {
                let editor = editor.read(cx);
                if editor.file_loading && editor.editor.is_none() {
                    Some("EditorLoading")
                } else if editor.markdown_preview_visible() {
                    Some("EditorMarkdownPreview")
                } else {
                    (editor.pdf_surface_visible() || editor.content_too_large)
                        .then_some("EditorReadOnly")
                }
            });
        let root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .font_family(UI_FONT)
            .bg(rgb(theme().bg_base))
            // Tier A probe: counts wheel events that reach the workspace
            // behind every overlay, whichever pane is showing.
            .when(cfg!(feature = "gui-e2e"), |el| {
                el.on_scroll_wheel(|_, _, _| super::e2e::note_workspace_scroll())
            })
            .children(self.render_platform_titlebar(cx))
            .child(super::workspace_mode::sidebar_gesture_end(self, cx))
            // Key events only dispatch along the focus path, so the root must
            // own (and initially hold) focus for window-wide actions to work.
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            .when_some(editor_context, |el, context| el.key_context(context))
            // Block Tab in the menu's focus subtree if no binding handled it.
            // gpui-component's Root binds Tab before capture, so the matching
            // early interceptor in keyboard_nav handles that normal route.
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" && this.visible_platform_menu_section().is_some() {
                    if let Some(root) = &this.root_focus {
                        window.focus(root, cx);
                    }
                    cx.stop_propagation();
                }
            }))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // Targets override this default during capture; leaving any target
            // must not retain its prohibited cursor.
            .on_drag_move::<BranchDrag>(|_, window, cx| {
                if cx.active_drag_cursor_style() != Some(gpui::CursorStyle::OpenHand) {
                    cx.set_active_drag_cursor_style(gpui::CursorStyle::OpenHand, window);
                }
            })
            .capture_any_mouse_up(cx.listener(|this, event: &gpui::MouseUpEvent, _, _| {
                if event.button == MouseButton::Left {
                    this.finish_bottom_panel_resize();
                }
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.finish_bottom_panel_resize()),
            )
            // T-BP-002: cmd-j toggle action (window-wide via on_action on root div).
            .on_action(toggle_bottom_panel)
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    this.input_escape_started_marked =
                        kagi_ui_core::input_actions::focused_input_has_marked_text(window, cx);
                    cx.propagate();
                }),
            )
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Escape, _, cx| {
                    if !this.input_escape_started_marked {
                        cx.propagate();
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &super::FocusSidebarFilter, window, cx| {
                    this.focus_sidebar_filter(window, cx);
                }),
            )
            .when(
                {
                    let (input, sidebar) = self.find_menu_targets(window, cx);
                    input || sidebar
                },
                |el| {
                    el.on_action(cx.listener(
                        |this, _: &commands::FindInFocusedView, window, cx| {
                            let (input, sidebar) = this.find_menu_targets(window, cx);
                            if input {
                                window.dispatch_action(Box::new(gpui_component::input::Search), cx);
                            } else if sidebar {
                                window.dispatch_action(Box::new(super::FocusSidebarFilter), cx);
                            }
                        },
                    ))
                },
            )
            .on_action(copy_diff_selection)
            .on_action(diff_selection_copied)
            .when(self.editor_save_dispatch_available(cx), |el| {
                el.on_action(save_editor_file)
            })
            .on_action(
                cx.listener(|this, _: &menu_keys::ContextMenuKey, window, cx| {
                    this.open_context_menu_from_key(window, cx);
                }),
            )
            // Arrows: step diff files while the main diff is open, otherwise
            // move the commit selection (user request).
            .on_action(cx.listener(|this, _: &DiffPrevFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                // File-history view is its own full overlay with its own entry
                // list + diff pane — navigate that, not the main commit list.
                if this.ui().file_history.is_some() {
                    this.step_file_history_selection(-1, cx);
                } else if this.pr_mode_visible() {
                    this.pr_mode_step(-1, cx);
                } else if this.ui().ecosystem.is_none() && this.ui().editor_workspace.is_some() {
                    // T-WS-EDITOR-001 feedback: Editor mode steps its file
                    // tree — but only when it's actually the visible center.
                    this.step_editor_ws_selection(-1, window, cx);
                } else if this.ui().main_diff.is_some() {
                    this.main_diff_step(-1, cx);
                } else if this.commit_list_has_focus(window, cx) {
                    this.step_commit_selection(-1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DiffNextFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                if this.ui().file_history.is_some() {
                    this.step_file_history_selection(1, cx);
                } else if this.pr_mode_visible() {
                    this.pr_mode_step(1, cx);
                } else if this.ui().ecosystem.is_none() && this.ui().editor_workspace.is_some() {
                    this.step_editor_ws_selection(1, window, cx);
                } else if this.ui().main_diff.is_some() {
                    this.main_diff_step(1, cx);
                } else if this.commit_list_has_focus(window, cx) {
                    this.step_commit_selection(1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CommitFirst, window, cx| {
                if this.commit_list_has_focus(window, cx) {
                    this.jump_commit_selection(false);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CommitLast, window, cx| {
                if this.commit_list_has_focus(window, cx) {
                    this.jump_commit_selection(true);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CommitPageUp, window, cx| {
                if this.commit_list_has_focus(window, cx) {
                    this.step_commit_selection(-this.commit_page_size());
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &CommitPageDown, window, cx| {
                if this.commit_list_has_focus(window, cx) {
                    this.step_commit_selection(this.commit_page_size());
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &PrModePrevPane, window, cx| {
                if this.root_has_focus(window) && this.pr_mode_visible() {
                    this.pr_mode_cycle_focus(-1, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PrModeNextPane, window, cx| {
                if this.root_has_focus(window) && this.pr_mode_visible() {
                    this.pr_mode_cycle_focus(1, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CheckoutSelected, window, cx| {
                this.checkout_selected_commit(window, cx);
            }))
            // Menus and shortcuts share the same handler and focus/history gate.
            .when(self.history_menu_available(false, window, cx), |el| {
                el.on_action(cx.listener(|this, _: &commands::HistoryUndo, window, cx| {
                    if this.history_menu_available(false, window, cx) {
                        this.open_history_undo_modal();
                        cx.notify();
                    }
                }))
            })
            .when(self.history_menu_available(true, window, cx), |el| {
                el.on_action(cx.listener(|this, _: &commands::HistoryRedo, window, cx| {
                    if this.history_menu_available(true, window, cx) {
                        this.open_history_redo_modal();
                        cx.notify();
                    }
                }))
            })
            // ── W5-MENU / ADR-0029: conditional command handlers ──────────
            // Each menu action's handler is registered on the focused root ONLY
            // when `command_state == Enabled`.  gpui's macOS menu validation
            // (`is_action_available`, walks the dispatch tree) then greys out
            // any command whose handler is absent — the ADR-0029 disabled model.
            .map(|el| self.register_menu_actions(el, cx))
            // Document tabs occupy the native titlebar, even for one document.
            .child(self.render_tab_strip(window, cx))
            // ── Header slot ──────────────────────────────────
            // ADR-0013: pass HEAD commit summary for Undo label (first row = HEAD).
            .child(self.render_header_slot(toolbar_state, status_summary, cx))
            // #704 is the exit when the conflict sidebar is hidden or unmounted.
            // Admission remains owned by TabViewState::operation.
            .when(
                !conflict_body_visible
                    || conflict_entity
                        .as_ref()
                        .is_some_and(|view| view.read(cx).sidebar_hidden),
                |el| el.children(self.render_operation_strip(cx)),
            )
            // ── Body slot: in Conflict Mode the conflict resolution pane
            //    replaces the normal sidebar | list | panel body. `ConflictView`
            //    owns the collapsible conflict tree and the selected file's
            //    editor or read-only saved inspection.
            .when(conflict_body_visible, |el| {
                if let Some(entity) = conflict_entity.clone() {
                    let visible = self
                        .panel_motion
                        .bottom
                        .visible(kagi_ui_core::motion::now());
                    let height = self.bottom_panel_body_height(window, false);
                    // Conflict Mode replaces render_body, not the window-wide
                    // panel: preserved drafts must remain inspectable here.
                    let persistent_lane = self
                        .toast_stack
                        .as_ref()
                        .is_some_and(|stack| stack.read(cx).persistent_lane_open());
                    let lane = persistent_lane
                        .then(|| {
                            self.render_toasts(
                                window,
                                super::busy::ToastPlacement::PersistentLane,
                                true,
                                cx,
                            )
                        })
                        .flatten();
                    let notifications = self.render_toasts(
                        window,
                        super::busy::ToastPlacement::RepositoryTransient,
                        true,
                        cx,
                    );
                    entity.update(cx, |view, _| {
                        view.notification_lane = lane;
                        view.notifications = notifications;
                    });
                    el.child(entity).children(self.render_bottom_panel_slot(
                        visible,
                        height,
                        self.bottom_tab,
                        window,
                        cx,
                    ))
                } else {
                    el
                }
            })
            .when(!conflict_body_visible, |el| {
                el.child(self.render_body(
                    row_count,
                    has_more_commits,
                    selected,
                    detail,
                    is_dirty,
                    commit_scroll_handle,
                    commit_panel_open,
                    commit_panel.clone(),
                    wip_diffstat,
                    window,
                    cx,
                ))
            })
            .children(self.render_compact_composer(window, cx))
            // The body owns the Commit Panel coauthor popover. Keep these
            // two root-level pane popovers below workspace menus as in Z_ORDER.
            .children(conflict_file_menu_overlay)
            .children(editor_tree_menu_overlay)
            // ── Commit context menu overlay (below modals) ─────
            .children(commit_menu_overlay)
            // ── Branch context menu overlay (below modals) ─────
            .children(branch_menu_overlay)
            // ── Stash context menu overlay (below modals) ──────
            .children(stash_menu_overlay)
            .children(tag_menu_overlay)
            .children(worktree_menu_overlay)
            // ── W5-MENU: menu-driven overlay (branch picker / About / shortcuts) ──
            .children(self.render_menu_overlay(window, cx));

        // Keep drawing frames while a pane is still sliding (#950, #955).
        kagi_ui_core::motion::request_frame(
            window,
            self.panel_motion.animating(kagi_ui_core::motion::now()),
        );

        let root = root
            // ── Operation queue strip (#355) — the front tab's queued
            //    intents and cancel list, directly above the status bar. ──
            .children(self.render_queue_strip(window, cx))
            // ── Status bar slot (T017) — last operation result ─
            .child(self.render_status_bar(status_footer, bottom_panel_open, window, cx));
        // The one collector Home also uses keeps popup/modal ordering intact.
        // Attach it after the opaque footer, so window-fitted menus and modals
        // are not overpainted by workspace chrome.
        let root = self.attach_modal_layer(root, true, window, cx);
        let content = root
            // Linux/FreeBSD in-app menu dropdown (native menu bar is macOS-only).
            .children(self.render_platform_menu_dropdown(self.window_active, cx))
            .into_any();
        let content = self.attach_active_modal_key_routing(content, true, cx);
        self.repository_drop_target(content, cx)
    }
}
