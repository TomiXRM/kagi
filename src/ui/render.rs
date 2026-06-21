//! Presentation / render layer for `KagiApp`.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 1, P1): the
//! `impl Render for KagiApp`, the `render_*` view-construction methods, and the
//! free `render_*` presentation helpers. Behaviour is unchanged — this is a pure
//! physical split. Per Rust's visibility rules a child module (`crate::ui::render`)
//! can access the private fields and private methods of `KagiApp` defined in its
//! ancestor module `crate::ui`, so these methods move with no visibility change.

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
use super::*;
use crate::ui::modal_renderers::*;
use gpui_component::button::ButtonVariants as _;

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
        let icon = big_sync_icon(accent, "kagi-busy-snackbar-spin");
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

    /// Render the stash graph rows (ADR-0088): one fixed row per stash, shown
    /// directly below the WIP row, in the stash colour with an inbox icon and a
    /// graph node that connects down to the stash's base commit. Left-click pops,
    /// right-click opens the stash menu (same as the sidebar).
    fn render_stash_graph_rows(
        &self,
        badge_col_w: f32,
        graph_col_w: f32,
        graph_scroll_x: f32,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let visible_lanes = graph_view::lanes_for_width(graph_col_w);
        let stash_color = theme().color_warning;
        let stash_lanes = self.active_view.stash_graph_lanes.clone();
        let rh = row_height(self.graph_compact);

        // Lanes of connected stashes rendered *above* the current row, whose
        // branch lines must keep passing straight down through this row (fixes
        // the topmost stash's line vanishing at the next stash row).
        let mut passing_lanes: Vec<usize> = Vec::new();

        self.active_view
            .stash_graph_rows
            .iter()
            .map(|sr| {
                let index = sr.index;
                let label = sr.label.clone();
                let msg_for_menu = sr.label.to_string();
                let mut edges: Vec<kagi::graph::GraphEdge> = passing_lanes
                    .iter()
                    .map(|&lane| kagi::graph::GraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        kind: kagi::graph::EdgeKind::Pass,
                        // Stash lanes are painted in the stash colour; the lane
                        // colour index is unused for them.
                        color: lane,
                    })
                    .collect();
                if sr.connected {
                    // This stash's own line leaves its node downward; below this
                    // row it becomes a pass-through for subsequent rows.
                    edges.push(kagi::graph::GraphEdge {
                        from_lane: sr.lane,
                        to_lane: sr.lane,
                        kind: kagi::graph::EdgeKind::OutOfNode,
                        color: sr.lane,
                    });
                    passing_lanes.push(sr.lane);
                }
                let pop = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                    this.open_pop_modal(index);
                    cx.notify();
                });
                let menu = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
                    this.open_stash_menu(index, msg_for_menu.clone(), e.position);
                    cx.stop_propagation();
                    cx.notify();
                });
                let (cb, cbd, ct) = theme::badge_style(stash_color);
                div()
                    .id(("stash-graph-row", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .px_3()
                    .h(px(rh))
                    .on_click(pop)
                    .on_mouse_down(gpui::MouseButton::Right, menu)
                    .hover(|s| s.bg(rgb(theme().selected)))
                    // Badge column: a yellow stash chip with an inbox icon.
                    .child(
                        div()
                            .w(theme::scaled_px(badge_col_w))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_start()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(gpui::rgba(cb))
                                    .border_1()
                                    .border_color(gpui::rgba(cbd))
                                    .text_color(rgb(ct))
                                    .text_sm()
                                    .child(
                                        gpui::svg()
                                            .path("icons/inbox.svg")
                                            .w(theme::scaled_px(12.))
                                            .h(theme::scaled_px(12.))
                                            .text_color(rgb(ct)),
                                    )
                                    .child(SharedString::from("stash")),
                            )
                            // Connector line into the BRANCH/TAG pane toward the
                            // stash node (only when it connects to a base).
                            .when(sr.connected, |el| {
                                el.child(div().flex_1().h_full().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Inner divider spacer (badge|graph), bridged for the connector.
                    .child(
                        div()
                            .relative()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface)))
                            .when(sr.connected, |el| {
                                el.child(div().absolute().inset_0().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Graph column: the stash node + line down to its base.
                    .child(
                        div()
                            .w(theme::scaled_px(graph_col_w))
                            .h_full()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .when(visible_lanes > 0, |el| {
                                el.child(
                                    graph_view::graph_canvas(
                                        sr.lane,
                                        // Stash nodes paint in the stash colour;
                                        // node_color is unused for them.
                                        sr.lane,
                                        edges,
                                        visible_lanes,
                                        false,
                                        false,
                                        true,
                                        graph_scroll_x,
                                        graph_lane_pad_l(),
                                        stash_lanes.clone(),
                                    )
                                    .size_full(),
                                )
                            }),
                    )
                    // Inner divider spacer (graph|message).
                    .child(
                        div()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                    )
                    // Message column: the stash label, in the stash colour.
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .truncate()
                            .text_color(rgb(stash_color))
                            .child(label),
                    )
                    .into_any()
            })
            .collect()
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
        // a web-page zoom. `set_rem_size` persists, but kagi re-asserts it every
        // frame so it self-heals after window re-create / zoom changes.
        window.set_rem_size(px(theme::rem_size_px()));

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

        // W30-CONFLICT-UI: detect Conflict Mode once per repo path (no-op when
        // already detected this cycle).  Covers the startup / tab-switch
        // instant-apply paths where `reload()` did not run; the watcher and
        // post-operation paths force re-detection via `reload()`.
        self.detect_conflict_mode();

        // W3-NOTIFY: the toast auto-dismiss ticker now lives on the
        // `ToastStack` entity and is (re)started by `push_notify`, so KagiApp's
        // render no longer needs to nudge it (ADR-0110 Phase 5).

        // Background auto-fetch ticker (periodic `git fetch` so the graph and
        // ahead/behind stay fresh). Lazily spawned; no-op when off / no repo.
        self.ensure_auto_fetch_ticker(cx);

        // ADR-0084: seed the undo/redo history from the reflog once per repo, so
        // Cmd+Z works on a freshly-opened repo (the initial CLI/snapshot path
        // never calls `reload()`). `seed_history_from_reflog` is only-when-empty,
        // so it never clobbers an in-session stack.
        if !self.history_seed_attempted {
            self.history_seed_attempted = true;
            if let Some(repo_path) = self.repo_path.clone() {
                if let Ok(backend) = kagi_git::Backend::open(&repo_path) {
                    self.seed_history_from_reflog(&backend);
                }
            }
        }

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

        let row_count = self.active_view.rows.len();
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
                if !self.diff_cache.contains_key(&i) && !self.remote_diff_inflight.contains(&i) {
                    self.load_remote_changed_files(i, cx);
                }
            }
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected
            .and_then(|i| self.active_view.details.get(i))
            .cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> =
            selected.map(|i| self.diff_cache.get(&i).cloned().unwrap_or(None));
        // W16-DIFFSTAT: per-file additions/deletions for the selected commit.
        let changed_diffstat: Option<Vec<FileDiffStat>> =
            selected.and_then(|i| self.diffstat_cache.get(&i).cloned());
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
        // `self.sidebar_rows` below and read by the virtualized list processor.
        let sidebar_filter = self.sidebar_filter.clone();
        // PERF-SIDEBAR-VIRT: flatten the navigator into `self.sidebar_rows`
        // (honouring collapse + filter) so the "sidebar-list" uniform_list can
        // virtualize it. Rebuilt every render; the processor reads the field.
        let sidebar_filter_text: String = self
            .sidebar_filter
            .as_ref()
            .map(|ent| ent.read(cx).value().to_lowercase())
            .unwrap_or_default();
        self.sidebar_rows = sidebar::build_sidebar_rows(
            &self.active_view.branches,
            &self.active_view.remote_branches,
            &self.active_view.tags,
            &self.active_view.stashes,
            &self.active_view.worktrees,
            &self.sidebar_collapsed,
            &self.branch_groups_collapsed,
            &sidebar_filter_text,
        );
        let sidebar_row_count = self.sidebar_rows.len();
        let sidebar_scroll_handle = self.sidebar_scroll_handle.clone();
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
        let conflict_merge_pending = self.conflict.merge_commit_pending;
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
        let sidebar_width = self.sidebar_width;
        let panel_width = self.panel_width;
        // T030: inner column widths for the commit list.
        let badge_col_w = self.badge_col_w;
        let graph_col_w = self.graph_col_w;

        // T028: clone scroll handle for wiring into uniform_list via track_scroll.
        let commit_scroll_handle = self.commit_scroll_handle.clone();

        // T023: divider drag-move handler callback (single listener handles both dividers).
        // Placed on the root div so it fires even when the mouse moves outside
        // the narrow 4px divider strip.
        // Widths are derived from the ABSOLUTE cursor position, not deltas:
        // the sidebar starts at the window's left edge and the panel ends at
        // its right edge, so the divider should simply track the cursor.
        // (The previous delta-based approach needed a drag-start anchor that
        // `on_drag` cannot provide, which made the divider jump to its
        // clamp bounds — the "two positions / inverted" bug.)
        let divider_drag_move = cx.listener(
            move |this, event: &gpui::DragMoveEvent<DividerDrag>, window, cx| {
                let drag = *event.drag(cx);
                let cursor_x = f32::from(event.event.position.x);
                // W28: sidebar/panel widths are stored UNSCALED (logical px) but
                // rendered via `scaled_px`, so the divider visually sits at
                // `width * zoom`.  The cursor is in raw window px, so convert back
                // to logical space (divide by zoom) before clamping/storing, and
                // interpret the 4px divider's 2px half-offset in scaled space too.
                let z = theme::zoom();
                match drag.kind {
                    DividerKind::Sidebar => {
                        // Divider sits at x = sidebar_width * zoom; centre on cursor.
                        let new_width = ((cursor_x - 2.0 * z) / z).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                        if (new_width - this.sidebar_width).abs() > 0.5 {
                            this.sidebar_width = new_width;
                            cx.notify();
                        }
                    }
                    DividerKind::Panel => {
                        // Divider sits at x = viewport_width - panel_width * zoom.
                        let viewport_w = f32::from(window.viewport_size().width);
                        let new_width =
                            ((viewport_w - cursor_x - 2.0 * z) / z).clamp(PANEL_MIN, PANEL_MAX);
                        if (new_width - this.panel_width).abs() > 0.5 {
                            this.panel_width = new_width;
                            cx.notify();
                        }
                    }
                    DividerKind::BadgeCol => {
                        // T030/W28: badge column left edge = sidebar_width + INNER_DIV_W, all
                        // rendered scaled, so the on-screen left edge is (..)*z; convert the
                        // raw cursor back to logical space (/z) before clamping/storing.
                        let badge_col_left = this.sidebar_width + INNER_DIV_W; // sidebar divider = 4px
                        let new_w = ((cursor_x / z) - badge_col_left - INNER_DIV_W / 2.0)
                            .clamp(BADGE_COL_MIN, BADGE_COL_MAX);
                        if (new_w - this.badge_col_w).abs() > 0.5 {
                            this.badge_col_w = new_w;
                            theme::set_col_width("badge_col_w", new_w);
                            cx.notify();
                        }
                    }
                    DividerKind::GraphCol => {
                        // T030/W28: graph column left edge = badge_col_left + badge_col_w + INNER_DIV_W,
                        // all rendered scaled; convert the raw cursor back to logical space (/z).
                        let badge_col_left = this.sidebar_width + INNER_DIV_W;
                        let graph_col_left = badge_col_left + this.badge_col_w + INNER_DIV_W;
                        let new_w = ((cursor_x / z) - graph_col_left - INNER_DIV_W / 2.0)
                            .clamp(GRAPH_COL_MIN, GRAPH_COL_MAX);
                        if (new_w - this.graph_col_w).abs() > 0.5 {
                            this.graph_col_w = new_w;
                            theme::set_col_width("graph_col_w", new_w);
                            cx.notify();
                        }
                    }
                    DividerKind::BottomPanel => {
                        // T-BP-002: absolute-coordinate formula from ADR-0007:
                        //   height = viewport_h - cursor_y - status_bar_h(22) - 2
                        // W28: the panel is rendered scaled, so the on-screen gap
                        // between the cursor and the window bottom is the *scaled*
                        // height; divide by zoom to recover the unscaled stored
                        // value. The status bar (also scaled) and divider half are
                        // scaled in screen space too.
                        let viewport_h = f32::from(window.viewport_size().height);
                        let cursor_y = f32::from(event.event.position.y);
                        // max fraction is a screen-space cap → convert to unscaled.
                        let max_h = (viewport_h * BOTTOM_PANEL_MAX_FRAC) / z;
                        let new_h = ((viewport_h - cursor_y - (22.0 + 2.0) * z) / z)
                            .clamp(BOTTOM_PANEL_MIN_H, max_h);
                        if (new_h - this.bottom_panel_height).abs() > 0.5 {
                            this.bottom_panel_height = new_h;
                            cx.notify();
                        }
                    }
                    DividerKind::InspectorSplit => {
                        // W7-INSPECTOR2: absolute-coordinate ratio against the
                        // *measured* message+files region (paint-time canvas in
                        // inspector.rs).  Static offsets miss the variable-height
                        // header above the region, which showed up as a ~2cm jump
                        // when starting a drag.  Falls back to the constant-based
                        // approximation until the first paint has run.
                        let cursor_y = f32::from(event.event.position.y);
                        let (geom_top, geom_bottom) = this.inspector_geom.get();
                        let (top, bottom) = if geom_bottom - geom_top > 1.0 {
                            // Primary path: the canvas measured the real (already
                            // scaled) region bounds in screen px — use as-is.
                            (geom_top, geom_bottom)
                        } else {
                            // Transient fallback before first paint: the layout
                            // chrome is rendered scaled, so scale the constant
                            // offsets into screen space too.
                            let viewport_h = f32::from(window.viewport_size().height);
                            let bottom_taken = if this.bottom_panel_open {
                                STATUS_BAR_H + this.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                            } else {
                                STATUS_BAR_H
                            };
                            (INSPECTOR_TOP_OFFSET * z, viewport_h - bottom_taken * z)
                        };
                        // The divider itself occupies INSPECTOR_SPLIT_DIVIDER_H of
                        // the region; the flex split applies to the remainder. The
                        // span is in screen px (scaled), so scale the divider too.
                        let span = bottom - top - inspector::INSPECTOR_SPLIT_DIVIDER_H * z;
                        if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                            eprintln!(
                            "[kagi] split-drag: cursor_y={:.1} top={:.1} bottom={:.1} split={:.3}",
                            cursor_y, top, bottom, this.inspector_split
                        );
                        }
                        if span > 1.0 {
                            let ratio = ((cursor_y - top) / span)
                                .clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX);
                            if (ratio - this.inspector_split).abs() > 0.001 {
                                this.inspector_split = ratio;
                                cx.notify();
                            }
                        }
                    }
                    DividerKind::ConflictAB => {
                        // T-CONFLICT-UI-003: A|B vertical divider — ratio of the
                        // measured A·B row width given to A.  The cursor sits on
                        // the divider center, while flex layout assigns the ratio
                        // to the space excluding the scaled divider.
                        let cursor_x = f32::from(event.event.position.x);
                        let (left, right) = this.conflict.ab_geom.get();
                        if let Some(ratio) = conflict_split_ratio_from_cursor(
                            cursor_x,
                            left,
                            right,
                            CONFLICT_SPLIT_DIVIDER * z,
                            CONFLICT_AB_MIN,
                            CONFLICT_AB_MAX,
                        ) {
                            if (ratio - this.conflict.ab_split).abs() > 0.001 {
                                this.conflict.ab_split = ratio;
                                cx.notify();
                            }
                        }
                    }
                    DividerKind::FileHistoryRows => {
                        // ADR-0089: list/diff vertical split. Use the region's
                        // *measured* (top, bottom) screen bounds recorded by the
                        // paint-time canvas in `render_fh_list_and_diff`, so the
                        // cursor maps exactly. Falls back to a constant offset
                        // until the first paint has run.
                        let cursor_y = f32::from(event.event.position.y);
                        let (geom_top, geom_bottom) = this.file_history_geom.get();
                        let (top, bottom) = if geom_bottom - geom_top > 1.0 {
                            (geom_top, geom_bottom)
                        } else {
                            let viewport_h = f32::from(window.viewport_size().height);
                            let bottom_taken = if this.bottom_panel_open {
                                STATUS_BAR_H + this.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                            } else {
                                STATUS_BAR_H
                            };
                            (110.0 * z, viewport_h - bottom_taken * z)
                        };
                        let span = bottom - top;
                        if span > 1.0 {
                            if let Some(fh) = this.file_history.as_mut() {
                                let ratio = ((cursor_y - top) / span).clamp(0.15, 0.85);
                                if (ratio - fh.split).abs() > 0.002 {
                                    fh.split = ratio;
                                    cx.notify();
                                }
                            }
                        }
                    }
                    DividerKind::ConflictResult => {
                        // T-CONFLICT-UI-003: A·B / Result horizontal divider — ratio
                        // of the measured editor split region given to the A·B row.
                        // The previous separate hunk-control strip is gone; chunk
                        // controls live inside the A/B lists, so this measured
                        // region now matches the rendered split exactly.
                        let cursor_y = f32::from(event.event.position.y);
                        let (top, bottom) = this.conflict.geom.get();
                        if let Some(ratio) = conflict_split_ratio_from_cursor(
                            cursor_y,
                            top,
                            bottom,
                            CONFLICT_SPLIT_DIVIDER * z,
                            CONFLICT_RESULT_MIN,
                            CONFLICT_RESULT_MAX,
                        ) {
                            if (ratio - this.conflict.result_split).abs() > 0.001 {
                                this.conflict.result_split = ratio;
                                cx.notify();
                            }
                        }
                    }
                }
            },
        );

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.commit_panel_open;
        let commit_panel = self.commit_panel.clone();
        let commit_input = self.commit_input.clone();
        // T-COMMIT-009 / W14-TEMPLATE: structured template mode + field inputs.
        let commit_template_mode = self.commit_template_mode;
        let commit_template_inputs = self.commit_template_inputs.clone();

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
        div()
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
                    commit_input.clone(),
                    commit_template_mode,
                    commit_template_inputs.clone(),
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
            // ── W5-MENU: menu-driven overlay (branch picker / About / shortcuts) ──
            .children(self.render_menu_overlay(window, cx))
            // ── Plan modal overlay (above everything) ──────
            .when_some(plan_modal, |el, modal| {
                el.child(render_plan_modal(modal, cx))
            })
            // ── Pull plan modal overlay (T-HT-003) ──────────
            .when_some(pull_modal, |el, modal| {
                el.child(render_pull_modal(modal, cx))
            })
            // ── Undo / Pop plan modal overlays ───────────────
            .when_some(undo_modal, |el, modal| {
                el.child(render_undo_modal(modal, cx))
            })
            // ── Operation Undo / Redo modal (T-UNDOREDO-001) ──
            .when_some(history_modal, |el, modal| {
                el.child(render_history_modal(modal, cx))
            })
            // ── Sequencer conflict-continue confirmation (ADR-0068) ──
            .when_some(conflict_continue_modal, |el, modal| {
                el.child(render_conflict_continue_modal(modal, cx))
            })
            .when_some(amend_modal, |el, modal| {
                el.child(render_amend_modal(modal, cx))
            })
            .when_some(pop_modal, |el, modal| el.child(render_pop_modal(modal, cx)))
            // ── Stash drop modal overlay (ADR-0087) ─────────
            .when_some(stash_drop_modal, |el, modal| {
                el.child(render_stash_drop_modal(modal, cx))
            })
            // ── Push plan modal overlay (T-HT-004) ──────────
            .when_some(push_modal, |el, modal| {
                el.child(render_push_modal(modal, cx))
            })
            .when_some(branch_plan_modal, |el, modal| {
                el.child(render_branch_plan_modal(modal, cx))
            })
            .when_some(set_upstream_modal, |el, modal| {
                el.child(render_set_upstream_modal(modal, cx))
            })
            .when_some(rename_branch_modal, |el, modal| {
                el.child(render_rename_branch_modal(modal, cx))
            })
            .when_some(merge_modal, |el, modal| {
                el.child(render_merge_modal(modal, cx))
            })
            .when_some(tracking_checkout_modal, |el, modal| {
                el.child(render_tracking_checkout_modal(modal, cx))
            })
            .when_some(switch_to_latest_modal, |el, modal| {
                el.child(render_switch_to_latest_modal(modal, cx))
            })
            // ── Create-branch modal overlay (above everything) ──
            .when_some(create_branch_modal, |el, modal| {
                el.child(render_create_branch_modal(modal, modal_focus.clone(), cx))
            })
            // ── Create-worktree modal overlay ───────────────
            .when_some(create_worktree_modal, |el, modal| {
                el.child(render_create_worktree_modal(modal, modal_focus.clone(), cx))
            })
            // ── Remote SSH browse modal overlay (ADR-0089) ───
            .when_some(remote_browse_modal, |el, modal| {
                el.child(render_remote_browse_modal(modal, modal_focus.clone(), cx))
            })
            // ── Stash push modal overlay ─────────────────────
            .when_some(stash_push_modal, |el, modal| {
                el.child(render_stash_push_modal(modal, stash_push_focus, cx))
            })
            // ── Stash apply modal overlay ────────────────────
            .when_some(stash_apply_modal, |el, modal| {
                el.child(render_stash_apply_modal(modal, cx))
            })
            // ── Cherry-pick modal overlay (T016) ────────────
            .when_some(cherry_pick_modal, |el, modal| {
                el.child(render_cherry_pick_modal(modal, cx))
            })
            // ── Revert modal overlay (T-CM-034) ──────────────
            .when_some(revert_modal, |el, modal| {
                el.child(render_revert_modal(modal, cx))
            })
            // ── Delete-branch modal overlay (W2-DELETE) ──────
            .when_some(delete_branch_modal, |el, modal| {
                el.child(render_delete_branch_modal(modal, cx))
            })
            // ── Discard danger modal overlay (W17-DISCARD) ───
            .when_some(discard_modal, |el, modal| {
                el.child(render_discard_modal(modal, cx))
            })
            // ── Unstaged file context menu (right-click → Discard) ──
            .when_some(file_menu, |el, (fi, pos)| {
                el.child(render_file_menu_overlay(fi, pos, cx))
            })
            // ── Commit plan modal overlay (T025) ─────────────
            .when(
                commit_panel_open
                    && commit_panel
                        .as_ref()
                        .and_then(|p| p.plan_modal.as_ref())
                        .is_some(),
                |el| {
                    if let Some(Some(plan_modal)) =
                        commit_panel.as_ref().map(|p| p.plan_modal.clone())
                    {
                        el.child(render_commit_plan_modal(plan_modal, cx))
                    } else {
                        el
                    }
                },
            )
            // ── Smart Commit modal overlay (T-COMMIT-016) ────
            .when_some(self.smart_commit.modal.clone(), |el, modal| {
                el.child(render_smart_commit_modal(modal, cx))
            })
            // ── Auto-update modal overlay (ADR-0082) ──────────
            .when_some(
                if self.update_modal_open {
                    self.update_available.as_ref().map(|(p, _)| {
                        (
                            p.clone(),
                            self.update_installing,
                            self.update_status.clone(),
                        )
                    })
                } else {
                    None
                },
                |el, (plan, installing, status)| {
                    el.child(render_update_modal(plan, installing, status, window, cx))
                },
            )
            // ── Status bar slot (T017) — last operation result ─
            .child(self.render_status_bar(status_footer, bottom_panel_open, cx))
            // ── W3-NOTIFY: toast stack (above everything) ──────
            .children(self.render_toasts())
            // Linux/FreeBSD in-app menu dropdown (native menu bar is macOS-only).
            .children(self.render_platform_menu_dropdown(cx))
            .into_any()
    }
}

