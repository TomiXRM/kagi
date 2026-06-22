//! Body slot (sidebar | commit list | inspector / commit panel) split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state. The WIP
//! / stash-graph row builders it consumes live in `render_wip.rs`. Behaviour is
//! unchanged — a pure physical move.

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
use super::*;

impl KagiApp {
    /// Body slot — the main content area: sidebar | divider | commit list | optional panel.
    ///
    /// All parameters are pre-cloned values from `render`; no additional
    /// state access is performed inside this method.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_body(
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
        // `self.sidebar.rows` (built in `render`); render_body only needs the
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
        // T-SPLIT-HELPERS-001 / ADR-0116 Wave 3: commit_input / template mode +
        // inputs are now read from `self` inside `render_commit_panel` (now a
        // `&self` method), so they no longer thread through render_body.
        wip_diffstat: Option<WipDiffStat>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // W11-AVATAR: snapshot the resolved avatar images so the inspector can
        // swap the initial circle for a real image without re-borrowing self.
        let avatar_images = self.avatars.images.clone();
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
            // Count every dirty kind so the row's "N changes" matches the
            // `is_dirty` gate above — otherwise an untracked-only (or
            // conflict-only) tree renders the row with a misleading "0 changes".
            let live_total = self.active_view.status_summary.wip_change_count();
            // Whether the *open* repo is itself a linked worktree (vs the main
            // working tree). Drives the open-repo WIP row's glyph: 🌲 worktree,
            // ✏️ normal branch.
            let open_is_worktree = self
                .tabs
                .get(self.active_tab)
                .map(|t| t.is_worktree)
                .unwrap_or(false);
            let worktrees = &self.active_view.worktrees;
            let cur_idx = worktrees.iter().position(|w| w.is_current);
            let mut params: Vec<(
                gpui::Hsla,
                SharedString,
                usize,
                Option<WipDiffStat>,
                WipRowClick,
                bool, // is_worktree → 🌲 vs ✏️
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
                    open_is_worktree,
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
                    true, // linked-worktree rows are always worktrees → 🌲
                ));
            }

            params
                .into_iter()
                .map(|(color, label, count, ds, click, is_worktree)| {
                    self.render_wip_row(
                        color,
                        label,
                        count,
                        ds,
                        click,
                        is_worktree,
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
                        cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                            let rows_len = this.active_view.rows.len();
                            let compact = this.graph_compact;
                            // Real commit rows for the part of the range that
                            // maps to commits; the trailing synthetic index
                            // (== rows_len) is the "load more" row.
                            let commit_range = range.start..range.end.min(rows_len);
                            let mut els: Vec<gpui::AnyElement> = render_rows(
                                &this.active_view.rows,
                                &this.avatars.images,
                                commit_range,
                                selected,
                                this.badge_col_w,
                                this.graph_col_w,
                                compact,
                                this.graph_scroll_x,
                                &this.active_view.stash_graph_lanes,
                                this.active_view
                                    .branch_solo
                                    .as_ref()
                                    .map(|solo| &solo.visible_commits),
                                cx,
                            )
                            .into_iter()
                            .map(gpui::IntoElement::into_any_element)
                            .collect();
                            if range.end > rows_len {
                                els.push(render_load_more_row(compact, cx));
                            }
                            els
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
        let main_diff_for_center = main_diff;

        // W5-MENU: View → Toggle Sidebar hides the navigator + its divider.
        let sidebar_visible = self.sidebar.visible;
        // ADR-0089: File History takes over the center+right area (sidebar stays).
        // ADR-0117: it is now its own Entity<FileHistoryView>; clone the handle so
        // we can embed it below (GPUI renders the entity directly).
        let file_history = self.file_history.clone();
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

        // ADR-0089 / ADR-0117: File History view (top priority) — replaces
        // center + right. The entity renders its own center+right body; embedding
        // `Entity<FileHistoryView>` gives it an isolated `cx.notify()` scope.
        if let Some(fh) = file_history {
            body_row = body_row.child(fh);
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
                body_row = body_row.child(divider2).child(self.render_commit_panel(
                    panel_state,
                    panel_width,
                    preview,
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
}
