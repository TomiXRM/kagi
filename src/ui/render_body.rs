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

/// One WIP row's render parameters: its [`graph_wip::WipTarget`] key (#476 —
/// what its connector anchor is looked up by), the worktree's ordinal lane
/// colour (#767: a *fallback*, used only when the row has no anchor to take
/// HEAD's colour from), chip label, change count, diffstat, click action, and
/// whether to draw the worktree glyph.
type WipRowParams = (
    graph_wip::WipTarget,
    usize,
    SharedString,
    usize,
    Option<WipDiffStat>,
    WipRowClick,
    bool,
);

impl KagiApp {
    /// Commit selection stays in commit coordinates; only scrolling includes
    /// the WIP/stash prefix in the shared virtual list.
    pub(super) fn commit_list_index(&self, commit_index: usize) -> usize {
        let view = self.view();
        usize::from(view.is_dirty)
            + view
                .worktrees
                .iter()
                .filter(|worktree| {
                    !worktree.is_current && worktree.wip.is_some_and(|wip| wip.is_dirty())
                })
                .count()
            + view.stash_graph_rows.len()
            + commit_index
    }

    /// Body slot: sidebar | (center above bottom panel) | optional right panel.
    ///
    /// The bottom panel belongs to the center column, leaving navigation and
    /// the inspector at their full body height.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_body(
        &mut self,
        row_count: usize,
        has_more_commits: bool,
        selected: Option<usize>,
        // ADR-0121 B2: only the `has_detail` gate remains here — the Inspector
        // adapter (`workspace::InspectorItem`) re-derives the full detail +
        // changed-files/badges inputs from `self` in its render.
        detail: Option<detail_panel::CommitDetail>,
        is_dirty: bool,
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
        // Capture plain data; the virtual list may render or measure a row more
        // than once, so elements are built inside its processor.
        let (wip_params, wip_anchors) = {
            // Count every dirty kind so the row's "N changes" matches the
            // `is_dirty` gate above — otherwise an untracked-only (or
            // conflict-only) tree renders the row with a misleading "0 changes".
            let live_total = self.view().status_summary.wip_change_count();
            // Whether the *open* repo is itself a linked worktree (vs the main
            // working tree). Drives the open-repo WIP row's glyph: 🌲 worktree,
            // ✏️ normal branch.
            let open_is_worktree = self
                .tabs
                .get(self.active_tab)
                .map(|t| t.is_worktree)
                .unwrap_or(false);
            let worktrees = &self.view().worktrees;
            let cur_idx = worktrees.iter().position(|w| w.is_current);
            // NOTE (#472/#476): each row carries the same `WipTarget` key
            // `graph_wip::wip_targets` derives from the snapshot, and looks its
            // connector lane up by that key. The two lists no longer have to
            // agree row-for-row — a worktree that has been committed (and so is
            // clean) simply drops out here and finds no lane, instead of
            // shifting every row below it onto the wrong lane and colour.
            let mut params: Vec<WipRowParams> = Vec::new();

            // Open-repo row: ALWAYS driven by the live working-tree status (kept
            // fresh by the watcher), independent of whether a worktree entry was
            // flagged `is_current` — so clicking and the +/- diffstat keep working
            // even when path canonicalization can't match the open repo. Clicking
            // opens the commit panel (stage/unstage).
            if is_dirty {
                let color_idx = cur_idx.unwrap_or(0);
                let label = cur_idx
                    .and_then(|i| worktrees[i].branch.clone())
                    .or_else(|| {
                        self.view()
                            .branches
                            .iter()
                            .find(|(_, is_head)| *is_head)
                            .map(|(n, _)| n.clone())
                    })
                    .unwrap_or_else(|| "WIP".to_string());
                params.push((
                    graph_wip::WipTarget::Current,
                    color_idx,
                    SharedString::from(label),
                    live_total,
                    wip_diffstat,
                    WipRowClick::CommitPanel,
                    open_is_worktree,
                ));
            }

            // Linked-worktree rows: from the snapshot's per-worktree wip. #473:
            // clicking shows that worktree's changes in the commit panel in
            // place; right-clicking offers "Open in new tab" (the old click).
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
                    graph_wip::WipTarget::Worktree(idx),
                    idx,
                    label,
                    wip.total(),
                    None,
                    WipRowClick::Worktree {
                        path: wt.path.clone(),
                        name: wt.name.clone(),
                        locked: wt.locked,
                    },
                    true, // linked-worktree rows are always worktrees → 🌲
                ));
            }

            // Target-keyed anchors survive a worktree becoming clean.
            let row_targets: Vec<graph_wip::WipTarget> = params.iter().map(|p| p.0).collect();
            let wip_anchors = graph_wip::lanes_for_rows(&self.view().wip_lanes, &row_targets);
            (params, wip_anchors)
        };
        let mut wip_passing_lanes = Vec::new();
        for anchor in wip_anchors.iter().flatten() {
            let trace = (anchor.lane, anchor.color);
            if !wip_passing_lanes.contains(&trace) {
                wip_passing_lanes.push(trace);
            }
        }
        let wip_count = wip_params.len();
        let prefix_count = wip_count + self.view().stash_graph_rows.len();

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
                    .map(|el| match self.view().branch_solo.as_ref() {
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
            // WIP, stash and commit rows share one scroll origin.
            .child({
                // W12-GCADOPT (§2.10): keep a handle clone for the Scrollbar
                // overlay; the other is moved into `track_scroll`.
                let scrollbar_handle = commit_scroll_handle.clone();
                let rows = div()
                    .id("commit-list-roles")
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        uniform_list(
                            "commit-list",
                            prefix_count + row_count,
                            cx.processor(
                                move |this, range: std::ops::Range<usize>, _window, cx| {
                                    let rows_len = this.view().rows.len();
                                    let compact = this.graph_compact;
                                    let mut els = Vec::with_capacity(range.len());
                                    if range.start < wip_count {
                                        let mut passing = Vec::new();
                                        for (
                                            i,
                                            (
                                                _,
                                                ordinal,
                                                label,
                                                count,
                                                diffstat,
                                                click,
                                                is_worktree,
                                            ),
                                        ) in wip_params
                                            .iter()
                                            .take(range.end.min(wip_count))
                                            .enumerate()
                                        {
                                            let anchor = wip_anchors[i];
                                            let color =
                                                anchor.map_or(*ordinal, |anchor| anchor.color);
                                            if range.contains(&i) {
                                                els.push(this.render_wip_row(
                                                    color,
                                                    label.clone(),
                                                    *count,
                                                    *diffstat,
                                                    click.clone(),
                                                    *is_worktree,
                                                    commit_panel_open,
                                                    anchor.map(|anchor| anchor.lane),
                                                    &passing,
                                                    this.badge_col_w,
                                                    this.graph_col_w,
                                                    this.ui().graph_scroll_x,
                                                    (i, prefix_count + rows_len),
                                                    cx,
                                                ));
                                            }
                                            if let Some(anchor) = anchor {
                                                let trace = (anchor.lane, anchor.color);
                                                if !passing.contains(&trace) {
                                                    passing.push(trace);
                                                }
                                            }
                                        }
                                    }
                                    if range.start < prefix_count && range.end > wip_count {
                                        els.extend(this.render_stash_graph_rows(
                                            this.badge_col_w,
                                            this.graph_col_w,
                                            this.ui().graph_scroll_x,
                                            &wip_passing_lanes,
                                            range.start.saturating_sub(wip_count)
                                                ..range.end.min(prefix_count) - wip_count,
                                            (wip_count, prefix_count + rows_len),
                                            cx,
                                        ));
                                    }
                                    let commit_range =
                                        range.start.saturating_sub(prefix_count).min(rows_len)
                                            ..range.end.saturating_sub(prefix_count).min(rows_len);
                                    els.extend(
                                        render_rows(
                                            &this.view().rows,
                                            &this.avatars.images,
                                            commit_range,
                                            selected,
                                            this.badge_col_w,
                                            this.graph_col_w,
                                            compact,
                                            this.ui().graph_scroll_x,
                                            &this.view().stash_graph_lanes,
                                            this.view()
                                                .branch_solo
                                                .as_ref()
                                                .map(|solo| &solo.visible_commits),
                                            (prefix_count, prefix_count + rows_len),
                                            &this.context_anchor,
                                            cx,
                                        )
                                        .into_iter()
                                        .map(gpui::IntoElement::into_any_element),
                                    );
                                    els
                                },
                            ),
                        )
                        // T028: wire scroll handle so jump_to_branch can scroll the list.
                        .track_scroll(&commit_scroll_handle)
                        .flex_1()
                        .min_h(px(0.)),
                    );
                let list = with_vertical_scrollbar(
                    "commit-list-scroll",
                    &scrollbar_handle,
                    super::list_a11y::list_box("commit-list", rows, Msg::A11yCommitList.t()),
                    true,
                )
                .child(e2e::measure_inside("commit-list-viewport"));
                // The pagination Button is a sibling, never a descendant of
                // the ListBox, whose virtual children are all Options.
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .child(list)
                    .when(has_more_commits, |column| {
                        column.child(render_load_more_row(self.graph_compact, cx))
                    })
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
            branch_cleanup_open: workspace::BranchCleanupItem.is_open(self),
            pr_mode: workspace::PrModeItem.is_open(self),
            issues_mode: workspace::IssuesModeItem.is_open(self),
            loading: self.loading_tab().is_some(),
            diff_open: workspace::MainDiffItem.is_open(self),
            commit_panel_open,
            commit_panel_present: commit_panel.is_some(),
            compare_open: workspace::CompareItem.is_open(self),
            inspector_visible: self.inspector_visible,
            has_detail: detail.is_some(),
            editor_mode: workspace::EditorWorkspaceItem.is_open(self),
        });

        // #955: the side panes slide toward what the layout shows, but only
        // when their own toggle moved them; a layout change jumps. Switching
        // between Inspector, Compare and Commit Panel keeps the slot shown,
        // so it is no motion at all.
        let motion_now = super::panel_motion::now();
        let motion_instant = super::panel_motion::instant();
        let sidebar_shown = layout.left == workspace::LeftPane::Navigator;
        self.panel_motion.sync_sidebar(
            sidebar_shown,
            self.sidebar.visible,
            motion_now,
            motion_instant,
        );
        let right_shown = matches!(
            layout.right,
            workspace::RightPane::Inspector
                | workspace::RightPane::Compare
                | workspace::RightPane::CommitPanel
        );
        self.panel_motion.sync_right(
            right_shown,
            self.inspector_visible,
            motion_now,
            motion_instant,
        );
        if right_shown {
            self.panel_motion.right_pane = Some(layout.right);
        }
        let sidebar_fraction = self.panel_motion.sidebar.visible(motion_now);
        let right_fraction = self.panel_motion.right.visible(motion_now);

        let mut body_row = div()
            .id("repo-tab-panel")
            .flex()
            .flex_row()
            .flex_1()
            // Only the center column gives up height to the bottom panel;
            // the body and both side panes still reach the status bar.
            .min_h(px(0.));
        // ── Left slot (W5-MENU: hidden when toggled off) ──
        // Drawn while it is shown or still closing; the divider travels with
        // the sidebar's edge inside the clip, which hangs it from the right.
        if sidebar_fraction > 0. {
            let mode = self.workspace_mode();
            let sidebar = div()
                .flex()
                .flex_row()
                .size_full()
                .child(workspace_mode::render_sidebar_pages(self, mode, cx))
                .child(divider1);
            body_row = body_row.child(
                super::panel_motion::clip(
                    "sidebar-clip",
                    super::panel_motion::Axis::Horizontal,
                    super::panel_motion::Anchor::End,
                    theme::scaled_px(self.sidebar.width + 4.),
                    sidebar_fraction,
                    sidebar,
                )
                .when(cfg!(feature = "gui-e2e"), |clip| {
                    clip.child(e2e::measure_inside("sidebar-clip"))
                }),
            );
        }

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
        let panel_visible = self.panel_motion.bottom.visible(motion_now);
        let panel_height = self.bottom_panel_height;
        let panel_tab = self.bottom_tab;
        let mut bottom_panel = self
            .render_bottom_panel_slot(panel_visible, panel_height, panel_tab, cx)
            .map(|panel| panel.into_any_element());
        // These takeovers render their own side panes. Their actual center
        // consumes the panel; only the other modes place it in the outer row.
        let nested = matches!(
            layout.center,
            workspace::CenterPane::Editor
                | workspace::CenterPane::PrMode
                | workspace::CenterPane::IssuesMode
        );
        let center_content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden();
        let center_content = match workspace::center_item(layout.center) {
            Some(item) => match item.render(
                self,
                &layout,
                if nested { bottom_panel.take() } else { None },
                cx,
            ) {
                Some(el) => center_content.child(el),
                // If a gate races closed, Editor/Diff fall back to the commit
                // list while takeover panes keep their empty center.
                None if matches!(
                    layout.center,
                    workspace::CenterPane::Editor | workspace::CenterPane::Diff
                ) =>
                {
                    center_content.child(commit_list_col)
                }
                None => center_content,
            },
            None => match layout.center {
                workspace::CenterPane::Loading => center_content.child(render_loading_placeholder(
                    self.loading_tab().unwrap_or_default(),
                )),
                _ => center_content.child(commit_list_col),
            },
        };
        let center_column = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(center_content)
            .children(bottom_panel);
        body_row = body_row.child(center_column);

        // ── Right slot: commit panel OR inspector (ADR-0120) ─────
        // Build divider 2 (shared between both panel modes).
        let divider2 = div()
            .id("divider-panel")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .relative()
            .when(cfg!(feature = "gui-e2e"), |divider| {
                divider.child(e2e::measure_inside("divider-panel"))
            })
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
        // Drawn while shown or still closing (then the pane last shown); the
        // divider travels with the pane's edge inside the clip.
        let right_pane = if right_shown {
            Some(layout.right)
        } else {
            self.panel_motion.right_pane.filter(|_| right_fraction > 0.)
        };
        if let Some(el) = right_pane
            .and_then(workspace::right_item)
            .and_then(|item| item.render(self, &layout, None, cx))
        {
            let pane = div()
                .flex()
                .flex_row()
                .size_full()
                .child(divider2)
                .child(el);
            body_row = body_row.child(
                super::panel_motion::clip(
                    "right-pane-clip",
                    super::panel_motion::Axis::Horizontal,
                    super::panel_motion::Anchor::Start,
                    theme::scaled_px(self.panel_width + 4.),
                    right_fraction,
                    pane,
                )
                .when(cfg!(feature = "gui-e2e"), |clip| {
                    clip.child(e2e::measure_inside("right-pane-clip"))
                }),
            );
        }

        // The repository tab's content, named after the tab (#983). The
        // toolbar, operation strip and status bar stay outside, as siblings
        // on the root; Conflict Mode replaces this row and has no panel.
        match self.tabs.get(self.active_tab) {
            Some(tab) => {
                super::tab_panel_a11y::tab_panel(body_row, "repo-tab-panel", tab.name.clone())
            }
            None => body_row,
        }
    }
}