// ── AppShell layout slots ────────────────────────────────────────────────────
// ADR-0007 / T-BP-001: KagiApp::render is decomposed into four vertical
// flex slots.  Each slot is a plain method so that later tickets
// (T-BP-002, T-HT-001, …) can extend their signatures without
// touching the caller site.
impl KagiApp {
    /// W5-MENU / ADR-0029: register an `on_action` handler for every menu
    /// command, **but only when that command is currently enabled**.  Leaving a
    /// handler unregistered is exactly how macOS greys the matching menu item
    /// out (gpui validates each item via `is_action_available`, which checks the
    /// dispatch tree).  All handlers funnel into `handle_menu_command`, so the
    /// behaviour stays in `commands.rs` (no menu-specific logic lives here).
    fn register_menu_actions(&self, el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use commands as cmds;

        // Helper: conditionally attach one action handler bound to its registry
        // id.  `$ty` is the gpui Action type; `$id` is the registry id string.
        macro_rules! menu_act {
            ($el:expr, $ty:ty, $id:literal) => {{
                let enabled = cmds::is_enabled(self, $id);
                $el.when(enabled, |el| {
                    el.on_action(cx.listener(|this, _: &$ty, window, cx| {
                        this.handle_menu_command($id, window, cx);
                    }))
                })
            }};
        }

        let el = menu_act!(el, cmds::About, "app.about");
        // T-SETTINGS-001: open Settings (menu item + cmd-,).
        let el = menu_act!(el, cmds::OpenSettings, "app.settings");
        let el = menu_act!(el, cmds::Quit, "app.quit");
        let el = menu_act!(el, cmds::NewTab, "file.newTab");
        let el = menu_act!(el, cmds::CloseTab, "file.closeTab");
        let el = menu_act!(el, cmds::CloneRepository, "file.cloneRepository");
        let el = menu_act!(el, cmds::OpenRepository, "file.openRepository");
        let el = menu_act!(el, cmds::OpenInTerminal, "file.openInTerminal");
        let el = menu_act!(el, cmds::ConnectRemote, "file.connectRemote");
        let el = menu_act!(el, cmds::RefreshRepository, "file.refresh");
        let el = menu_act!(el, cmds::ZoomIn, "view.zoomIn");
        let el = menu_act!(el, cmds::ZoomOut, "view.zoomOut");
        let el = menu_act!(el, cmds::ZoomReset, "view.zoomReset");
        let el = menu_act!(el, cmds::EnterFullScreen, "view.fullScreen");
        let el = menu_act!(el, cmds::ToggleSidebar, "view.toggleSidebar");
        let el = menu_act!(el, cmds::ToggleCommitDetails, "view.toggleCommitDetails");
        let el = menu_act!(el, cmds::ToggleDiffView, "view.toggleDiffView");
        let el = menu_act!(el, cmds::Fetch, "repo.fetch");
        let el = menu_act!(el, cmds::Pull, "repo.pull");
        let el = menu_act!(el, cmds::Push, "repo.push");
        let el = menu_act!(el, cmds::OpenInFinder, "repo.openInFinder");
        let el = menu_act!(el, cmds::NewBranch, "branch.new");
        let el = menu_act!(el, cmds::CheckoutBranch, "branch.checkout");
        let el = menu_act!(el, cmds::RenameBranch, "branch.rename");
        let el = menu_act!(el, cmds::DeleteBranch, "branch.delete");
        let el = menu_act!(el, cmds::CopyCommitHash, "commit.copyHash");
        let el = menu_act!(el, cmds::CheckoutCommit, "commit.checkout");
        let el = menu_act!(el, cmds::CreateBranchFromCommit, "commit.createBranch");
        let el = menu_act!(el, cmds::CherryPickCommit, "commit.cherryPick");
        let el = menu_act!(el, cmds::RevertCommit, "commit.revert");
        let el = menu_act!(el, cmds::ResetToCommit, "commit.reset");
        let el = menu_act!(
            el,
            cmds::CompareWithWorkingTree,
            "commit.compareWorkingTree"
        );
        let el = menu_act!(el, cmds::MinimizeWindow, "window.minimize");
        let el = menu_act!(el, cmds::ZoomWindow, "window.zoom");
        let el = menu_act!(el, cmds::NewWindow, "window.new");
        let el = menu_act!(el, cmds::CloseWindow, "window.close");
        let el = menu_act!(el, cmds::KeyboardShortcuts, "help.shortcuts");
        let el = menu_act!(el, cmds::Documentation, "help.documentation");
        let el = menu_act!(el, cmds::ReportIssue, "help.reportIssue");
        // W9-THEME: theme switch actions (always enabled).
        let el = menu_act!(el, cmds::ThemeCatppuccin, "theme.catppuccin");
        let el = menu_act!(el, cmds::ThemeXcodeDark, "theme.xcodeDark");
        let el = menu_act!(el, cmds::ThemeXcodeLight, "theme.xcodeLight");
        let el = menu_act!(el, cmds::ThemeOneDark, "theme.oneDark");
        let el = menu_act!(el, cmds::ThemeOneLight, "theme.oneLight");
        let el = menu_act!(el, cmds::ThemeMonokai, "theme.monokai");
        // W22-I18N: language switch actions (always enabled).
        let el = menu_act!(el, cmds::LangEnglish, "lang.english");
        let el = menu_act!(el, cmds::LangJapanese, "lang.japanese");
        el
    }

