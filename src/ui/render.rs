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
    /// Render the toast / busy overlay as an absolute container (bottom-left,
    /// above the status bar). The toast cards live in the `Entity<ToastStack>`
    /// child, so a push / expire re-renders only that subtree instead of all of
    /// `KagiApp` (ADR-0110 Phase 5). The busy snackbar stays here because it is
    /// driven by `busy_op` (KagiApp state). Returns `None` before the window
    /// (and thus the toast entity) exists.
    fn render_toasts(&self) -> Option<gpui::AnyElement> {
        let toast_stack = self.toast_stack.clone()?;
        let mut stack = div()
            .absolute()
            .bottom(theme::scaled_px(34.))
            .left(theme::scaled_px(12.))
            .w(theme::scaled_px(460.))
            .flex()
            .flex_col()
            .gap_2();

        // While an async op runs, show a busy snackbar with a spinning sync icon
        // (user request) — a lighter alternative to a blocking popup.
        if let Some(op) = self.busy_op {
            stack = stack.child(self.render_busy_snackbar(op));
        }

        // The toast cards are an independently-rendered child entity.
        stack = stack.child(toast_stack);
        Some(stack.into_any())
    }

    /// A snackbar shown while an async op runs: a continuously spinning sync
    /// icon + a friendly label (user request — a non-blocking alternative to a
    /// modal busy-spinner). Driven automatically by `busy_op`, so every async
    /// op gets one for free.
    fn render_busy_snackbar(&self, op: &'static str) -> gpui::AnyElement {
        let accent = theme().color_branch;
        let icon = render_overlay::big_sync_icon(accent, "kagi-busy-snackbar-spin");
        div()
            .w(theme::scaled_px(460.))
            .flex()
            .flex_row()
            .items_center()
            // 1.5× the toast gap (8px → 12px) so the larger sync icon breathes
            // a bit more from the label (user request).
            .gap_3()
            .px_4()
            .py_3()
            .rounded(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_1()
            .border_color(rgb(accent))
            .text_base()
            .text_color(rgb(theme().text_main))
            .child(div().flex_shrink_0().child(icon))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(SharedString::from(busy_label(op))),
            )
            .into_any()
    }

    fn render_commit_menu_overlay(
        &self,
        state: CommitMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let detail = self.active_view.details.get(state.row_index)?;
        let target = self.commit_id_for_row(state.row_index)?;
        let ctx = self.menu_context(state.row_index)?;
        let groups = context_menu::build_commit_menu(&ctx);
        let title = detail.full_message.as_ref().lines().next().unwrap_or("");
        let header = context_menu::short_title_header(detail.full_sha.as_ref(), title);
        Some(context_menu::render_commit_menu_overlay(
            state, target, header, groups, window, cx,
        ))
    }

    fn render_branch_menu_overlay(
        &self,
        state: BranchMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let ctx = self.branch_menu_context(&state);
        let groups = branch_menu::branch_context_menu_items(&ctx);
        let header = branch_menu::header(&ctx);
        Some(branch_menu::render_branch_menu_overlay(
            state, header, groups, window, cx,
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
            state, header, groups, window, cx,
        ))
    }
}