/// W6-TABSPEED / ADR-0030: center-pane placeholder shown while an uncached tab
/// is loading on a background thread.  The tab strip stays operable above it.
/// ADR-0165: while the load is in flight, a cute mini "commit graph" — three
/// theme-colored dots bobbing in sequence — replaces the old static ⟳ glyph.
/// Same `.with_animation` idiom as the ecosystem loader / sync spinner. The
/// animated block must stay outside any `overflow_y_scroll` container
/// (`with_animation` doesn't tick there — see `kagi-ui-ecosystem/render.rs`).
/// Honors the reduce-motion setting (issue #354 / ADR-0173): when
/// `theme::reduce_motion()` is on the dots render static (no bob, no per-frame
/// animation ticks). Otherwise the motion is kept gentle: small amplitude,
/// slow cycle, sine easing.
const LOADING_BOB_MS: u64 = 1400;
const LOADING_DOT_AMPLITUDE: f32 = 11.0;

/// Pure vertical lift (in pre-scale px) of one bobbing loading dot. Returns
/// `0.0` when `reduce_motion` is on so the dot stays at rest; otherwise the
/// positive half of a staggered sine (hop up, rest, hop again). Kept pure and
/// GUI-free so the reduce-motion behaviour is unit-testable (see tests below).
pub(super) fn loading_dot_lift(reduce_motion: bool, phase: f32, delta: f32) -> f32 {
    if reduce_motion {
        return 0.0;
    }
    let t = ((delta + phase) % 1.0) * std::f32::consts::TAU;
    t.sin().max(0.0) * LOADING_DOT_AMPLITUDE
}