    /// Header slot — the Toolbar bar (T-HT-001 / ADR-0013).
    ///
    /// Layout (34 px):
    ///   LEFT:   repo-name | branch → upstream ↑A ↓B
    ///   CENTRE: Pull(↓N) Push(↑N) | Branch Stash Pop | Undo("<summary>") Terminal
    ///   RIGHT:  Refresh
    fn render_header_slot(
        &mut self,
        toolbar: ToolbarState,
        summary: StatusBarSummary,
        // HEAD commit summary for Undo label (first row in commit list). ADR-0013.
        undo_summary: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ── Click handlers ──────────────────────────────────────────────────
        // Pull — disabled when behind=0 or no upstream (ADR-0013).
        let pull_on = toolbar.pull_on;
        let pull_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pull_on {
                this.open_pull_modal(cx);
            } else {
                let reason = if this.busy_op.is_some() {
                    Msg::PullBusy.t()
                } else if this.active_view.status_summary.is_detached {
                    Msg::PullDetached.t()
                } else if this.active_view.status_summary.is_unborn {
                    Msg::PullUnborn.t()
                } else if this.active_view.status_summary.no_upstream {
                    Msg::PullNoUpstream.t()
                } else {
                    Msg::PullNothing.t()
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Push (T-HT-004).
        let push_on = toolbar.push_on;
        let push_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if push_on {
                this.open_push_modal(cx);
            } else {
                let reason = if this.busy_op.is_some() {
                    Msg::PushBusy.t()
                } else if this.active_view.status_summary.is_detached {
                    Msg::PushDetached.t()
                } else if this.active_view.status_summary.is_unborn {
                    Msg::PushUnborn.t()
                } else if this.active_view.status_summary.no_upstream
                    && !this.active_view.status_summary.has_remote
                {
                    Msg::PushNoRemote.t()
                } else {
                    Msg::PushNothing.t()
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Branch — always enabled; use selected commit if any, else HEAD.
        let branch_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            // Resolve target commit: selected row → HEAD commit (first detail).
            let at = this
                .selected
                .and_then(|i| this.active_view.details.get(i))
                .map(|d| CommitId(d.full_sha.to_string()))
                .or_else(|| {
                    // Fall back to HEAD commit (first detail entry).
                    this.active_view
                        .details
                        .first()
                        .map(|d| CommitId(d.full_sha.to_string()))
                });
            if let Some(id) = at {
                this.open_create_branch_modal(id, cx);
            }
            cx.notify();
        });

        // Stash — enabled only when dirty.
        let stash_on = toolbar.stash_on;
        let stash_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if stash_on {
                this.open_stash_push_modal(cx);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::StashClean.t()));
            }
            cx.notify();
        });

        // Pop — enabled only when stash exists.
        let pop_on = toolbar.pop_on;
        let pop_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pop_on {
                // Pop the newest stash (index 0) — plan with conflict prediction.
                this.open_pop_modal(0);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::PopEmpty.t()));
            }
            cx.notify();
        });

        // Undo — operation-history undo (T-UNDOREDO-001, ADR-0081). Enabled per
        // the in-session history cursor (can_undo). Click opens the undo plan
        // modal (preview → confirm runs the safe ref move).
        let undo_on = self.operation_history.can_undo();
        let undo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_undo() {
                this.open_history_undo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
            }
            cx.notify();
        });

        // Redo — operation-history redo. Enabled per can_redo().
        let redo_on = self.operation_history.can_redo();
        let redo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_redo() {
                this.open_history_redo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
            }
            cx.notify();
        });

        // Refresh — always enabled.
        let refresh_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.refresh_spin_started = Some(Instant::now());
            // Re-read local .git immediately (instant feedback) …
            this.reload();
            this.status_footer = FooterStatus::Idle(SharedString::from(Msg::Refreshed.t()));
            // W3-NOTIFY: explicit refresh gets a completion toast (the
            // watcher's automatic reloads stay silent to avoid spam).
            this.push_toast(ToastKind::Success, Msg::Refreshed.t(), cx);
            // … then also fetch the remote in the background so changes pushed
            // elsewhere (e.g. a GitHub merge) show up. Quiet: success reloads the
            // graph, failure (offline / no remote) is silent — no error spam.
            this.fetch_async(true, cx);
            cx.notify();
        });

        // Terminal — toggles bottom panel to Terminal tab (ADR-0013).
        let terminal_on = self.bottom_panel_open && self.bottom_tab == BottomTab::Terminal;
        let terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if this.bottom_panel_open && this.bottom_tab == BottomTab::Terminal {
                // Same tab visible → close panel (toggle off).
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal session when first opened.
                this.ensure_terminal(window, cx);
            }
            cx.notify();
        });

        // ── Helper: build a single Finder/Keynote-style toolbar button ──────
        // W10-TOOLBAR: icon on top (20px ≈ Size::Medium), text_xs label below,
        // vertically stacked. Whole button gets a hover bg + rounded; width is
        // content-fit with a shared min-width so the row reads as a grid.
        //
        // `id` must be a unique string for GPUI element tracking.
        // `count` (>0) renders a small chip overlay at the icon's top-right;
        // 0 hides it (ADR-0013: Pull ↓N / Push ↑N).
        // `enabled` drives muted colour; disabled buttons keep their click
        // handler (which sets the reason footer) but render in muted colour.
        let make_btn = |id: &'static str,
                        label: &'static str,
                        icon: gpui_component::IconName,
                        enabled: bool,
                        count: usize| {
            let text_color = if enabled {
                theme().text_main
            } else {
                theme().text_muted
            };
            let chip_bg = theme().color_branch;
            let chip_fg = theme().bg_base;

            // Icon cell — `.relative()` so the count chip can be `.absolute()`
            // anchored to the icon's top-right corner (gpui has no negative
            // clip, so the chip is placed inside the icon bounds).
            let mut icon_cell = div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w(theme::scaled_px(22.0))
                .h(theme::scaled_px(22.0))
                .child(
                    gpui_component::Icon::new(icon)
                        .with_size(gpui_component::Size::Size(theme::scaled_px(20.0)))
                        .text_color(rgb(text_color)),
                );
            if count > 0 {
                let chip_text = if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                };
                icon_cell = icon_cell.child(
                    div()
                        .absolute()
                        .top(theme::scaled_px(-2.0))
                        .right(theme::scaled_px(-2.0))
                        .min_w(theme::scaled_px(14.0))
                        .h(theme::scaled_px(14.0))
                        .px(theme::scaled_px(3.0))
                        .rounded_full()
                        .bg(rgb(chip_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(chip_fg))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .line_height(theme::scaled_px(14.0))
                        .child(SharedString::from(chip_text)),
                );
            }

            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme::scaled_px(1.0))
                .min_w(theme::scaled_px(52.0))
                .px_1()
                .py(theme::scaled_px(2.0))
                .rounded_md()
                .hover(|style| style.bg(rgb(theme().selected)))
                .cursor(if enabled {
                    gpui::CursorStyle::PointingHand
                } else {
                    gpui::CursorStyle::Arrow
                })
                .child(icon_cell)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_color))
                        .child(SharedString::from(label)),
                )
        };

        // ── Undo / Redo tooltips: previewed operation summary (ADR-0081) ────
        // Labels stay the fixed "Undo"/"Redo"; the (possibly long) operation
        // summary is surfaced on hover. Sourced from the operation-history
        // cursor (peek_undo / peek_redo). `undo_summary` (legacy undo-commit
        // tooltip) is no longer used now that the button is generalised.
        let _ = &undo_summary;
        let undo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_undo()
            .map(|e| SharedString::from(format!("Undo: {}", e.summary)));
        let redo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_redo()
            .map(|e| SharedString::from(format!("Redo: {}", e.summary)));

        // ── Left label: branch info (ADR-0013) ─────────────────────────────
        // Format: `branch → upstream ↑A ↓B`  or state labels when detached/unborn.
        let branch_label = if summary.is_detached {
            "detached HEAD".to_string()
        } else if summary.is_unborn {
            "no commits yet".to_string()
        } else if summary.no_upstream {
            format!("{} (no upstream)", summary.branch)
        } else {
            let ahead = summary.ahead.unwrap_or(0);
            let behind = summary.behind.unwrap_or(0);
            if summary.upstream_name.is_empty() {
                format!("{} \u{2191}{} \u{2193}{}", summary.branch, ahead, behind)
            } else {
                format!(
                    "{} \u{2192} {} \u{2191}{} \u{2193}{}",
                    summary.branch, summary.upstream_name, ahead, behind
                )
            }
        };

        // ── Vertical separator ──────────────────────────────────────────────
        let sep = || {
            div()
                // 1px hairline kept literal (scaling a hairline blurs it);
                // only the visible height tracks zoom.
                .w(px(1.0))
                .h(theme::scaled_px(16.0))
                .bg(rgb(theme().text_muted))
                .mx_1()
                .flex_shrink_0()
        };

        // ── Toolbar bar (52 px — W10-TOOLBAR vertical buttons) ──────────────
        div()
            .id("toolbar-bar")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(theme::scaled_px(52.0))
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            .text_color(rgb(theme().text_sub))
            // ── LEFT column (flex_1, equal width to the RIGHT column so the
            // centre cluster is window-centred regardless of side widths).
            // 3-column layout: [LEFT flex_1][centre cluster][RIGHT flex_1]. ──
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    // ── LEFT: Refresh (user request: left of the repo title) ──
                    .child({
                        // Spin for one full turn after a click (user request).
                        const SPIN_MS: u64 = 700;
                        // Spin while any async op is in flight (merge plan/exec,
                        // pull, push, fetch, …) — the user wants the sync icon to
                        // keep turning during async work — and for one rotation
                        // after an explicit Refresh click.
                        if let Some(t) = self.refresh_spin_started {
                            if t.elapsed() >= Duration::from_millis(SPIN_MS) {
                                self.refresh_spin_started = None;
                            }
                        }
                        let spinning =
                            self.busy_op.is_some() || self.refresh_spin_started.is_some();
                        let icon = gpui::svg()
                            .path("icons/refresh-cw.svg")
                            .w(theme::scaled_px(16.0))
                            .h(theme::scaled_px(16.0))
                            .text_color(rgb(theme().text_main));
                        let icon: gpui::AnyElement = if spinning {
                            use gpui::AnimationExt as _;
                            icon.with_animation(
                                "tb-refresh-spin",
                                // Repeat so it spins continuously for the whole
                                // async op (not just one rotation).
                                gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
                                |svg, delta| {
                                    svg.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(delta * std::f32::consts::TAU),
                                    ))
                                },
                            )
                            .into_any_element()
                        } else {
                            icon.into_any_element()
                        };
                        div()
                            .id("tb-refresh")
                            .flex_shrink_0()
                            .mr_2()
                            .p_1()
                            .rounded_md()
                            .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                            .on_click(refresh_click)
                            .child(icon)
                    })
                    // ── repo name (top) + current branch (smaller, below) ──
                    // Stacked vertically so a long branch label never competes
                    // horizontally with the repo name (which used to vanish) nor
                    // runs under the centre Pull/Push/Branch cluster. Each line
                    // shrinks + truncates within the left column (user request).
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .mr_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme().text_main))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .line_height(theme::scaled_px(16.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(summary.repo_name.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().text_sub))
                                    .line_height(theme::scaled_px(13.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(branch_label)),
                            ),
                    ),
            ) // ── end LEFT column ──
            // ── CENTRE: window-centred cluster (flex_shrink_0 group) ──
            // Pull Push | Branch Stash Pop | Undo Terminal
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    // Pull (↓N chip when behind>0)
                    .child(
                        make_btn(
                            "tb-pull",
                            "Pull",
                            gpui_component::IconName::ArrowDown,
                            toolbar.pull_on,
                            toolbar.behind,
                        )
                        .on_click(pull_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Push (↑N chip when ahead>0)
                    .child(
                        make_btn(
                            "tb-push",
                            "Push",
                            gpui_component::IconName::ArrowUp,
                            toolbar.push_on,
                            toolbar.ahead,
                        )
                        .on_click(push_click),
                    )
                    .child(sep())
                    // Branch
                    .child(
                        make_btn(
                            "tb-branch",
                            "Branch",
                            gpui_component::IconName::Plus,
                            true,
                            0,
                        )
                        .on_click(branch_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Stash
                    .child(
                        make_btn(
                            "tb-stash",
                            "Stash",
                            gpui_component::IconName::Inbox,
                            toolbar.stash_on,
                            0,
                        )
                        .on_click(stash_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Pop
                    .child(
                        make_btn(
                            "tb-pop",
                            "Pop",
                            gpui_component::IconName::FolderOpen,
                            toolbar.pop_on,
                            0,
                        )
                        .on_click(pop_click),
                    )
                    .child(sep())
                    // Undo — operation-history undo (T-UNDOREDO-001). Label fixed; the
                    // previewed operation summary is shown in the tooltip.
                    .child(
                        make_btn(
                            "tb-undo",
                            Msg::Undo.t(),
                            gpui_component::IconName::Undo2,
                            undo_on,
                            0,
                        )
                        .when_some(undo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(undo_click),
                    )
                    // Redo — operation-history redo (T-UNDOREDO-001).
                    .child(
                        make_btn(
                            "tb-redo",
                            Msg::Redo.t(),
                            gpui_component::IconName::Redo2,
                            redo_on,
                            0,
                        )
                        .when_some(redo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(redo_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Terminal (toggles bottom panel Terminal tab)
                    .child(
                        make_btn(
                            "tb-terminal",
                            "Terminal",
                            gpui_component::IconName::SquareTerminal,
                            terminal_on,
                            0,
                        )
                        .on_click(terminal_click),
                    ),
            ) // ── end CENTRE cluster ──
            // ── RIGHT column (flex_1, equal width to the LEFT column) ──
            // Settings — now a standard toolbar button (icon + "Settings"
            // label) matching Pull/Push (T-SETTINGS-001 / ADR-0080). Opens the
            // Settings overlay; also reachable via the kagi menu and cmd-,.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .flex_1()
                    // Auto-update (ADR-0082): "↑ Update vX.Y.Z" chip when a newer
                    // release is available; click opens the update modal.
                    .when_some(
                        self.update_available.as_ref().map(|(p, _)| p.tag.clone()),
                        |el, tag| {
                            let open = cx.listener(|this, _: &gpui::ClickEvent, _w, cx| {
                                this.update_modal_open = true;
                                cx.notify();
                            });
                            el.child(
                                div()
                                    .id("tb-update")
                                    .flex()
                                    .items_center()
                                    .px(theme::scaled_px(8.0))
                                    .py(theme::scaled_px(4.0))
                                    .mr(theme::scaled_px(8.0))
                                    .rounded_md()
                                    .bg(rgb(theme().color_branch))
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|s| s.bg(rgb(theme().color_remote)))
                                    .child(
                                        div()
                                            .text_color(rgb(theme().bg_base))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(SharedString::from(format!(
                                                "\u{2191} Update {}",
                                                tag
                                            ))),
                                    )
                                    .on_click(open),
                            )
                        },
                    )
                    .child({
                        let settings_click =
                            cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                                this.menu_overlay = Some(commands::MenuOverlay::Settings);
                                // Probe Ollama so the Smart Commit model picker is
                                // usable without first opening the commit panel.
                                this.refresh_smart_commit_detection(cx);
                                cx.notify();
                            });
                        make_btn(
                            "tb-settings",
                            "Settings",
                            gpui_component::IconName::Settings,
                            true,
                            0,
                        )
                        .on_click(settings_click)
                    }),
            )
    }

    /// Render one WIP row for a single worktree (Model A+: every worktree's
    /// uncommitted state is shown at once, each tinted in its own colour so the
    /// rows are distinguishable at a glance — user request).
    ///
    /// The currently-open worktree's row opens the commit panel (stage/unstage)
    /// and carries a live `+/-` diffstat; a linked worktree's row switches the
    /// open repo to that worktree so its changes can be acted on there.
    #[allow(clippy::too_many_arguments)]
    fn render_wip_row(
        &self,
        color: gpui::Hsla,
        label: SharedString,
        change_count: usize,
        diffstat: Option<WipDiffStat>,
        click: WipRowClick,
        commit_panel_open: bool,
        badge_col_w: f32,
        graph_col_w: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let is_commit_panel = matches!(click, WipRowClick::CommitPanel);
        let dark = theme().dark;
        // Tinted chip built from the worktree's lane colour (badge_style only
        // takes a packed u32; lane_color is Hsla, so build the tint directly).
        // Stronger fill than a ref badge so the WIP rows read loudly.
        let chip_bg = gpui::hsla(color.h, color.s, color.l, if dark { 0.32 } else { 1.0 });
        let chip_border = gpui::hsla(color.h, color.s, color.l, if dark { 0.60 } else { 1.0 });
        let chip_text = if dark {
            rgb(0xffffff)
        } else {
            rgb(theme().bg_base)
        };
        // Whole-row background: a subtle wash of the worktree colour so each WIP
        // row is distinguishable at a glance (user request), with a bold colour
        // stripe down the left edge for prominence.
        let row_wash = gpui::hsla(color.h, color.s, color.l, if dark { 0.14 } else { 0.10 });

        let note = if is_commit_panel {
            i18n::wip_row_note(change_count)
        } else {
            i18n::wip_row_other(change_count)
        };

        // 🌳 ties the row to its worktree HEAD marker in the graph.
        let chip_label = SharedString::from(format!("🌳 {label}"));

        let mut row = div()
            .id(SharedString::from(format!("wip-row-{label}")))
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .pr_3()
            .border_l(theme::scaled_px(3.))
            .border_color(color)
            .pl(theme::scaled_px(9.))
            .h(px(row_height(self.graph_compact)));
        row = if is_commit_panel && commit_panel_open {
            row.bg(rgb(theme().selected))
        } else {
            row.bg(row_wash)
        };
        // Both row kinds are clickable: the open repo's row opens the commit
        // panel; a linked worktree's row switches the open repo to it. The two
        // closures are distinct types, so wire `on_click` inside each match arm.
        row = match click {
            WipRowClick::CommitPanel => {
                row.on_click(cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    this.open_commit_panel(window, cx);
                    cx.notify();
                }))
            }
            WipRowClick::OpenWorktree(path) => row.on_click(cx.listener(
                move |this, _e: &gpui::ClickEvent, _window, cx| {
                    this.open_repository(path.clone(), cx);
                    cx.notify();
                },
            )),
        };
        row = row.hover(|s| s.bg(rgb(theme().selected))).cursor_pointer();

        row
            // Badges column: worktree-coloured chip carrying a 🌳 + the branch
            // name, left-aligned like the commit-row badges.
            .child(
                div()
                    .w(theme::scaled_px(badge_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_start()
                    .gap_1()
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(chip_bg)
                            .border_1()
                            .border_color(chip_border)
                            .text_color(chip_text)
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .flex_shrink_0()
                            .overflow_hidden()
                            .truncate()
                            .child(chip_label),
                    ),
            )
            // Inner divider spacer (badge|graph handle width)
            .child(
                div()
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
            )
            // Graph column: hollow "not yet committed" node tinted in the
            // worktree colour — visually continues the graph upward.
            .child(
                div()
                    .w(theme::scaled_px(graph_col_w))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .ml(theme::scaled_px(graph_view::LANE_W / 2.0 - 4.5))
                            .w(theme::scaled_px(9.))
                            .h(theme::scaled_px(9.))
                            .rounded_full()
                            .border_1()
                            .border_color(color),
                    ),
            )
            // Inner divider spacer (graph|message handle width)
            .child(
                div()
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
            )
            // Summary area: change-count note, with the +N/-M diffstat (when
            // available, i.e. the current worktree) pushed to the right end.
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(theme().text_muted))
                            .overflow_hidden()
                            .truncate()
                            .child(SharedString::from(note)),
                    )
                    .when_some(diffstat, |el, stat| el.child(render_wip_diffstat(stat))),
            )
            .into_any_element()
    }

    /// Body slot — the main content area: sidebar | divider | commit list | optional panel.
    ///
    /// All parameters are pre-cloned values from `render`; no additional
    /// state access is performed inside this method.
    #[allow(clippy::too_many_arguments)]
    fn render_body(
        &mut self,
        row_count: usize,
        selected: Option<usize>,
        detail: Option<detail_panel::CommitDetail>,
        changed_files: Option<Option<Vec<FileStatus>>>,
        changed_diffstat: Option<Vec<FileDiffStat>>,
        selected_badges: Vec<commit_list::RefBadge>,
        inspector_tree_view: bool,
        main_diff: Option<MainDiffView>,
        compare_view: Option<CompareView>,
        main_diff_scroll_handle: UniformListScrollHandle,
        // PERF-SIDEBAR-VIRT: the navigator is now virtualized from
        // `self.sidebar_rows` (built in `render`); render_body only needs the
        // row count + scroll handle + filter input for `render_sidebar`.
        sidebar_row_count: usize,
        sidebar_scroll_handle: UniformListScrollHandle,
        sidebar_filter: Option<Entity<InputState>>,
        is_dirty: bool,
        sidebar_width: f32,
        panel_width: f32,
        badge_col_w: f32,
        graph_col_w: f32,
        commit_scroll_handle: UniformListScrollHandle,
        commit_panel_open: bool,
        commit_panel: Option<commit_panel::CommitPanelState>,
        commit_input: Option<Entity<InputState>>,
        commit_template_mode: bool,
        commit_template_inputs: Option<[Entity<InputState>; 6]>,
        wip_diffstat: Option<WipDiffStat>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // W11-AVATAR: snapshot the resolved avatar images so the inspector can
        // swap the initial circle for a real image without re-borrowing self.
        let avatar_images = self.avatar_images.clone();
        // Build divider 1: sidebar | main.
        let divider1 = div()
            .id("divider-sidebar")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::Sidebar,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── WIP rows (Model A+: one per dirty worktree, each in its own colour) ──
        // Built before the column so the closures don't conflict-borrow `self`:
        // gather plain params first (cloning out of `self.active_view`), then map
        // to elements via `render_wip_row`.
        let wip_rows: Vec<gpui::AnyElement> = {
            let live_total =
                self.active_view.status_summary.staged + self.active_view.status_summary.unstaged;
            let worktrees = &self.active_view.worktrees;
            let cur_idx = worktrees.iter().position(|w| w.is_current);
            let mut params: Vec<(
                gpui::Hsla,
                SharedString,
                usize,
                Option<WipDiffStat>,
                WipRowClick,
            )> = Vec::new();

            // Open-repo row: ALWAYS driven by the live working-tree status (kept
            // fresh by the watcher), independent of whether a worktree entry was
            // flagged `is_current` — so clicking and the +/- diffstat keep working
            // even when path canonicalization can't match the open repo. Clicking
            // opens the commit panel (stage/unstage).
            if is_dirty {
                let color = theme().lane_color(cur_idx.unwrap_or(0));
                let label = cur_idx
                    .and_then(|i| worktrees[i].branch.clone())
                    .or_else(|| {
                        self.active_view
                            .branches
                            .iter()
                            .find(|(_, is_head)| *is_head)
                            .map(|(n, _)| n.clone())
                    })
                    .unwrap_or_else(|| "WIP".to_string());
                params.push((
                    color,
                    SharedString::from(label),
                    live_total,
                    wip_diffstat,
                    WipRowClick::CommitPanel,
                ));
            }

            // Linked-worktree rows: from the snapshot's per-worktree wip. Clicking
            // switches the open repo to that worktree so its changes can be acted on.
            for (idx, wt) in worktrees.iter().enumerate() {
                if wt.is_current {
                    continue;
                }
                let Some(wip) = wt.wip else { continue };
                if !wip.is_dirty() {
                    continue;
                }
                let label =
                    SharedString::from(wt.branch.clone().unwrap_or_else(|| wt.name.clone()));
                params.push((
                    theme().lane_color(idx),
                    label,
                    wip.total(),
                    None,
                    WipRowClick::OpenWorktree(wt.path.clone()),
                ));
            }

            params
                .into_iter()
                .map(|(color, label, count, ds, click)| {
                    self.render_wip_row(
                        color,
                        label,
                        count,
                        ds,
                        click,
                        commit_panel_open,
                        badge_col_w,
                        graph_col_w,
                        cx,
                    )
                })
                .collect()
        };

        // T030: column header row (fixed, above WIP and commit list).
        let col_header = div()
            .id("col-header")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(theme::scaled_px(COL_HEADER_H))
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            // Badge column label
            .child(
                div()
                    .w(theme::scaled_px(badge_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_start()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("BRANCH / TAG")),
            )
            // Handle between badge and graph columns
            .child(
                div()
                    .id("divider-badge-col")
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag {
                            kind: DividerKind::BadgeCol,
                        },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Graph column label + compact toggle button (W2-GRAPH).
            .child({
                let is_compact = self.graph_compact;
                let compact_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                    this.graph_compact = !this.graph_compact;
                    // T-SETTINGS-001: persist so the Settings window + restart agree.
                    theme::set_compact_graph(this.graph_compact);
                    cx.notify();
                });
                div()
                    .w(theme::scaled_px(graph_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .on_scroll_wheel(cx.listener(
                        move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                            this.scroll_graph_by(&e.delta, cx);
                        },
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from("GRAPH")),
                    )
                    .child(
                        div()
                            .id("compact-toggle")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(rgb(if is_compact {
                                theme().color_branch
                            } else {
                                theme().text_muted
                            }))
                            .hover(|s| s.text_color(rgb(theme().color_branch)))
                            .on_click(compact_click)
                            .child(SharedString::from(if is_compact { "▥" } else { "▤" })),
                    )
            })
            // Handle between graph and message columns
            .child(
                div()
                    .id("divider-graph-col")
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag {
                            kind: DividerKind::GraphCol,
                        },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Message column label
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("MESSAGE")),
            );

        // ADR-0088: stash graph rows, shown below the WIP row.
        let stash_graph_row_els =
            self.render_stash_graph_rows(badge_col_w, graph_col_w, self.graph_scroll_x, cx);

        let commit_list_col = div()
            .flex_1()
            // Allow the center column to shrink below its longest commit
            // message's intrinsic width so the right-hand inspector panel always
            // keeps its space (flex min-width defaults to content size, which for
            // repos with long commit/merge messages pushes the inspector
            // off-screen — user report, remote SSH repos with long branch names).
            .min_w(px(0.))
            .overflow_hidden()
            .h_full()
            .flex()
            .flex_col()
            // ── Column header row (T030) ──────────────
            .child(col_header)
            // ── WIP rows (one per dirty worktree, each colour-coded) ──
            .children(wip_rows)
            // ── Stash graph rows (ADR-0088), below WIP ───────
            .children(stash_graph_row_els)
            // ── Virtualized commit list ──────────────
            .child({
                // W12-GCADOPT (§2.10): keep a handle clone for the Scrollbar
                // overlay; the other is moved into `track_scroll`.
                let scrollbar_handle = commit_scroll_handle.clone();
                with_vertical_scrollbar(
                    "commit-list-scroll",
                    &scrollbar_handle,
                    uniform_list(
                        "commit-list",
                        row_count,
                        cx.processor(move |this, range, _window, cx| {
                            render_rows(
                                &this.active_view.rows,
                                &this.avatar_images,
                                range,
                                selected,
                                this.badge_col_w,
                                this.graph_col_w,
                                this.graph_compact,
                                this.graph_scroll_x,
                                &this.active_view.stash_graph_lanes,
                                this.active_view
                                    .branch_solo
                                    .as_ref()
                                    .map(|solo| &solo.visible_commits),
                                cx,
                            )
                        }),
                    )
                    // T028: wire scroll handle so jump_to_branch can scroll the list.
                    .track_scroll(commit_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    true,
                )
            });

        // Active file (for list highlight) derived from the open main diff.
        let active_src = main_diff.as_ref().map(|d| d.source.clone());
        let active_commit_file: Option<usize> = match &active_src {
            Some(MainDiffSource::Commit { file_index, .. }) => Some(*file_index),
            Some(MainDiffSource::Compare { file_index, .. }) => Some(*file_index),
            _ => None,
        };
        let active_wip: Option<(bool, PathBuf)> = match &active_src {
            Some(MainDiffSource::Unstaged { path }) => Some((false, path.clone())),
            Some(MainDiffSource::Staged { path }) => Some((true, path.clone())),
            _ => None,
        };
        let main_diff_for_center = main_diff;

        // W5-MENU: View → Toggle Sidebar hides the navigator + its divider.
        let sidebar_visible = self.sidebar_visible;
        // ADR-0089: File History takes over the center+right area (sidebar stays).
        let file_history_open = self.file_history.is_some();
        let fh_branch = self
            .file_history
            .as_ref()
            .map(|fh| fh.branch.clone())
            .unwrap_or_default();
        let mut body_row = div()
            .flex()
            .flex_row()
            .flex_1()
            // min_h(0) — NOT h_full: the body must be able to shrink below its
            // natural content height, otherwise it pushes the bottom panel and
            // status bar out of the window on small window sizes (user report).
            .min_h(px(0.))
            // ── Left sidebar (W5-MENU: hidden when toggled off) ──
            .when(sidebar_visible, |el| {
                el.child(sidebar::render_sidebar(
                    sidebar_filter,
                    sidebar_width,
                    sidebar_row_count,
                    sidebar_scroll_handle,
                    cx,
                ))
                // ── Sidebar divider ───────────────────────
                .child(divider1)
            });

        // ADR-0089: File History view (top priority) — replaces center + right.
        if file_history_open {
            // Source the FH state from `&self` here (legitimate &self access) and
            // pass it down — render functions must NEVER read the entity back via
            // `cx`, because they run while the KagiApp entity is checked out for
            // update (re-entrant read panics).
            let fh_state = self
                .file_history
                .as_ref()
                .expect("file_history_open implies file_history is Some");
            let fh_menu = self.file_history_menu;
            let fh_geom = self.file_history_geom.clone();
            body_row = body_row.child(render_file_history_view(
                fh_state,
                fh_menu,
                fh_branch,
                panel_width,
                fh_geom,
                cx,
            ));
            return body_row;
        }

        body_row = body_row
            // ── Center column: W6-TABSPEED loading placeholder, full-width
            //    diff (T-UI-003), or the commit list.  The right panel stays
            //    visible in BOTH non-loading modes so the user can click
            //    through files continuously (user request).
            .child(if let Some(loading_label) = self.loading_tab.clone() {
                render_loading_placeholder(loading_label).into_any_element()
            } else if let Some(diff_view) = main_diff_for_center {
                render_main_diff_view(diff_view, main_diff_scroll_handle, true, cx)
                    .into_any_element()
            } else {
                commit_list_col.into_any_element()
            });

        // ── Right panel: commit panel OR detail panel ───────────
        // Build divider 2 (shared between both panel modes).
        let divider2 = div()
            .id("divider-panel")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::Panel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        if commit_panel_open {
            // ── Commit Panel mode (T025) ──────────────
            if let Some(panel_state) = commit_panel.clone() {
                // T-COMMIT-001: staged preview (count / A·M·D / target branch /
                // author). Cached on the panel state (computed in reload_status) —
                // computing it here ran a full working_tree_status *every frame*,
                // which froze the panel to ~6fps on large repos (PERF fix).
                let preview = panel_state.preview.clone();
                body_row = body_row.child(divider2).child(render_commit_panel(
                    panel_state,
                    panel_width,
                    commit_input.clone(),
                    commit_template_mode,
                    commit_template_inputs.clone(),
                    active_wip.clone(),
                    self.smart_commit.clone(),
                    preview,
                    self.cp_unstaged_scroll_handle.clone(),
                    self.cp_staged_scroll_handle.clone(),
                    cx,
                ));
            }
        } else if self.inspector_visible {
            // ── Commit Inspector panel (W2-INSPECTOR; W5-MENU toggle) ──
            body_row = body_row.when_some(detail, |el, d| {
                // ── Commit metadata + changed files ─
                let at = CommitId(d.full_sha.as_ref().to_string());
                let compare_for_panel = compare_view.clone();
                let files = compare_for_panel
                    .as_ref()
                    .map(|view| Some(view.files.clone()))
                    .unwrap_or_else(|| changed_files.clone().unwrap_or(None));
                // W16-DIFFSTAT: only the commit-vs-parent view has aggregated
                // diffstat; compare mode is out of scope for this lane.
                let diffstat = if compare_for_panel.is_some() {
                    None
                } else {
                    changed_diffstat.clone()
                };
                el.child(divider2).child(inspector::render_inspector(
                    d,
                    at,
                    selected_badges.clone(),
                    files,
                    diffstat,
                    compare_for_panel,
                    active_commit_file,
                    inspector_tree_view,
                    self.inspector_split,
                    self.inspector_geom.clone(),
                    panel_width,
                    &avatar_images,
                    cx,
                ))
            });
        }

        body_row
    }

    /// Bottom panel slot — T-BP-002: open/close + height resize.
    ///
    /// Returns `None` when the panel is closed (so `div().children(…)` adds no
    /// child element).  When open, returns the panel div with:
    /// - a 4px horizontal divider at the top (drag to resize)
    /// - a tab bar (OperationLog / Terminal)
    /// - a placeholder body area
    fn render_bottom_panel_slot(
        &mut self,
        open: bool,
        height: f32,
        active_tab: BottomTab,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !open {
            return None;
        }

        // ── Horizontal resize divider at top of panel ──
        let h_divider = div()
            .id("divider-bottom-panel")
            .w_full()
            .h(theme::scaled_px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
            .cursor_row_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::BottomPanel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── Tab bar ──
        let tab_bar = {
            let tab_operationlog_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                this.bottom_tab = BottomTab::OperationLog;
                cx.notify();
            });
            let tab_terminal_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start the terminal on first show.
                this.ensure_terminal(window, cx);
                cx.notify();
            });
            let tab_activity_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                this.bottom_tab = BottomTab::Activity;
                cx.notify();
            });

            let make_tab = |label: &'static str, is_active: bool| {
                let text_color = if is_active {
                    theme().text_main
                } else {
                    theme().text_muted
                };
                let bg_color = if is_active {
                    theme().selected
                } else {
                    theme().panel
                };
                div()
                    .px_3()
                    .h(theme::scaled_px(BOTTOM_PANEL_TAB_H))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .bg(rgb(bg_color))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .child(SharedString::from(label))
            };

            div()
                .id("bottom-panel-tab-bar")
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .flex_shrink_0()
                .bg(rgb(theme().panel))
                .child(
                    div()
                        .id("tab-oplog")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_operationlog_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::OperationLog.label(),
                            active_tab == BottomTab::OperationLog,
                        )),
                )
                .child(
                    div()
                        .id("tab-terminal")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_terminal_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::Terminal.label(),
                            active_tab == BottomTab::Terminal,
                        )),
                )
                .child(
                    div()
                        .id("tab-activity")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_activity_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::Activity.label(),
                            active_tab == BottomTab::Activity,
                        )),
                )
        };

        // ── Body: Operation Log or Terminal ──
        let body = match active_tab {
            // ADR-0110 Phase 5: the op-log renders as its own child entity so a
            // push / row-expand re-renders only this subtree.
            BottomTab::OperationLog => self
                .op_log
                .clone()
                .map(|e| e.into_any_element())
                .unwrap_or_else(|| div().flex_1().min_h(px(0.)).into_any_element()),
            BottomTab::Terminal => self.render_terminal_body(cx),
            BottomTab::Activity => self.render_activity_body(cx),
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        // `height` is the unscaled, persisted body height; the whole container
        // (body + divider + tab strip) is scaled at render so it tracks zoom.
        // The BottomPanel drag math converts the raw cursor back to this
        // unscaled space (see divider_drag_move).
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        Some(
            div()
                .id("bottom-panel")
                .flex()
                .flex_col()
                .w_full()
                .h(theme::scaled_px(panel_h))
                .flex_shrink_0()
                .child(h_divider)
                .child(tab_bar)
                .child(body),
        )
    }

    /// Render the Activity tab body: a Day/Week/Month granularity toggle, a
    /// commit + merge line chart, and the top-5 contributor ranking. The data is
    /// pre-aggregated in `active_view.activity` (built in `build_tab_view`), so
    /// this method only lays it out and wires the toggle.
    fn render_activity_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use kagi_domain::activity::Granularity;
        let activity = &self.active_view.activity;
        let gran = self.activity_granularity;
        let gdata = activity.get(gran);
        let buckets = gdata.buckets.clone();

        // Compact granularity toggle (Day / Week / Month / Year) — sized to match
        // the rest of the app's chrome rather than standing out.
        let mk_gran = |g: Granularity| {
            let active = g == gran;
            let click = cx.listener(move |this, _: &gpui::ClickEvent, _w, cx| {
                this.activity_granularity = g;
                cx.notify();
            });
            div()
                .id(SharedString::from(format!("activity-gran-{}", g.label())))
                .px(theme::scaled_px(6.))
                .py(theme::scaled_px(1.))
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .text_color(rgb(if active {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .bg(rgb(if active {
                    theme().selected
                } else {
                    theme().panel
                }))
                .hover(|s| s.cursor_pointer().bg(rgb(theme().surface)))
                .on_click(click)
                .child(SharedString::from(g.label()))
        };

        // Legend entry: a short line swatch (matching the chart strokes) + text.
        let legend = |color: u32, text: String| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(div().w(px(14.)).h(px(3.)).rounded_full().bg(rgb(color)))
                .child(SharedString::from(text))
        };

        // Window totals + ranking are scoped to the selected granularity.
        let win_commits = gdata.total_commits;
        let win_merges = gdata.total_merges;
        let contribs = &gdata.contributors;

        let mut gran_row = div().flex().flex_row().items_center().gap_1();
        for g in Granularity::ALL {
            gran_row = gran_row.child(mk_gran(g));
        }

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .child(gran_row)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .child(SharedString::from(gran.window_label()))
                    .child(legend(
                        theme().color_branch,
                        format!("{win_commits} commits"),
                    ))
                    .child(legend(
                        theme().color_warning,
                        format!("{win_merges} merges"),
                    )),
            );

        let chart = if win_commits == 0 {
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!(
                    "No commits in the {}",
                    gran.window_label()
                )))
                .into_any_element()
        } else {
            let max_c = buckets.iter().map(|b| b.commits).max().unwrap_or(0);
            let first_label = gdata.start_label.clone();
            let last_label = "now".to_string();
            let tick = |t: String| {
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(t))
            };
            let axis_w = theme::scaled_px(26.);
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.))
                        .flex()
                        .flex_row()
                        .child(
                            div()
                                .w(axis_w)
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .items_end()
                                .pr_1()
                                .py(theme::scaled_px(6.))
                                .child(tick(max_c.to_string()))
                                .child(tick("0".into())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(activity_view::activity_chart(buckets).size_full()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .child(div().w(axis_w).flex_shrink_0())
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_row()
                                .justify_between()
                                .child(tick(first_label))
                                .child(tick(last_label)),
                        ),
                )
                .into_any_element()
        };

        let max_commits = contribs.first().map(|c| c.commits).unwrap_or(0);
        let mut rows_el = div()
            .id("activity-ranking-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if contribs.is_empty() {
            rows_el = rows_el.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("—")),
            );
        } else {
            for (i, c) in contribs.iter().take(50).enumerate() {
                rows_el = rows_el.child(activity_view::contributor_row(i + 1, c, max_commits));
            }
        }
        let ranking = div()
            .w(theme::scaled_px(300.))
            .flex_shrink_0()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme().color_branch))
                    .flex_shrink_0()
                    .child(SharedString::from("Top contributors")),
            )
            .child(rows_el);

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .gap(theme::scaled_px(4.))
            .p(theme::scaled_px(6.))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_row()
                    .gap(theme::scaled_px(8.))
                    .child(chart)
                    .child(ranking),
            )
            .into_any_element()
    }

    /// Render the Terminal tab body (T-BP-007).
    ///
    /// Three possible states:
    /// 1. Session running → render `TerminalView` entity directly (flex_1 + min_h).
    /// 2. Session failed to start → show the error message.
    /// 3. Not yet started (session is None, or view is None with no error) →
    ///    show a "starting…" placeholder.  The Terminal tab click listener has
    ///    already called `ensure_terminal`; the view will appear on next repaint.
    fn render_terminal_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // W4-TABS: look up the active repo's session in the HashMap.
        let active_session = self
            .repo_path
            .as_ref()
            .and_then(|rp| self.terminal_sessions.get(rp));
        // Case 1: running terminal view.
        if let Some(session) = active_session {
            if let Some(ref view_entity) = session.view {
                // cmd-v paste: gpui-terminal 0.1.0 has no built-in clipboard
                // paste, so an ancestor key listener reads the gpui clipboard
                // and writes straight to the PTY. Key events bubble along the
                // focus path, so this fires while the terminal is focused.
                let paste_writer = session.paste_writer.clone();
                let term_focus = view_entity.read(cx).focus_handle().clone();
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    // Mark this subtree as the "Terminal" key context so global
                    // arrow/escape KeyBindings (scoped `!Terminal`) don't consume
                    // those keys while the terminal is focused — they flow to the
                    // terminal's own on_key_down → PTY (history, vim, etc.).
                    .key_context("Terminal")
                    // Clicking anywhere in the terminal area refocuses the
                    // terminal (the view's own mouse handling is a no-op in
                    // gpui-terminal 0.1.0, so a stray click could leave the
                    // keyboard focus elsewhere and break typing/cmd-v).
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_this, _e: &gpui::MouseDownEvent, window, _cx| {
                            window.focus(&term_focus);
                        }),
                    )
                    .on_key_down(
                        cx.listener(move |_this, event: &KeyDownEvent, _window, cx| {
                            let ks = &event.keystroke;
                            if ks.modifiers.platform && ks.key == "v" {
                                if let Some(writer) = paste_writer.as_ref() {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        writer.paste_text(&text);
                                        eprintln!(
                                            "[kagi] terminal: paste {} chars",
                                            text.chars().count()
                                        );
                                    }
                                }
                                cx.stop_propagation();
                            }
                        }),
                    )
                    .child(view_entity.clone())
                    .into_any();
            }

            // Case 2: start failed — show error.
            if let Some(ref err) = session.start_error {
                let msg = SharedString::from(format!("terminal error: {}", err));
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(rgb(theme().panel))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .child(msg)
                    .into_any();
            }
        }

        // Case 3: placeholder (no session yet / shell exited, will restart).
        div()
            .flex_1()
            .min_h(px(0.))
            .bg(rgb(theme().panel))
            .px_3()
            .py_2()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(
                "(terminal exited — re-opening will restart)",
            ))
            .into_any()
    }

    /// Status bar slot — the 22 px footer (T-BP-003 full implementation).
    ///
    /// Left → Right layout:
    ///   branch [● dirty] [↑A ↓B | no upstream] [staged N] [unstaged M]
    ///   HH:MM:SS  ·  <last operation message (flex_1, overflow_hidden)>
    ///   right end: >_ (Terminal icon) ≡ (Operation Log icon) — VSCode style
    ///
    /// The old ▲/▼ toggle is replaced by the icon buttons.
    fn render_status_bar(
        &mut self,
        status_footer: FooterStatus,
        bottom_panel_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.active_view.status_summary.clone();
        let bottom_tab = self.bottom_tab;

        // ── Footer message colour ──────────────────────────────
        let (footer_color, footer_text) = match &status_footer {
            FooterStatus::Success(msg) => (theme().color_success, msg.clone()),
            FooterStatus::Failed(msg) => (theme().color_blocker, msg.clone()),
            FooterStatus::Idle(msg) => (theme().text_muted, msg.clone()),
            FooterStatus::Busy(msg) => (
                theme().color_branch,
                SharedString::from(format!("\u{27f3} {}", msg)), // ⟳ msg
            ),
        };

        // ── Status chips (view-model — ADR-0076 / issue #13 P5) ──
        // The pure StatusBarVM owns the presentation decisions (which chips,
        // their labels, and order); the view below maps each role to a theme
        // colour + margin. Unit-tested without a window in view_models.
        let status_vm = view_models::StatusBarVM::from_summary(&summary);
        let branch_text = SharedString::from(status_vm.branch.clone());

        // ── Last refresh time ──────────────────────────────────
        let refresh_label = if summary.last_refresh_secs > 0 {
            Some(
                div()
                    .ml(theme::scaled_px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from(format_hms(summary.last_refresh_secs))),
            )
        } else {
            None
        };

        // ── VSCode-style icon buttons (Terminal + Operation Log) ──────────
        // Clicking an inactive icon opens the panel on that tab.
        // Clicking the active icon closes the panel (toggle).
        let oplog_active = bottom_panel_open && bottom_tab == BottomTab::OperationLog;
        let terminal_active = bottom_panel_open && bottom_tab == BottomTab::Terminal;

        let icon_terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if terminal_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal when opening via status bar icon.
                this.ensure_terminal(window, cx);
            }
            cx.notify();
        });

        let icon_oplog_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if oplog_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::OperationLog;
            }
            cx.notify();
        });

        let icon_terminal_color = if terminal_active {
            theme().text_main
        } else {
            theme().text_muted
        };
        let icon_oplog_color = if oplog_active {
            theme().text_main
        } else {
            theme().text_muted
        };

        let icon_terminal = div()
            .id("status-icon-terminal")
            .ml(theme::scaled_px(4.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_terminal_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_terminal_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::SquareTerminal)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_terminal_color)),
            );

        let icon_oplog = div()
            .id("status-icon-oplog")
            .ml(theme::scaled_px(2.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_oplog_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_oplog_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::Menu)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_oplog_color)),
            );

        // ── Assemble status bar ────────────────────────────────
        let mut bar = div()
            .id("status-footer")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(theme::scaled_px(STATUS_BAR_H))
            .flex_shrink_0()
            .px_2()
            .bg(rgb(theme().panel))
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            // Branch label
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(theme().text_main))
                    .child(branch_text),
            );

        // Status chips (dirty bullet, staged/unstaged, conflict/stash,
        // ahead-behind, upstream name) — order and labels come from StatusBarVM.
        for chip in &status_vm.chips {
            use view_models::StatusChipRole::*;
            let (color, margin) = match chip.role {
                Dirty => (theme().color_warning, 4.),
                Staged => (theme().color_success, 4.),
                Unstaged => (theme().color_warning, 4.),
                Conflict => (theme().color_blocker, 4.),
                Stash => (theme().text_sub, 4.),
                AheadBehind => (theme().text_sub, 6.),
                NoUpstream => (theme().text_muted, 6.),
                UpstreamName => (theme().text_muted, 6.),
            };
            bar = bar.child(
                div()
                    .ml(theme::scaled_px(margin))
                    .text_color(rgb(color))
                    .flex_shrink_0()
                    .child(SharedString::from(chip.text.clone())),
            );
        }
        // Refresh time
        if let Some(chip) = refresh_label {
            bar = bar.child(chip);
        }

        // Last operation message: flex_1, overflow_hidden, only if space allows.
        bar = bar.child(
            div()
                .flex_1()
                .ml(theme::scaled_px(6.))
                .overflow_hidden()
                .text_color(rgb(footer_color))
                .child(footer_text),
        );

        // Icon buttons at the right end.
        bar.child(icon_terminal).child(icon_oplog)
    }
}