impl Render for KagiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ADR-0109: swap in any pending background highlight result before
        // painting, so the text-first diff gets its colors on the next frame
        // after the tree-sitter parse completes off-thread.
        self.apply_pending_highlights();

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
        self.sync_modal_inputs(window, cx);

        if std::env::var("KAGI_DEBUG_RENDER").as_deref() == Ok("1") {
            use std::sync::atomic::{AtomicU64, Ordering as O};
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, O::Relaxed) + 1;
            if n.is_multiple_of(50) {
                klog!("render: {} frames", n);
            }
        }

        // T-COMMIT-016: a Smart Commit message generated on a background thread
        // is pushed into the commit-message Input here, where `&mut Window` is
        // available (set_value requires it).
        if let Some(msg) = self.pending_smart_msg.take() {
            if self.commit_template_mode {
                // Template mode: parse the generated Conventional subject into
                // the type/scope/summary (+body) fields so each goes into its own
                // box (ADR-0090).
                let fields = kagi_git::parse_message(&msg);
                self.set_template_inputs(&fields, window, cx);
            } else if let Some(input) = self.commit_input.clone() {
                input.update(cx, |state, cx| {
                    state.set_value(msg, window, cx);
                });
            }
        }

        // Graph horizontal scroll: clamp against the current repo's lane
        // count so the offset self-heals after tab switches and column
        // resizes.
        {
            let lane_count = self
                .active_view
                .rows
                .first()
                .map(|r| r.lane_count)
                .unwrap_or(0);
            // W28: clamp against the scaled lane pitch (matches scroll_graph_by).
            let max = (lane_count as f32 * graph_view::lane_w() - self.graph_col_w).max(0.0);
            if self.graph_scroll_x > max {
                self.graph_scroll_x = max;
            }
        }

        // When the walk filled the current limit there may be more history to
        // pull in, so we append one extra "load more" row at the bottom of the
        // virtual list (rendered specially in the uniform_list processor).
        let has_more_commits =
            self.commit_limit > 0 && self.active_view.rows.len() >= self.commit_limit;
        let row_count = self.active_view.rows.len() + usize::from(has_more_commits);
        let selected = self.selected;

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

        // W4-TABS / ADR-0028: no open tabs → Welcome screen. A remote read-only
        // view (ADR-0089 Phase 2b) has no local tab but still renders the
        // workspace from its applied snapshot.
        if self.tabs.is_empty() && self.remote_view.is_none() {
            let welcome = self.render_welcome(cx).into_any();
            return self.platform_window_shell(welcome, cx);
        }

        // ADR-0089 Phase 2c: in a remote view, lazily load the selected commit's
        // changed files over SSH (once per row). The render trigger covers every
        // selection path (click / keyboard / jump) uniformly.
        if self.remote_view.is_some() {
            if let Some(i) = selected {
                if !self.diff_caches.changed_files.contains_key(&i)
                    && !self.diff_caches.remote_inflight.contains(&i)
                {
                    self.load_remote_changed_files(i, cx);
                }
            }
        } else if let Some(i) = selected {
            // ADR-0107 / perf: local view — lazily load the selected commit's
            // changed files + diffstat off the UI thread (once per row), so no
            // selection path (click / keyboard / jump) blocks the frame. `select`
            // only records the selection; this fires the async load.
            if !self.diff_caches.changed_files.contains_key(&i)
                && !self.diff_caches.local_inflight.contains(&i)
            {
                self.load_local_changed_files(i, cx);
            }
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected
            .and_then(|i| self.active_view.details.get(i))
            .cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> = selected.map(|i| {
            self.diff_caches
                .changed_files
                .get(&i)
                .cloned()
                .unwrap_or(None)
        });
        // W16-DIFFSTAT: per-file additions/deletions for the selected commit.
        let changed_diffstat: Option<Vec<FileDiffStat>> =
            selected.and_then(|i| self.diff_caches.diffstat.get(&i).cloned());
        let wip_diffstat = self.wip_diffstat;
        // W2-INSPECTOR: badges for the selected commit row and tree-view toggle state.
        let selected_badges: Vec<commit_list::RefBadge> = selected
            .and_then(|i| self.active_view.rows.get(i))
            .map(|r| r.badges.clone())
            .unwrap_or_default();
        let inspector_tree_view = self.inspector_tree_view;

        // T-UI-003: Clone main diff state if present.
        let main_diff = self.main_diff.clone();
        let compare_view = self.compare_view.clone();
        let main_diff_scroll_handle = self.main_diff_scroll_handle.clone();

        // Clone modal state for render.
        let is_dirty = self.active_view.is_dirty;
        // PERF-SIDEBAR-VIRT: the navigator data (branches/remotes/tags/…) is no
        // longer cloned for render_sidebar — it's flattened into
        // `self.sidebar.rows` below and read by the virtualized list processor.
        let sidebar_filter = self.sidebar.filter.clone();
        // PERF-SIDEBAR-VIRT: flatten the navigator into `self.sidebar.rows`
        // (honouring collapse + filter) so the "sidebar-list" uniform_list can
        // virtualize it. The processor reads the field.
        //
        // T-PERF-RENDER-002 (ADR-0116 Wave 2): only rebuild when the inputs
        // change. A cheap, allocation-free fingerprint (view epoch + collection
        // lengths + collapsed sets + filter text) gates the O(all-refs)
        // clone+collect so unchanged frames reuse the cached `rows`. The filter
        // `InputState` has no notification path into `KagiApp`, so its value is
        // read+folded each frame rather than tracked via the epoch.
        let sidebar_filter_text: String = self
            .sidebar
            .filter
            .as_ref()
            .map(|ent| ent.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let sidebar_fingerprint = sidebar::sidebar_rows_fingerprint(
            self.view_epoch,
            self.active_view.branches.len(),
            self.active_view.remote_branches.len(),
            self.active_view.tags.len(),
            self.active_view.stashes.len(),
            self.active_view.worktrees.len(),
            &self.sidebar.collapsed,
            &self.branch_groups_collapsed,
            &sidebar_filter_text,
        );
        if sidebar_fingerprint != self.sidebar.rows_fingerprint {
            self.sidebar.rows = sidebar::build_sidebar_rows(
                &self.active_view.branches,
                &self.active_view.remote_branches,
                &self.active_view.tags,
                &self.active_view.stashes,
                &self.active_view.worktrees,
                &self.sidebar.collapsed,
                &self.branch_groups_collapsed,
                &sidebar_filter_text,
            );
            self.sidebar.rows_fingerprint = sidebar_fingerprint;
        }
        let sidebar_row_count = self.sidebar.rows.len();
        let sidebar_scroll_handle = self.sidebar.scroll_handle.clone();
        let plan_modal = self.plan_modal().cloned();
        let pull_modal = self.pull_modal().cloned();
        let undo_modal = self.undo_modal().cloned();
        let history_modal = self.history_modal().cloned();
        let amend_modal = self.amend_modal().cloned();
        let pop_modal = self.pop_modal().cloned();
        let stash_drop_modal = self.stash_drop_modal().cloned();
        let push_modal = self.push_modal().cloned();
        let branch_plan_modal = self.branch_plan_modal().cloned();
        let set_upstream_modal = self.set_upstream_modal().cloned();
        let rename_branch_modal = self.rename_branch_modal().cloned();
        let merge_modal = self.merge_modal().cloned();
        let tracking_checkout_modal = self.tracking_checkout_modal().cloned();
        let switch_to_latest_modal = self.switch_to_latest_modal().cloned();
        let create_branch_modal = self.create_branch_modal().cloned();
        let create_worktree_modal = self.create_worktree_modal().cloned();
        let remote_browse_modal = self.remote_browse_modal.clone();
        let delete_branch_modal = self.delete_branch_modal().cloned();
        let discard_modal = self.discard_modal().cloned();
        let file_menu = self.file_menu;
        let modal_focus = self.modal_focus.clone();
        let stash_push_modal = self.stash_push_modal().cloned();
        let stash_push_focus = self.stash_push_focus.clone();
        let stash_apply_modal = self.stash_apply_modal().cloned();
        let cherry_pick_modal = self.cherry_pick_modal().cloned();
        let revert_modal = self.revert_modal().cloned();
        let conflict_continue_modal = self.conflict_continue_modal().cloned();
        let status_footer = self.status_footer.clone();
        // W30-CONFLICT-UI: clone the Conflict Mode snapshot for render (free
        // functions in `conflict_view` render from this immutable copy).
        let conflict = self.conflict.mode.clone();
        // T-CONFLICT-FLOW-030: while a continued merge waits for its commit
        // message, show the normal body (commit panel) instead of the conflict
        // resolution body (ADR-0068). Conflict Mode is still active (MERGE_HEAD
        // present) but the editor is hidden behind the commit message panel.
        let conflict_merge_pending = self.conflict_merge_pending;
        // T-CONFLICT-UI: chrome the 3-pane Conflict Editor needs from the app
        // (the editors live on `self`, not on the cloned `ConflictMode`).
        let conflict_chrome = conflict_view::EditorChrome {
            inputs: self
                .conflict
                .editor_inputs
                .as_ref()
                .map(|i| conflict_view::EditorInputs {
                    path: i.path.clone(),
                    result: i.result.clone(),
                }),
            ab_scroll: self.conflict.ab_scroll_handle.clone(),
            result_editing: self.conflict.result_editing,
            reset_all_armed: self.conflict.reset_all_armed,
            ab_split: self.conflict.ab_split,
            result_split: self.conflict.result_split,
            selected_hunk: self.conflict.selected_hunk,
            geom: self.conflict.geom.clone(),
            ab_geom: self.conflict.ab_geom.clone(),
        };
        let commit_menu_overlay = self
            .commit_menu
            .clone()
            .and_then(|state| self.render_commit_menu_overlay(state, window, cx));
        let branch_menu_overlay = self
            .branch_menu
            .clone()
            .and_then(|state| self.render_branch_menu_overlay(state, window, cx));
        let stash_menu_overlay = self
            .stash_menu
            .clone()
            .and_then(|state| self.render_stash_menu_overlay(state, window, cx));
        // T-CONFLICT-DASH-022: per-file "…" overflow menu overlay (anchored at the
        // click position; rendered top-level so it floats over the body).
        let conflict_file_menu_overlay = match (&conflict, self.conflict.file_menu) {
            (Some(m), Some((idx, pos))) => {
                Some(conflict_view::render_file_menu(m, idx, pos, window, cx))
            }
            _ => None,
        };
        // T-HT-001: clone toolbar/summary state for header render.
        // W3-NOTIFY: while a background git op runs, disable every git button
        // so operations never overlap.
        let mut toolbar_state = self.active_view.toolbar_state.clone();
        if self.busy_op.is_some() {
            toolbar_state.pull_on = false;
            toolbar_state.push_on = false;
            toolbar_state.stash_on = false;
            toolbar_state.pop_on = false;
            toolbar_state.undo_on = false;
        }
        let status_summary = self.active_view.status_summary.clone();

        // T023: pane widths for divider rendering.
        let sidebar_width = self.sidebar.width;
        let panel_width = self.panel_width;
        // T030: inner column widths for the commit list.
        let badge_col_w = self.badge_col_w;
        let graph_col_w = self.graph_col_w;

        // T028: clone scroll handle for wiring into uniform_list via track_scroll.
        let commit_scroll_handle = self.commit_scroll_handle.clone();

        // T023: divider drag-move handler — the full per-divider math lives in
        // `handle_divider_drag` (extracted in T-SPLIT-RENDER-001); placed on the
        // root div so it fires even when the mouse leaves the 4px divider strip.
        let divider_drag_move = cx.listener(
            move |this, event: &gpui::DragMoveEvent<DividerDrag>, window, cx| {
                this.handle_divider_drag(event, window, cx);
            },
        );

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.commit_panel_open;
        let commit_panel = self.commit_panel.clone();
        // T-SPLIT-HELPERS-001 / ADR-0116 Wave 3: commit_input + template mode/inputs
        // are read directly from `self` inside `render_commit_panel` (now a `&self`
        // method), so they no longer need to be hoisted/threaded through render_body.

        // T-BP-002: bottom panel state.
        let bottom_panel_open = self.bottom_panel_open;
        let bottom_panel_height = self.bottom_panel_height;
        let bottom_tab = self.bottom_tab;

        // T-BP-002: cmd-j toggle action handler.
        let toggle_bottom_panel = cx.listener(|this, _: &ToggleBottomPanel, _window, cx| {
            this.bottom_panel_open = !this.bottom_panel_open;
            cx.notify();
        });

        // T-UI-003: Esc closes the main diff view (no-op when main_diff is None).
        let close_main_diff = cx.listener(|this, _: &CloseMainDiff, _window, cx| {
            // Esc cancels an open modal first (user request: Esc = cancel).
            if this.cancel_active_modal(cx) {
                return;
            }
            if this.commit_menu.is_some() {
                this.commit_menu = None;
                cx.notify();
            } else if this.branch_menu.is_some() {
                this.branch_menu = None;
                cx.notify();
            } else if this.main_diff.is_some() {
                this.close_main_diff();
                cx.notify();
            }
        });

        // ── Normal state: header + body + bottom panel slot + status bar ─────
        let root = div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(UI_FONT)
            .bg(rgb(theme().bg_base))
            .children(self.render_platform_titlebar(cx))
            // Key events only dispatch along the focus path, so the root must
            // own (and initially hold) focus for window-wide actions to work.
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // T-BP-002: cmd-j toggle action (window-wide via on_action on root div).
            .on_action(toggle_bottom_panel)
            // T-UI-003: Esc closes the main diff view.
            .on_action(close_main_diff)
            // Arrows: step diff files while the main diff is open, otherwise
            // move the commit selection (user request).
            .on_action(cx.listener(|this, _: &DiffPrevFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                // File-history view is its own full overlay with its own entry
                // list + diff pane — navigate that, not the main commit list.
                if this.file_history.is_some() {
                    this.step_file_history_selection(-1, cx);
                } else if this.main_diff.is_some() {
                    this.main_diff_step(-1, cx);
                } else {
                    this.step_commit_selection(-1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DiffNextFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                if this.file_history.is_some() {
                    this.step_file_history_selection(1, cx);
                } else if this.main_diff.is_some() {
                    this.main_diff_step(1, cx);
                } else {
                    this.step_commit_selection(1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CheckoutSelected, window, cx| {
                this.checkout_selected_commit(window, cx);
            }))
            // ADR-0084: Cmd+Z / Cmd+Shift+Z app history undo/redo. Mirrors the
            // toolbar undo/redo buttons: open the plan→confirm modal when there
            // is something to (un)do, else surface a "nothing to" footer. The
            // keybinding's `!Input && !Terminal` predicate already keeps these
            // off text fields and the terminal.
            .on_action(cx.listener(|this, _: &commands::HistoryUndo, _window, cx| {
                if this.operation_history.can_undo() {
                    this.open_history_undo_modal();
                } else {
                    this.status_footer =
                        FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &commands::HistoryRedo, _window, cx| {
                if this.operation_history.can_redo() {
                    this.open_history_redo_modal();
                } else {
                    this.status_footer =
                        FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
                }
                cx.notify();
            }))
            // Enter checks out the selected commit. Handled as a raw key on
            // the root (the "enter" KeyBinding never dispatched — its
            // key_char "\n" takes a different path through the keymap than
            // chord keys like the arrows). All overlay/input guards live in
            // checkout_selected_commit.
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if std::env::var("KAGI_DEBUG_KEYS").as_deref() == Ok("1") {
                    eprintln!(
                        "[kagi] key: {:?} char={:?}",
                        e.keystroke.key, e.keystroke.key_char
                    );
                }
                let ks = &e.keystroke;
                if ks.key == "enter"
                    && !ks.modifiers.platform
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && !ks.modifiers.shift
                {
                    // Enter approves an open modal (user request); otherwise it
                    // checks out the selected commit.
                    if !this.confirm_active_modal(cx) {
                        this.checkout_selected_commit(window, cx);
                    }
                    cx.notify();
                }
            }))
            // ── W5-MENU / ADR-0029: conditional command handlers ──────────
            // Each menu action's handler is registered on the focused root ONLY
            // when `command_state == Enabled`.  gpui's macOS menu validation
            // (`is_action_available`, walks the dispatch tree) then greys out
            // any command whose handler is absent — the ADR-0029 disabled model.
            .map(|el| self.register_menu_actions(el, cx))
            // ── W4-TABS: repository tab strip (above the header toolbar) ──
            .children(self.render_tab_strip(cx))
            // ── Header slot ──────────────────────────────────
            // ADR-0013: pass HEAD commit summary for Undo label (first row = HEAD).
            .child(self.render_header_slot(
                toolbar_state,
                status_summary,
                self.active_view.rows.first().map(|r| r.summary.to_string()),
                cx,
            ))
            // ── W30-CONFLICT-UI: persistent conflict banner (under header) ──
            .children(
                conflict
                    .as_ref()
                    .map(|m| conflict_view::render_banner(m, cx)),
            )
            // ── Body slot: in Conflict Mode the conflict resolution pane
            //    replaces the normal sidebar | list | panel body. The center is
            //    the A/B hunk editor + Result Preview; the right is always the
            //    Conflict Dashboard (GitKraken-style — see render_body).
            .when(conflict.is_some() && !conflict_merge_pending, |el| {
                let m = conflict.clone().unwrap();
                el.child(conflict_view::render_body(&m, &conflict_chrome, cx))
            })
            .when(conflict.is_none() || conflict_merge_pending, |el| {
                el.child(self.render_body(
                    row_count,
                    selected,
                    detail,
                    changed_files,
                    changed_diffstat,
                    selected_badges,
                    inspector_tree_view,
                    main_diff,
                    compare_view,
                    main_diff_scroll_handle,
                    sidebar_row_count,
                    sidebar_scroll_handle,
                    sidebar_filter,
                    is_dirty,
                    sidebar_width,
                    panel_width,
                    badge_col_w,
                    graph_col_w,
                    commit_scroll_handle,
                    commit_panel_open,
                    commit_panel.clone(),
                    wip_diffstat,
                    cx,
                ))
            })
            // ── Bottom panel slot (T-BP-002) ─────────────────
            // Hidden on the conflict-resolution screen (user request): the
            // 3-pane editor + dashboard own the whole body there. The terminal
            // returns once the conflict is resolved / the commit panel shows.
            .when(!(conflict.is_some() && !conflict_merge_pending), |el| {
                el.children(self.render_bottom_panel_slot(
                    bottom_panel_open,
                    bottom_panel_height,
                    bottom_tab,
                    cx,
                ))
            })
            // ── Commit context menu overlay (below modals) ─────
            .children(commit_menu_overlay)
            // ── Branch context menu overlay (below modals) ─────
            .children(branch_menu_overlay)
            // ── Stash context menu overlay (below modals) ──────
            .children(stash_menu_overlay)
            // ── Conflict per-file "…" overflow menu overlay ────
            .children(conflict_file_menu_overlay)
            // ── W5-MENU: menu-driven overlay (branch picker / About / shortcuts) ──
            .children(self.render_menu_overlay(window, cx));

        // ── Modal / popover overlay layer (extracted: T-SPLIT-RENDER-001) ──
        let root = self.attach_modal_overlays(
            root,
            plan_modal,
            pull_modal,
            undo_modal,
            history_modal,
            conflict_continue_modal,
            amend_modal,
            pop_modal,
            stash_drop_modal,
            push_modal,
            branch_plan_modal,
            set_upstream_modal,
            rename_branch_modal,
            merge_modal,
            tracking_checkout_modal,
            switch_to_latest_modal,
            create_branch_modal,
            create_worktree_modal,
            remote_browse_modal,
            stash_push_modal,
            stash_apply_modal,
            cherry_pick_modal,
            revert_modal,
            delete_branch_modal,
            discard_modal,
            file_menu,
            modal_focus,
            stash_push_focus,
            commit_panel_open,
            commit_panel,
            window,
            cx,
        );

        root
            // ── Status bar slot (T017) — last operation result ─
            .child(self.render_status_bar(status_footer, bottom_panel_open, cx))
            // ── W3-NOTIFY: toast stack (above everything) ──────
            .children(self.render_toasts())
            // Linux/FreeBSD in-app menu dropdown (native menu bar is macOS-only).
            .children(self.render_platform_menu_dropdown(cx))
            .into_any()
    }
}
