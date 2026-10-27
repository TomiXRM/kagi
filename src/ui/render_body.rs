//! Body slot (sidebar | commit list | inspector / commit panel) split out of
//! `render.rs` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3). Child module of
//! `crate::ui`, so it keeps direct access to `KagiApp`'s private state. The WIP
//! / stash-graph row builders it consumes live in `render_wip.rs`. The Graph
//! header records painted column origins for its resize handles (#1011).

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
// ADR-0121 B1: bring the pane-item trait into scope for `is_open` / `render`.
use super::workspace::WorkspaceItem;
use super::*;

pub(crate) mod motion;
use motion::{RowKey, WipPresentation};

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

/// Measure a column's painted left edge, independent of its resizable width.
/// The drag handler can then derive width from an absolute pointer position even
/// if several mouse moves arrive before the next paint or the sidebar slides.
fn column_origin_probe(origin: std::rc::Rc<std::cell::Cell<Option<f32>>>) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, _| origin.set(Some(f32::from(bounds.origin.x))),
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

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
    /// Collect the same center-pane gates for rendering and keyboard routing.
    pub(super) fn workspace_inputs(
        &self,
        commit_panel_open: bool,
        commit_panel_present: bool,
        has_detail: bool,
    ) -> workspace::WorkspaceInputs {
        workspace::WorkspaceInputs {
            sidebar_visible: self.sidebar.visible,
            file_history_open: workspace::FileHistoryItem.is_open(self),
            ecosystem_open: workspace::EcosystemItem.is_open(self),
            branch_cleanup_open: workspace::BranchCleanupItem.is_open(self),
            pr_mode: workspace::PrModeItem.is_open(self),
            issues_mode: workspace::IssuesModeItem.is_open(self),
            loading: self.loading_tab().is_some(),
            diff_open: workspace::MainDiffItem.is_open(self),
            commit_panel_open,
            commit_panel_present,
            compare_open: workspace::CompareItem.is_open(self),
            inspector_visible: self.inspector_visible,
            has_detail,
            editor_mode: workspace::EditorWorkspaceItem.is_open(self),
        }
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
        commit_scroll_handle: UniformListScrollHandle,
        commit_panel_open: bool,
        // ADR-0118 (Phase 5.2): the Commit Panel is now an `Entity<CommitPanelView>`
        // that self-renders. render_body pushes the parent-owned render inputs
        // (active_wip / scaled width / smart-commit snapshot) into it, then embeds
        // `entity.clone()` as a child.
        commit_panel: Option<Entity<commit_panel::CommitPanelView>>,
        wip_diffstat: Option<WipDiffStat>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let compact = self.compact.enabled;
        // ADR-0120: rendering and keyboard routing resolve the same center
        // from the same gates; precedence lives only in `resolve_workspace`.
        let layout = workspace::resolve_workspace(&self.workspace_inputs(
            commit_panel_open,
            commit_panel.is_some(),
            detail.is_some(),
        ));
        if compact {
            if self.bottom_tab == BottomTab::Changes || self.compact.popup_owner.is_some() {
                let _ = self.prepare_commit_panel_presentation(window, cx);
            }
            if layout.center != workspace::CenterPane::CommitList {
                self.close_compact_composer(window, cx);
                self.compact.wip_bounds.set(None);
            }
        }

        // The sidebar follows its toggle (#955); the right slot follows
        // resolved visibility (#1001). Whole-workspace takeovers stay instant.
        let motion_now = kagi_ui_core::motion::now();
        let motion_instant = kagi_ui_core::motion::instant();
        // #1790: one sidebar for Graph, PRs and Issues, one toggle, one motion.
        let sidebar_shown = matches!(
            layout.left,
            workspace::LeftPane::Navigator
                | workspace::LeftPane::PrList
                | workspace::LeftPane::IssueList
        );
        self.panel_motion.sync_sidebar(
            sidebar_shown,
            self.sidebar.visible,
            motion_now,
            motion_instant || compact,
        );
        let right_shown = matches!(
            layout.right,
            workspace::RightPane::Inspector
                | workspace::RightPane::Compare
                | workspace::RightPane::CommitPanel
        );
        let mode = self.workspace_mode();
        let graph_body = mode == workspace_mode::WorkspaceMode::Graph
            && matches!(
                layout.center,
                workspace::CenterPane::CommitList | workspace::CenterPane::Diff
            );
        self.panel_motion.sync_right(
            layout.right,
            selected,
            graph_body,
            motion_now,
            motion_instant || compact,
        );
        let sidebar_fraction = if compact {
            0.
        } else {
            self.panel_motion.left.visible(motion_now)
        };
        let right_fraction = if compact {
            0.
        } else {
            self.panel_motion.right.visible(motion_now)
        };
        let right_pane = if compact {
            None
        } else if right_shown {
            Some(layout.right)
        } else {
            self.panel_motion.right_pane.filter(|_| right_fraction > 0.)
        };
        let right_width = if right_pane == Some(workspace::RightPane::CommitPanel) {
            self.commit_panel_viewport_width(window)
        } else {
            self.panel_width
        };
        self.pane_fit = super::pane_budget::fit(
            self.sidebar.width,
            right_width,
            f32::from(window.viewport_size().width),
            !compact && (sidebar_shown || sidebar_fraction > 0.),
            !compact && (right_shown || right_fraction > 0.),
            right_pane != Some(workspace::RightPane::CommitPanel),
        );
        let sidebar_width = self.pane_fit.sidebar;
        let right_width = self.pane_fit.right;
        let center_width = (f32::from(window.viewport_size().width)
            - if compact {
                theme::scaled(super::compact::RAIL_WIDTH)
            } else {
                0.
            }
            - theme::scaled(sidebar_width + 4.) * sidebar_fraction
            - theme::scaled(right_width + 4.) * right_fraction)
            .max(0.);
        let columns = super::graph_columns::fit(
            self.badge_col_w,
            self.graph_col_w,
            center_width,
            f32::from(window.rem_size()) * 1.5, // px_3 on both row edges.
        );
        self.graph_column_fit = columns;
        let (badge_col_w, graph_col_w) = (columns.badge, columns.graph);
        // Tab switches, zoom and fitting all change the visible lane window.
        let lane_count = self.view().rows.first().map(|r| r.lane_count).unwrap_or(0);
        let max = (lane_count as f32 * graph_view::lane_w() - theme::scaled(graph_col_w)).max(0.);
        if self.ui().graph_scroll_x > max {
            self.with_ui(|ui| ui.graph_scroll_x = max);
        }
        // Build divider 1: sidebar | main.
        let divider1 = div()
            .id("divider-sidebar")
            .group("divider-sidebar")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .relative()
            .when(cfg!(feature = "gui-e2e"), |divider| {
                divider.child(e2e::measure_inside("divider-sidebar"))
            })
            .bg(rgb(theme().sidebar))
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme().surface))
                    .group_hover("divider-sidebar", |s| s.bg(rgb(theme().ui_accent())))
                    .group_drag_over::<DividerDrag>("divider-sidebar", |s| {
                        s.bg(rgb(theme().ui_accent()))
                    })
                    .when(cfg!(feature = "gui-e2e"), |line| {
                        line.child(e2e::measure_inside("divider-sidebar-line"))
                    }),
            )
            .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_col_resize())
            .drag_over::<DividerDrag>(|style, drag, _, _| {
                if drag.kind == DividerKind::Sidebar {
                    style.bg(rgb(theme().ui_accent()))
                } else {
                    style
                }
            })
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
        let composer_row = if self.compact.enabled {
            wip_params
                .iter()
                .position(|params| self.compact_wip_anchor(&params.5, cx))
        } else {
            None
        };
        if self.compact.enabled && self.compact.popup_owner.is_some() && composer_row.is_none() {
            self.close_compact_composer(window, cx);
            self.compact.wip_bounds.set(None);
        }
        let mut wip_passing_lanes = Vec::new();
        for anchor in wip_anchors.iter().flatten() {
            let trace = (anchor.lane, anchor.color);
            if !wip_passing_lanes.contains(&trace) {
                wip_passing_lanes.push(trace);
            }
        }
        let wip_count = wip_params.len();
        let prefix_count = wip_count + self.view().stash_graph_rows.len();

        if let Some(session) = self.active_session().filter(|_| {
            mode == workspace_mode::WorkspaceMode::Graph
                && layout.center == workspace::CenterPane::CommitList
        }) {
            let selected_key = if commit_panel_open {
                // The ordinary panel canonicalizes its path on open; the current
                // row remains identified by the session's path (including fixture
                // aliases). Foreign metadata is the authoritative target distinction.
                self.ui()
                    .commit_panel
                    .as_ref()
                    .and_then(|panel| {
                        let panel = panel.read(cx);
                        if panel.foreign.is_some() {
                            Some(RowKey::Wip(panel.repo_path.clone()))
                        } else {
                            self.repo_path.clone().map(RowKey::Wip)
                        }
                    })
                    .or_else(|| self.repo_path.clone().map(RowKey::Wip))
            } else {
                self.ui()
                    .compare_view
                    .as_ref()
                    .and_then(|compare| {
                        if let CompareTarget::Commit(id) = &compare.read(cx).view().target {
                            self.view()
                                .stashes
                                .iter()
                                .any(|stash| &stash.target == id)
                                .then(|| RowKey::Stash(id.clone()))
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        selected
                            .and_then(|ix| self.view().rows.get(ix))
                            .map(|row| RowKey::Commit(row.id.clone()))
                    })
            };
            let mut passing = Vec::new();
            let wip_presentations = wip_params
                .iter()
                .zip(&wip_anchors)
                .map(|(params, lane)| {
                    let path = match &params.5 {
                        WipRowClick::CommitPanel => self.repo_path.clone().unwrap_or_default(),
                        WipRowClick::Worktree { path, .. } => path.clone(),
                    };
                    let row = WipPresentation {
                        key: RowKey::Wip(path),
                        params: params.clone(),
                        lane: *lane,
                        passing: passing.clone(),
                    };
                    if let Some(lane) = lane {
                        let trace = (lane.lane, lane.color);
                        if !passing.contains(&trace) {
                            passing.push(trace);
                        }
                    }
                    row
                })
                .collect();
            let view = self.reads.get(Some(session));
            if let Some(ui) = self.ui.get_mut(&session) {
                let selection_moving =
                    ui.graph_motion
                        .sync_selection(selected_key, motion_now, motion_instant);
                let rows_moving = ui.graph_motion.sync_rows(
                    view,
                    ui.view_publish_gen,
                    wip_presentations,
                    &commit_scroll_handle,
                    px(row_height(self.graph_compact)),
                    motion_now,
                    motion_instant,
                );
                kagi_ui_core::motion::request_frame(window, selection_moving || rows_moving);
            }
        }

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
                    .relative()
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
                                    .text_color(rgb(theme().accent_text_on(theme().surface)))
                                    .hover(|st| {
                                        st.bg(rgb(theme().selected)).text_color(rgb(
                                            theme().accent_text_on(theme().selected)
                                        ))
                                    })
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
                    })
                    .child(column_origin_probe(self.badge_col_origin_x.clone())),
            )
            // Handle between badge and graph columns
            .child(
                div()
                    .id("divider-badge-col")
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .relative()
                    .when(cfg!(feature = "gui-e2e"), |divider| {
                        divider.child(e2e::measure_inside("divider-badge-col"))
                    })
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_col_resize())
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
                    this.seed_settings(cx);
                    cx.notify();
                });
                div()
                    .w(theme::scaled_px(graph_col_w))
                    .relative()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .on_scroll_wheel(cx.listener(
                        move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                            this.scroll_graph_by(&e.delta, graph_col_w, cx);
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
                                theme().accent_text_on(theme().panel)
                            } else {
                                theme().text_muted
                            }))
                            .hover(|s| s.text_color(rgb(theme().accent_text_on(theme().panel))))
                            .on_click(compact_click)
                            .child(SharedString::from(if is_compact { "▥" } else { "▤" })),
                    )
                    .child(column_origin_probe(self.graph_col_origin_x.clone()))
            })
            // Handle between graph and message columns
            .child(
                div()
                    .id("divider-graph-col")
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .relative()
                    .when(cfg!(feature = "gui-e2e"), |divider| {
                        divider.child(e2e::measure_inside("divider-graph-col"))
                    })
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_col_resize())
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

        let commit_list_col =
            div()
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
                .when(compact, |column| {
                    column
                        .relative()
                        .child(e2e::measure_inside("compact-graph"))
                })
                // ── Column header row (T030) ──────────────
                .child(
                    col_header
                        .relative()
                        .child(e2e::measure_inside("graph-column-header")),
                )
                // WIP, stash and commit rows share one scroll origin.
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h(px(0.))
                        .child(e2e::measure_inside("toast-content-anchor"))
                        .child({
                            if self.loading_tab().is_none()
                                && self.remote_view.is_none()
                                && self.view().status_summary.is_unborn
                                && prefix_count + row_count == 0
                            {
                                // The cached unborn page replaces the entire list, so
                                // release its retained focus before its tracker disappears.
                                if self
                                    .graph_focus
                                    .as_ref()
                                    .is_some_and(|focus| focus.is_focused(window))
                                {
                                    if let Some(root) = &self.root_focus {
                                        root.focus(window, cx);
                                    }
                                }
                                let mut empty = div()
                                    .relative()
                                    .size_full()
                                    .child(self.render_unborn_empty_state(cx));
                                let height = px(row_height(self.graph_compact));
                                for (ghost, offset, opacity) in
                                    self.ui().graph_motion.ghosts_after(0, height)
                                {
                                    let (_, ordinal, label, count, diffstat, click, is_worktree) =
                                        &ghost.params;
                                    let content = self.render_wip_row(
                                        ghost.lane.map_or(*ordinal, |lane| lane.color),
                                        label.clone(),
                                        *count,
                                        *diffstat,
                                        click.clone(),
                                        *is_worktree,
                                        ghost.lane.map(|lane| lane.lane),
                                        &ghost.passing,
                                        badge_col_w,
                                        graph_col_w,
                                        self.ui().graph_scroll_x,
                                        (0, 0),
                                        Some(&ghost.key),
                                        cx,
                                    );
                                    empty = empty.child(
                                        div()
                                            .absolute()
                                            .top(offset)
                                            .w_full()
                                            .h(height)
                                            .opacity(opacity)
                                            .child(content)
                                            .child(e2e::measure_inside("graph-wip-ghost")),
                                    );
                                }
                                empty.into_any_element()
                            } else {
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
                                        if this.compact.enabled
                                            && this.compact.popup_owner.is_some()
                                            && composer_row
                                                .is_some_and(|row| !this.compact_wip_visible(row))
                                        {
                                            this.close_compact_composer(_window, cx);
                                            this.compact.wip_bounds.set(None);
                                            cx.notify();
                                        }
                                        let rows_len = this.view().rows.len();
                                        let compact = this.graph_compact;
                                        let requested = range;
                                        let slot_height = px(row_height(compact));
                                        // ItemSize.item is the viewport; contents / final
                                        // item count is uniform_list's measured row pitch.
                                        let height = this
                                            .ui()
                                            .commit_scroll_handle
                                            .0
                                            .borrow()
                                            .last_item_size
                                            .filter(|_| prefix_count + rows_len > 0)
                                            .map_or(slot_height, |size| {
                                                size.contents.height
                                                    / (prefix_count + rows_len) as f32
                                            });
                                        let range = this
                                            .ui()
                                            .graph_motion
                                            .presentation_range(requested.clone(), height);
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
                                                        anchor.map(|anchor| anchor.lane),
                                                        &passing,
                                                        badge_col_w,
                                                        graph_col_w,
                                                        this.ui().graph_scroll_x,
                                                        (i, prefix_count + rows_len),
                                                        None,
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
                                                badge_col_w,
                                                graph_col_w,
                                                this.ui().graph_scroll_x,
                                                &wip_passing_lanes,
                                                range.start.saturating_sub(wip_count)
                                                    ..range.end.min(prefix_count) - wip_count,
                                                (wip_count, prefix_count + rows_len),
                                                cx,
                                            ));
                                        }
                                        let commit_range = range
                                            .start
                                            .saturating_sub(prefix_count)
                                            .min(rows_len)
                                            ..range.end.saturating_sub(prefix_count).min(rows_len);
                                        els.extend(
                                            render_rows(
                                                &this.view().rows,
                                                &this.avatars.images,
                                                commit_range,
                                                this.window_active,
                                                badge_col_w,
                                                graph_col_w,
                                                columns.subject_min,
                                                compact,
                                                this.ui().graph_scroll_x,
                                                &this.view().stash_graph_lanes,
                                                this.view()
                                                    .branch_solo
                                                    .as_ref()
                                                    .map(|solo| &solo.visible_commits),
                                                (prefix_count, prefix_count + rows_len),
                                                &this.context_anchor,
                                                &this.ui().graph_motion,
                                                cx,
                                            )
                                            .into_iter()
                                            .map(gpui::IntoElement::into_any_element),
                                        );
                                        let mut slots = Vec::with_capacity(requested.len());
                                        let mut leading = Vec::new();
                                        for (offset, row) in els.into_iter().enumerate() {
                                            let index = range.start + offset;
                                            let motion = &this.ui().graph_motion;
                                            let sample = motion
                                                .key(index)
                                                .map(|key| motion.sample(key, height));
                                            let mut slot =
                                                div().relative().w_full().h(slot_height).child(
                                                    div()
                                                        .relative()
                                                        .top(sample.map_or(px(0.), |s| s.offset))
                                                        .w_full()
                                                        .h(slot_height)
                                                        .opacity(sample.map_or(1., |s| s.opacity))
                                                        .child(row),
                                                );
                                            for (ghost, offset, opacity) in
                                                motion.ghosts_after(index, height)
                                            {
                                                let (
                                                    _,
                                                    ordinal,
                                                    label,
                                                    count,
                                                    diffstat,
                                                    click,
                                                    is_worktree,
                                                ) = &ghost.params;
                                                let content = this.render_wip_row(
                                                    ghost.lane.map_or(*ordinal, |lane| lane.color),
                                                    label.clone(),
                                                    *count,
                                                    *diffstat,
                                                    click.clone(),
                                                    *is_worktree,
                                                    ghost.lane.map(|lane| lane.lane),
                                                    &ghost.passing,
                                                    badge_col_w,
                                                    graph_col_w,
                                                    this.ui().graph_scroll_x,
                                                    (index, prefix_count + rows_len),
                                                    Some(&ghost.key),
                                                    cx,
                                                );
                                                slot = slot.child(
                                                    div()
                                                        .absolute()
                                                        .top(offset)
                                                        .w_full()
                                                        .h(slot_height)
                                                        .opacity(opacity)
                                                        .child(content)
                                                        .child(e2e::measure_inside(
                                                            "graph-wip-ghost",
                                                        )),
                                                );
                                            }
                                            if requested.contains(&index) {
                                                // Paint leading fringe before the first real slot;
                                                // trailing fringe is appended after the last one.
                                                if index == requested.start && !leading.is_empty() {
                                                    slot = div()
                                                        .relative()
                                                        .w_full()
                                                        .h(slot_height)
                                                        .children(std::mem::take(&mut leading))
                                                        .child(slot);
                                                }
                                                slots.push(slot);
                                            } else {
                                                let before = index < requested.start;
                                                let anchor = if before {
                                                    requested.start
                                                } else {
                                                    requested.end.saturating_sub(1)
                                                };
                                                let fringe = slot
                                                    .absolute()
                                                    .top(height * (index as f32 - anchor as f32))
                                                    .into_any_element();
                                                if before {
                                                    leading.push(fringe);
                                                } else if let Some(last) = slots.last_mut() {
                                                    *last = std::mem::replace(last, div())
                                                        .child(fringe);
                                                }
                                            }
                                        }
                                        slots
                                            .into_iter()
                                            .map(IntoElement::into_any_element)
                                            .collect::<Vec<_>>()
                                    },
                                ),
                            )
                            // T028: wire scroll handle so jump_to_branch can scroll the list.
                            .track_scroll(&commit_scroll_handle)
                            .flex_1()
                            .min_h(px(0.)),
                        );
                                // One retained stop for the virtual commit list, not
                                // one per recycled row. The inset ring leaves its
                                // viewport and row geometry unchanged.
                                let graph_focus = self
                                    .graph_focus
                                    .get_or_insert_with(|| cx.focus_handle())
                                    .clone()
                                    .tab_index(0)
                                    .tab_stop(true);
                                let ring_visible = graph_focus.is_focused(window)
                                    && window.last_input_was_keyboard();
                                let list = with_vertical_scrollbar(
                                    "commit-list-scroll",
                                    &scrollbar_handle,
                                    super::list_a11y::list_box(
                                        "commit-list",
                                        rows.track_focus(&graph_focus),
                                        Msg::A11yCommitList.t(),
                                    ),
                                    true,
                                )
                                .relative()
                                .when(ring_visible, |list| {
                                    list.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .border_2()
                                            .border_color(rgb(theme().ui_accent())),
                                    )
                                })
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
                                    .into_any_element()
                            }
                        })
                        .children(self.render_toasts(
                            window,
                            super::busy::ToastPlacement::Repository,
                            crate::ui::toast_stack::ToastAudience::Repository,
                            cx,
                        )),
                );

        // The Graph page's panes are drawn this frame only while the sidebar
        // is (or is still closing) and shows that page — the PRs and Issues
        // pages are lists of their own. Their keyboard state follows this
        // frame, not the last (#981 review).
        let panes_front = sidebar_fraction > 0.
            && !matches!(
                mode,
                workspace_mode::WorkspaceMode::Prs | workspace_mode::WorkspaceMode::Issues
            );
        let rail_front = compact && super::compact_sidebar::is_open(window, cx);
        self.sync_sidebar_focus(panes_front || rail_front, window, cx);

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
        if compact {
            body_row = body_row.child(super::compact_sidebar::render(self, window, cx));
        } else if sidebar_fraction > 0. {
            // `mode` is this frame's, read above.
            let sidebar = div()
                .flex()
                .flex_row()
                .size_full()
                .child(workspace_mode::render_sidebar_pages(self, mode, cx))
                .child(divider1);
            body_row = body_row.child(
                kagi_ui_core::motion::clip(
                    "sidebar-clip",
                    kagi_ui_core::motion::Axis::Horizontal,
                    kagi_ui_core::motion::Anchor::End,
                    theme::scaled_px(sidebar_width + 4.),
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
        // Fit only the presentation; shrinking a window or increasing zoom
        // must not replace the larger preferred height.
        let panel_height = self.bottom_panel_body_height(
            window,
            matches!(layout.center, workspace::CenterPane::Editor),
        );
        let panel_tab = self.bottom_tab;
        let mut bottom_panel = self
            .render_bottom_panel_slot(panel_visible, panel_height, panel_tab, window, cx)
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
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .overflow_hidden();
        let mut takeover_notifications = !matches!(
            layout.center,
            workspace::CenterPane::CommitList | workspace::CenterPane::Editor
        );
        let center_content = match workspace::center_item(layout.center) {
            Some(item) => match item.render(
                self,
                &layout,
                if nested { bottom_panel.take() } else { None },
                window,
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
                    takeover_notifications = false;
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
        // Like Home and Settings, takeovers reserve space only for persistent
        // cards. Transient feedback stays overlaid; a fallback commit list
        // already owns its stack and must not acquire a second one here.
        let persistent_lane = self.toast_stack.as_ref().is_some_and(|stack| {
            stack
                .read(cx)
                .persistent_lane_open(crate::ui::toast_stack::ToastAudience::Repository)
        });
        let center_content = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .when(takeover_notifications && persistent_lane, |el| {
                el.child(
                    div()
                        .relative()
                        .flex()
                        .justify_end()
                        .flex_shrink_0()
                        .child(e2e::measure_inside("toast-content-anchor"))
                        .children(self.render_toasts(
                            window,
                            super::busy::ToastPlacement::PersistentLane,
                            crate::ui::toast_stack::ToastAudience::Repository,
                            cx,
                        )),
                )
            })
            .child(center_content.when(takeover_notifications, |el| {
                el.child(e2e::measure_inside("takeover-page-controls"))
            }))
            .when(takeover_notifications, |el| {
                el.children(self.render_toasts(
                    window,
                    super::busy::ToastPlacement::Transient,
                    crate::ui::toast_stack::ToastAudience::Repository,
                    cx,
                ))
            });
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
            .group("divider-panel")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .relative()
            .when(cfg!(feature = "gui-e2e"), |divider| {
                divider.child(e2e::measure_inside("divider-panel"))
            })
            .bg(rgb(theme().panel))
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w(px(1.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(rgb(theme().surface))
                    .group_hover("divider-panel", |s| s.bg(rgb(theme().ui_accent())))
                    .group_drag_over::<DividerDrag>("divider-panel", |s| {
                        s.bg(rgb(theme().ui_accent()))
                    })
                    .when(cfg!(feature = "gui-e2e"), |line| {
                        line.child(e2e::measure_inside("divider-panel-line"))
                    }),
            )
            .hover(|style| style.bg(rgb(theme().ui_accent())).cursor_col_resize())
            .drag_over::<DividerDrag>(|style, drag, _, _| {
                if drag.kind == DividerKind::Panel {
                    style.bg(rgb(theme().ui_accent()))
                } else {
                    style
                }
            })
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

        let right_element = if !right_shown && right_pane == Some(workspace::RightPane::Inspector) {
            self.panel_motion.inspector_row.and_then(|row| {
                self.ui_mut()?.sync_inspector_commit_files(row);
                workspace::render_inspector_body(self, Some(row), None, window, cx)
            })
        } else {
            right_pane
                .and_then(workspace::right_item)
                .and_then(|item| item.render(self, &layout, None, window, cx))
        };
        let compact_inspector_shown = compact
            && panel_visible > 0.
            && panel_tab == BottomTab::Changes
            && (self.ui().selected.is_some()
                || (self.ui().compare_view.is_some() && !self.ui().commit_panel_open));
        if !compact_inspector_shown
            && (!matches!(
                right_pane,
                Some(workspace::RightPane::Inspector | workspace::RightPane::Compare)
            ) || right_element.is_none())
        {
            self.ui()
                .inspector_model
                .yield_focus(self.root_focus.as_ref(), window, cx);
        }
        if let Some(el) = right_element {
            let pane = div()
                .flex()
                .flex_row()
                .size_full()
                .child(divider2)
                .child(el);
            body_row = body_row.child(
                kagi_ui_core::motion::clip(
                    "right-pane-clip",
                    kagi_ui_core::motion::Axis::Horizontal,
                    kagi_ui_core::motion::Anchor::Start,
                    theme::scaled_px(right_width + 4.),
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
            _ => body_row,
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
            #[cfg(feature = "gui-e2e")]
            super::e2e::system_accessibility::record_loading_animation(false);
            dot.into_any_element()
        } else {
            #[cfg(feature = "gui-e2e")]
            super::e2e::system_accessibility::record_loading_animation(true);
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
