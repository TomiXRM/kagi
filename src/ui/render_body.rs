//! Body slot (sidebar | commit list | inspector / commit panel) split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state. The WIP
//! / stash-graph row builders it consumes live in `render_wip.rs`. Behaviour is
//! unchanged — a pure physical move.

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
// ADR-0121 B1: bring the pane-item trait into scope for `is_open` / `render`.
use super::workspace::WorkspaceItem;
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
        // ADR-0121 B2: only the `has_detail` gate remains here — the Inspector
        // adapter (`workspace::InspectorItem`) re-derives the full detail +
        // changed-files/badges inputs from `self` in its render.
        detail: Option<detail_panel::CommitDetail>,
        // PERF-SIDEBAR-VIRT: the navigator is now virtualized from
        // `self.sidebar.rows` (built in `render`); render_body only needs the
        // row count + scroll handle + filter input for `render_sidebar`.
        sidebar_row_count: usize,
        sidebar_scroll_handle: UniformListScrollHandle,
        sidebar_filter: Option<Entity<InputState>>,
        is_dirty: bool,
        sidebar_width: f32,
        badge_col_w: f32,
        graph_col_w: f32,
        commit_scroll_handle: UniformListScrollHandle,
        commit_panel_open: bool,
        // ADR-0118 (Phase 5.2): the Commit Panel is now an `Entity<CommitPanelView>`
        // that self-renders. render_body pushes the parent-owned render inputs
        // (active_wip / scaled width / smart-commit snapshot) into it, then embeds
        // `entity.clone()` as a child.
        commit_panel: Option<Entity<commit_panel::CommitPanelView>>,
        wip_diffstat: Option<WipDiffStat>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    // Solo exit chip (user request): lives on the LEFT, in the
                    // BRANCH / TAG header — the eye is already on the branch
                    // column when soloing, so the way back sits on the same
                    // sight line (right-aligned placement reviewed and
                    // rejected). Replaces the header label while active.
                    .map(|el| match self.active_view.branch_solo.as_ref() {
                        None => el.child(SharedString::from("BRANCH / TAG")),
                        Some(solo) => {
                            let name = solo.name.clone();
                            let target = solo.target.clone();
                            el.child(
                                div()
                                    .id("exit-solo-chip")
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .bg(rgb(theme().surface))
                                    .text_color(rgb(theme().color_branch))
                                    .hover(|st| st.bg(rgb(theme().selected)))
                                    .cursor_pointer()
                                    .child(SharedString::from(format!("← Solo: {name}")))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(move |this, _e, _window, cx| {
                                            this.toggle_branch_solo(
                                                name.clone(),
                                                target.clone(),
                                                cx,
                                            );
                                            cx.notify();
                                        }),
                                    ),
                            )
                        }
                    }),
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
                    .track_scroll(&commit_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    true,
                )
            });

        // ADR-0120: resolve what each slot shows. The precedence lives in
        // `workspace::resolve_workspace` (one pure, unit-tested function), not
        // in branch ordering here — this method only routes on the result.
        // ADR-0121 B1: the entity-backed panes' gates come from their
        // registered items' `is_open` (same field reads, one source of truth).
        let layout = workspace::resolve_workspace(&workspace::WorkspaceInputs {
            sidebar_visible: self.sidebar.visible,
            file_history_open: workspace::FileHistoryItem.is_open(self),
            ecosystem_open: workspace::EcosystemItem.is_open(self),
            loading: self.loading_tab.is_some(),
            diff_open: workspace::MainDiffItem.is_open(self),
            commit_panel_open,
            commit_panel_present: commit_panel.is_some(),
            compare_open: workspace::CompareItem.is_open(self),
            inspector_visible: self.inspector_visible,
            has_detail: detail.is_some(),
            editor_mode: workspace::EditorWorkspaceItem.is_open(self),
        });

        let mut body_row = div()
            .flex()
            .flex_row()
            .flex_1()
            // min_h(0) — NOT h_full: the body must be able to shrink below its
            // natural content height, otherwise it pushes the bottom panel and
            // status bar out of the window on small window sizes (user report).
            .min_h(px(0.))
            // ── Left slot (W5-MENU: hidden when toggled off) ──
            .when(layout.left == workspace::LeftPane::Navigator, |el| {
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

        // ── Center slot ──────────────────────────────────
        // Takeovers (FileHistory / Ecosystem) span center + right; the resolver
        // already set `layout.right = Hidden` for them, so no early return is
        // needed. In the Loading/Diff/CommitList modes the right panel stays
        // visible so the user can click through files continuously (user
        // request).
        //
        // ADR-0121 B1: entity-backed panes route via "slot → registered item"
        // (`workspace::center_item`); each adapter carries what its old arm
        // did (how it wraps its entity, and its per-pane rationale). The
        // precedence is unchanged — it stays in `resolve_workspace`. The
        // non-entity contents (Loading placeholder / CommitList) keep plain
        // arms until B2 migrates them.
        body_row = match workspace::center_item(layout.center) {
            Some(item) => match item.render(self, &layout, cx) {
                Some(el) => body_row.child(el),
                // Gate raced closed between resolve and render: the Editor and
                // Diff arms' pre-existing fallback is the commit list, the
                // takeovers' is an empty center.
                None if matches!(
                    layout.center,
                    workspace::CenterPane::Editor | workspace::CenterPane::Diff
                ) =>
                {
                    body_row.child(commit_list_col)
                }
                None => body_row,
            },
            None => match layout.center {
                // W6-TABSPEED loading placeholder.
                workspace::CenterPane::Loading => body_row.child(render_loading_placeholder(
                    self.loading_tab.clone().unwrap_or_default(),
                )),
                _ => body_row.child(commit_list_col),
            },
        };

        // ── Right slot: commit panel OR inspector (ADR-0120) ─────
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

        // ADR-0121 B2: the right-slot panes route via "slot → registered item"
        // (`workspace::right_item`), like the center slot above; each adapter
        // carries what its old arm did (CommitPanel: push-then-embed the
        // entity; Inspector: the function-rendered panel). The precedence
        // (CommitPanel > Inspector) is unchanged — it stays in
        // `resolve_workspace`. `Hunks` (rendered inside the Editor entity —
        // see the `CenterPane::Editor` arm above) and `Hidden` have no item,
        // so no divider and no panel — same as the old no-op arms. A `None`
        // render (gate raced closed between resolve and render) also renders
        // nothing, exactly as the old per-field arms did.
        if let Some(el) =
            workspace::right_item(layout.right).and_then(|item| item.render(self, &layout, cx))
        {
            body_row = body_row.child(divider2).child(el);
        }

        body_row
    }
}