fn render_loading_placeholder(label: SharedString) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .bg(rgb(theme().bg_base))
        .child(render_loading_dots())
        .child(
            div()
                .text_lg()
                .text_color(rgb(theme().text_sub))
                .child(label),
        )
}

/// The three bobbing dots alone, for panes that load something smaller than a
/// whole tab (the PR conversation). Same outside-a-scroll-container rule.
pub(super) fn render_loading_dots() -> gpui::Div {
    use gpui::AnimationExt as _;
    let reduce_motion = theme::reduce_motion();
    // Commit-node colors: branch / accent / success — reads as a tiny graph.
    let colors = [theme().color_branch, theme().accent, theme().color_success];
    let mut dots = div()
        .flex()
        .flex_row()
        .items_end()
        .gap_2()
        .h(theme::scaled_px(22.0));
    for (i, color) in colors.into_iter().enumerate() {
        let phase = i as f32 * 0.15; // stagger: a little wave, left to right
        let dot = div()
            .w(theme::scaled_px(9.0))
            .h(theme::scaled_px(9.0))
            .rounded_full()
            .bg(rgb(color));
        // Reduce motion: render the dot static — skip `.with_animation` entirely
        // so no per-frame animation ticks are requested (the pinned GPUI build's
        // animation element does not honor reduce-motion itself).
        let dot = if reduce_motion {
            dot.into_any_element()
        } else {
            dot.with_animation(
                ("kagi-loading-dot", i),
                gpui::Animation::new(Duration::from_millis(LOADING_BOB_MS)).repeat(),
                move |el, delta| el.mb(theme::scaled_px(loading_dot_lift(false, phase, delta))),
            )
            .into_any_element()
        };
        dots = dots.child(dot);
    }
    dots
}

#[cfg(test)]
mod tests {
    use super::loading_dot_lift;

    #[test]
    fn loading_dot_lift_static_when_reduce_motion() {
        // Reduce motion on: the dot never lifts, at any phase/time.
        for &delta in &[0.0f32, 0.1, 0.25, 0.5, 0.75, 0.99] {
            assert_eq!(loading_dot_lift(true, 0.0, delta), 0.0);
            assert_eq!(loading_dot_lift(true, 0.3, delta), 0.0);
        }
    }

    #[test]
    fn loading_dot_lift_animates_when_enabled() {
        // Reduce motion off: the half-sine peaks (>0) somewhere in the cycle
        // and rests at 0 on the negative half — i.e. it actually moves.
        let peak = (0..100)
            .map(|i| loading_dot_lift(false, 0.0, i as f32 / 100.0))
            .fold(0.0f32, f32::max);
        assert!(peak > 1.0, "expected a visible hop, got peak {peak}");
        // The trough of the positive-half sine is a rest at exactly 0.
        assert_eq!(loading_dot_lift(false, 0.0, 0.5), 0.0);
    }
}