// ──────────────────────────────────────────────────────────────
// Render helper functions (render_rows, render_commit_panel, etc.)
// have been extracted to render_helpers.rs (ADR-0113).

// ──────────────────────────────────────────────────────────────
// Toast overlay (ADR-0110 Phase 5): the toast cards render as their own
// `Entity<ToastStack>` so a push/expire only re-renders this subtree, not
// the whole `KagiApp`. The busy snackbar stays on `KagiApp` (driven by
// `busy_op`); see `KagiApp::render_toasts`.

/// The big spinning sync icon shared by the busy snackbar and the
/// sync-flavoured no-op toasts (`ToastKind::Sync`), so every sync-icon
/// snackbar looks identical. `key` keeps each animation instance distinct.
pub(crate) fn big_sync_icon(accent: u32, key: impl Into<gpui::ElementId>) -> gpui::AnyElement {
    use gpui::AnimationExt as _;
    const SPIN_MS: u64 = 700;
    gpui::svg()
        .path("icons/refresh-cw.svg")
        // ~2× the header spinner (user request) so the snackbar reads
        // clearly as "working".
        .w(theme::scaled_px(32.0))
        .h(theme::scaled_px(32.0))
        .text_color(rgb(accent))
        .with_animation(
            key,
            gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
            |svg, delta| {
                svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
}

impl gpui::Render for toast_stack::ToastStack {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack = div().flex().flex_col().gap_2();
        for toast in self.toasts() {
            let (accent, glyph) = match toast.kind {
                ToastKind::Info => (theme().color_branch, "\u{27f3}"), // ⟳
                ToastKind::Success => (theme().color_success, "\u{2713}"), // ✓
                ToastKind::Error => (theme().color_blocker, "\u{2715}"), // ✕
                ToastKind::Sync => (theme().color_branch, ""),
            };
            let id = toast.id;
            let is_sync = toast.kind == ToastKind::Sync;
            // Sync toasts reuse the busy snackbar's big spinning icon (user
            // request: "already up to date" must match an in-flight op); the
            // others keep the compact text glyph.
            let icon_el: gpui::AnyElement = if is_sync {
                big_sync_icon(accent, ("kagi-toast-sync", id))
            } else {
                div()
                    .text_color(rgb(accent))
                    .child(SharedString::from(glyph))
                    .into_any_element()
            };
            let leaving = toast.dismissing.is_some();
            let dismiss = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                this.begin_exit(id, cx);
            });
            // Explicit width so the animated margin-left slides the whole card
            // horizontally (a stretched flex child wouldn't translate cleanly).
            let card = div()
                .w(theme::scaled_px(460.))
                .flex()
                .flex_row()
                .when(is_sync, |d| d.items_center().gap_3())
                .when(!is_sync, |d| d.items_start().gap_2())
                .px_4()
                .py_3()
                .rounded(theme::scaled_px(8.))
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(accent))
                .text_base()
                .text_color(rgb(theme().text_main))
                .child(div().flex_shrink_0().child(icon_el))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(toast.message.clone()),
                )
                .child(
                    div()
                        .id(("toast-dismiss", id))
                        .flex_shrink_0()
                        .px_1()
                        .text_color(rgb(theme().text_muted))
                        .hover(|s| s.text_color(rgb(theme().text_main)))
                        .on_click(dismiss)
                        .child(SharedString::from("\u{00d7}")),
                );

            // Slide + fade: in from the left on appear, out to the left on
            // dismiss. Keyed by toast id so the animation plays once and holds.
            use gpui::AnimationExt as _;
            let animated = if leaving {
                card.with_animation(
                    ("kagi-toast-exit", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_EXIT_MS))
                        .with_easing(gpui::quadratic),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * delta)).opacity(1.0 - delta),
                )
                .into_any_element()
            } else {
                card.with_animation(
                    ("kagi-toast-enter", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_ENTER_MS))
                        .with_easing(gpui::ease_out_quint()),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * (1.0 - delta))).opacity(delta),
                )
                .into_any_element()
            };
            stack = stack.child(animated);
        }
        stack
    }
}

