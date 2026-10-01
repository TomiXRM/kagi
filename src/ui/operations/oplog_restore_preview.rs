//! The restore card's "graph after" (#334 slice 2c, ADR-0214 §6): built once
//! when the card opens from the tab's loaded commits and the plan's restores
//! (`kagi_domain::restore_preview`), drawn with the commit graph's own row
//! painter. Display only — nothing is read from or written to the repository.

use crate::ui::graph_view;
use crate::ui::modals::oplog_restore::RestoreGraphPreview;
use crate::ui::tab_view::TabViewState;
use crate::ui::theme::{self, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, rgb, InteractiveElement, IntoElement, ParentElement, SharedString, Styled};
use kagi_domain::restore_preview::{self, FixedRoots, LoadedCommit, RestorePreview};
use kagi_git::OperationPlan;
use kagi_ui_core::i18n;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Lanes drawn before the rail is clipped (wider graphs keep their layout).
const MAX_LANES: usize = 8;

/// `None` when the plan moves nothing (a blocked plan carries no restores).
pub(crate) fn build(view: &TabViewState, plan: &OperationPlan) -> Option<Arc<RestoreGraphPreview>> {
    let restores = kagi_domain::ref_restore::from_lines(&plan.preview_commits).ok()?;
    if restores.is_empty() {
        return None;
    }
    let commits: Vec<LoadedCommit> = view
        .rows
        .iter()
        .map(|r| LoadedCommit {
            id: r.id.clone(),
            parents: r.parents.clone(),
        })
        .collect();
    let branches: BTreeMap<String, kagi_git::CommitId> = view
        .branch_targets
        .iter()
        .map(|(name, oid)| (format!("refs/heads/{name}"), oid.clone()))
        .collect();
    // What the restore never moves keeps its commits: remote branches, tags,
    // detached worktree HEADs (an attached one follows its branch), stashes.
    let fixed = FixedRoots(
        view.remote_branches
            .iter()
            .map(|b| b.target.clone())
            .chain(view.tags.iter().map(|t| t.target.clone()))
            .chain(
                view.worktrees
                    .iter()
                    .filter(|w| w.branch.is_none())
                    .filter_map(|w| w.head.clone()),
            )
            .chain(view.stashes.iter().filter_map(|s| s.base.clone()))
            .collect(),
    );
    let graph = restore_preview::preview(&commits, &branches, &fixed, &restores);
    let summaries = match &graph {
        RestorePreview::Graph { rows, .. } => rows
            .iter()
            .map(|row| {
                view.commit_row_index
                    .get(&row.id)
                    .and_then(|i| view.rows.get(*i))
                    .map(|r| r.summary.clone())
                    .unwrap_or_default()
            })
            .collect(),
        RestorePreview::NotLoaded { .. } => Vec::new(),
    };
    Some(Arc::new(RestoreGraphPreview { graph, summaries }))
}

fn chip(label: String, color: u32) -> gpui::AnyElement {
    div()
        .flex_shrink_0()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(rgb(color))
        .text_color(rgb(color))
        .child(SharedString::from(label))
        .into_any_element()
}

/// The section drawn on the card after the warnings.
pub(crate) fn render(preview: &RestoreGraphPreview) -> gpui::AnyElement {
    let heading = |text: String| {
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(text))
    };
    let section = div()
        .id("restore-preview")
        .relative()
        .flex()
        .flex_col()
        .gap_1()
        .text_xs()
        .child(crate::ui::e2e::measure_inside("restore-preview"));
    let (rows, lane_count, removed, above, below) = match &preview.graph {
        RestorePreview::NotLoaded { refname, oid } => {
            return section
                .child(heading(i18n::oplog_panel::preview_heading(0)))
                .child(
                    div()
                        .relative()
                        .text_color(rgb(theme().color_warning))
                        .child(SharedString::from(i18n::oplog_panel::preview_not_loaded(
                            refname.trim_start_matches("refs/heads/"),
                            oid.get(..7).unwrap_or(oid),
                        )))
                        .child(crate::ui::e2e::measure_inside(
                            "restore-preview-unavailable",
                        )),
                )
                .into_any_element();
        }
        RestorePreview::Graph {
            rows,
            lane_count,
            removed,
            hidden_above,
            hidden_below,
        } => (rows, *lane_count, *removed, *hidden_above, *hidden_below),
    };
    let rail_w = graph_view::lane_w() * lane_count.clamp(1, MAX_LANES) as f32 + 4.;
    let more = |n: usize| (n > 0).then(|| heading(i18n::oplog_panel::preview_more(n)));
    section
        .child(
            heading(i18n::oplog_panel::preview_heading(removed))
                .relative()
                .child(crate::ui::e2e::measure_inside(format!(
                    "restore-preview-removed-{removed}"
                ))),
        )
        .children(more(above))
        .children(rows.iter().enumerate().map(|(n, row)| {
            let summary = preview.summaries.get(n).cloned().unwrap_or_default();
            div()
                .relative()
                .h(theme::scaled_px(graph_view::ROW_H))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(rail_w))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(
                            graph_view::graph_canvas(
                                row.graph.lane,
                                row.graph.color,
                                row.graph.edges.clone(),
                                graph_view::GraphNode::Commit {
                                    is_head: false,
                                    is_merge: false,
                                },
                                false,
                                0.,
                                0.,
                                Vec::new(),
                            )
                            .size_full(),
                        ),
                )
                .children(
                    row.moved_here
                        .iter()
                        .map(|b| chip(format!("{b} ←"), theme().color_warning)),
                )
                .children(
                    row.branches
                        .iter()
                        .map(|b| chip(b.clone(), theme().color_branch)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(rgb(theme().text_sub))
                        .child(summary),
                )
                .when(!row.moved_here.is_empty(), |d| {
                    d.child(crate::ui::e2e::measure_inside(format!(
                        "restore-preview-moved-{}-{}",
                        row.moved_here.join(","),
                        row.id.short()
                    )))
                })
                .child(crate::ui::e2e::measure_inside(format!(
                    "restore-preview-row-{n}"
                )))
        }))
        .children(more(below))
        .into_any_element()
}