// ──────────────────────────────────────────────────────────────
// Operation Log overlay (ADR-0110 Phase 5 Step 5.1): the op-log renders as
// its own `Entity<OpLogPanel>` so a push / row-expand re-renders only this
// subtree. Embedded by `KagiApp::render_bottom_panel`.

impl gpui::Render for oplog_panel::OpLogPanel {
    /// Render the Operation Log tab body (T-BP-004).
    ///
    /// Uses `uniform_list` for virtual scroll.  Each row shows:
    ///   `HH:MM:SS  op  outcome-summary` (outcome coloured green/red/yellow).
    /// Clicking a row toggles single-row expansion (before/after + error/blockers).
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_count = self.len();

        if entry_count == 0 {
            return div()
                .flex_1()
                .min_h(px(0.))
                .bg(rgb(theme().panel))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::NoOperationsYet.t()))
                .into_any();
        }

        let scroll_handle = self.scroll_handle();
        // W12-GCADOPT (§2.10): Scrollbar overlay on the Operation Log list.
        let scrollbar_handle = scroll_handle.clone();

        let oplog_list = uniform_list(
            "oplog-list",
            entry_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                let entries: Vec<OpLogEntry> = this.entries().iter().cloned().collect();
                let expanded = this.expanded();
                range
                    .filter_map(|i| entries.get(i).cloned().map(|e| (i, e)))
                    .map(move |(i, entry)| {
                        let time_label = SharedString::from(format_hms(entry.timestamp));
                        let op_label = SharedString::from(entry.op.clone());

                        let (outcome_label, outcome_color) = match &entry.outcome {
                            OpOutcome::Success { after } => (
                                SharedString::from(format!("Success \u{2192} {}", after.head)),
                                theme().color_success,
                            ),
                            OpOutcome::Failed { error } => (
                                SharedString::from(format!("Failed: {}", error)),
                                theme().color_blocker,
                            ),
                            OpOutcome::Refused { blockers } => (
                                SharedString::from(format!(
                                    "Refused ({} blocker{})",
                                    blockers.len(),
                                    if blockers.len() == 1 { "" } else { "s" }
                                )),
                                theme().color_warning,
                            ),
                        };

                        let is_expanded = expanded == Some(i);

                        let row_click =
                            cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                                this.toggle_expanded(i);
                                cx.notify();
                            });

                        let row_bg = if i % 2 == 0 {
                            theme().panel
                        } else {
                            theme().bg_base
                        };

                        // Summary row.
                        let mut row_div = div()
                            .id(("oplog-row", i))
                            .flex()
                            .flex_col()
                            .w_full()
                            .bg(rgb(row_bg))
                            .hover(|s| s.bg(rgb(theme().surface)).cursor_pointer())
                            .on_click(row_click)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .px_3()
                                    .h(theme::scaled_px(22.))
                                    .child(
                                        div()
                                            .w(theme::scaled_px(60.))
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(rgb(theme().text_muted))
                                            .child(time_label),
                                    )
                                    .child(
                                        div()
                                            .w(theme::scaled_px(100.))
                                            .flex_shrink_0()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(theme().text_sub))
                                            .child(op_label),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(outcome_color))
                                            .truncate()
                                            .child(outcome_label),
                                    ),
                            );

                        // Expansion detail rows (before + outcome specifics).
                        if is_expanded {
                            let mut detail_lines: Vec<SharedString> = Vec::new();
                            detail_lines.push(SharedString::from(format!(
                                "  before:  {}",
                                entry.before.head
                            )));
                            detail_lines.push(SharedString::from(format!(
                                "  dirty:   {}",
                                entry.before.dirty
                            )));
                            match &entry.outcome {
                                OpOutcome::Success { after } => {
                                    detail_lines.push(SharedString::from(format!(
                                        "  after:   {}",
                                        after.head
                                    )));
                                    detail_lines.push(SharedString::from(format!(
                                        "  dirty:   {}",
                                        after.dirty
                                    )));
                                }
                                OpOutcome::Failed { error } => {
                                    detail_lines
                                        .push(SharedString::from(format!("  error:   {}", error)));
                                }
                                OpOutcome::Refused { blockers } => {
                                    for b in blockers {
                                        detail_lines
                                            .push(SharedString::from(format!("  blocker: {}", b)));
                                    }
                                }
                            }
                            let detail_div = div()
                                .flex()
                                .flex_col()
                                .w_full()
                                .px_3()
                                .py_1()
                                .bg(rgb(theme().selected))
                                .text_xs()
                                .text_color(rgb(theme().text_sub))
                                .children(detail_lines.into_iter().map(|line| div().child(line)));
                            row_div = row_div.child(detail_div);
                        }

                        row_div
                    })
                    .collect()
            }),
        )
        .track_scroll(scroll_handle)
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().panel));

        with_vertical_scrollbar("oplog-list-scroll", &scrollbar_handle, oplog_list, true)
            .into_any_element()
    }
}
