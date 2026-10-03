//! Restore's after-view uses the already loaded commit graph and pure reachability.

use crate::ui::graph_view;
use crate::ui::graph_window::{self, Rail};
use crate::ui::modals::oplog_restore::RestoreGraphPreview;
use crate::ui::tab_view::TabViewState;
use crate::ui::theme::{self, theme};
use gpui::prelude::*;
use gpui::{div, px, rgb, SharedString};
use kagi_domain::graph::GraphRow;
use kagi_domain::head::Head;
use kagi_domain::ref_restore::RefRestore;
use kagi_domain::restore_preview::{self, FixedRoots, LoadedCommit, RestorePreview};
use kagi_ui_core::i18n::{self, oplog_panel::OplogPanelMsg, Msg};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The canonical rows have already been parsed by the modal opener.
pub(crate) fn build(
    view: &TabViewState,
    restores: &[RefRestore],
    head: &Head,
) -> Option<Arc<RestoreGraphPreview>> {
    if restores.is_empty() {
        return None;
    }
    let head_branch = match head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    if restores.iter().any(|r| r.refname.starts_with("refs/tags/")) {
        return Some(Arc::new(RestoreGraphPreview {
            graph: RestorePreview::TagChange,
            summaries: Vec::new(),
            rails: Vec::new(),
            merges: Vec::new(),
            head_branch,
        }));
    }
    // Solo filters the on-screen list, not the repository's loaded history.
    let (rows, row_index) = match &view.branch_solo {
        Some(solo) => (&solo.saved_rows, &solo.saved_row_index),
        None => (&view.rows, &view.commit_row_index),
    };
    let commits: Vec<LoadedCommit> = rows
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
    // Other roots retain commits even when local branches move.
    let fixed = FixedRoots(
        view.remote_branches
            .iter()
            .map(|b| b.target.clone())
            .chain(view.tags.iter().map(|t| t.target.clone()))
            .chain(view.pr_heads.iter().cloned())
            .chain(
                view.worktrees
                    .iter()
                    .filter(|w| w.branch.is_none())
                    .filter_map(|w| w.head.clone()),
            )
            .chain(view.stashes.iter().filter_map(|s| s.base.clone()))
            .collect(),
    );
    let graph = restore_preview::preview(&commits, &branches, &fixed, restores);
    let mut summaries = Vec::new();
    let mut rails = Vec::new();
    let mut merges = Vec::new();
    if let RestorePreview::Graph { rows: drawn, .. } = &graph {
        for row in drawn {
            let source = row_index
                .get(&row.id)
                .and_then(|i| rows.get(*i))
                .expect("preview rows come from loaded commits");
            summaries.push(source.summary.clone());
            rails.push(GraphRow {
                commit: source.id.clone(),
                lane: source.lane,
                color: source.node_color,
                edges: source.edges.clone(),
            });
            merges.push(source.is_merge);
        }
    }
    Some(Arc::new(RestoreGraphPreview {
        graph,
        summaries,
        rails,
        merges,
        head_branch,
    }))
}

pub(crate) fn heading_text(graph: &RestorePreview) -> String {
    match graph {
        RestorePreview::Graph { removed, .. } => i18n::oplog_panel::preview_heading(*removed),
        RestorePreview::NotLoaded { .. } | RestorePreview::TagChange => {
            Msg::OplogPanel(OplogPanelMsg::PreviewUnavailableHeading)
                .t()
                .to_string()
        }
    }
}

/// Copy the projection (or its honest unavailability), not old prose warnings.
pub(crate) fn clipboard_text(preview: &RestoreGraphPreview) -> String {
    let mut out = format!("{}\n", heading_text(&preview.graph));
    match &preview.graph {
        RestorePreview::NotLoaded { .. } => out.push_str(i18n::oplog_panel::preview_not_loaded()),
        RestorePreview::TagChange => {
            out.push_str(Msg::OplogPanel(OplogPanelMsg::PreviewTagChange).t())
        }
        RestorePreview::Graph {
            rows,
            hidden_above,
            hidden_below,
            ..
        } => {
            if *hidden_above > 0 {
                out.push_str(&format!(
                    "  {}\n",
                    i18n::oplog_panel::preview_more(*hidden_above)
                ));
            }
            for (n, row) in rows.iter().enumerate() {
                let mut refs: Vec<String> =
                    row.moved_here.iter().map(|b| format!("{b} ←")).collect();
                refs.extend(row.branches.iter().cloned());
                let label = if refs.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", refs.join(", "))
                };
                let ghost = if row.off_branch { " (off branch)" } else { "" };
                let summary = preview.summaries.get(n).map(|s| s.as_ref()).unwrap_or("");
                out.push_str(&format!("  {}{label}{ghost} {summary}\n", row.id.short()));
            }
            if *hidden_below > 0 {
                out.push_str(&format!(
                    "  {}\n",
                    i18n::oplog_panel::preview_more(*hidden_below)
                ));
            }
        }
    }
    out
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

/// The rail stores its width unscaled, but graph_canvas paints scaled lanes.
fn preview_scroll(lanes: usize, focus_lane: usize, width: f32, zoom: f32) -> f32 {
    let painted_width = width * zoom;
    let painted_lane_w = graph_view::LANE_W * zoom;
    graph_window::follow_scroll(focus_lane, painted_width, painted_lane_w).min(
        graph_window::max_scroll(lanes, painted_width, painted_lane_w),
    )
}

pub(crate) fn render(preview: &RestoreGraphPreview) -> gpui::AnyElement {
    let muted = |text: String| {
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
    let (rows, removed, above, below) = match &preview.graph {
        RestorePreview::NotLoaded { .. } => {
            return section
                .child(
                    div()
                        .relative()
                        .text_color(rgb(theme().color_warning))
                        .child(i18n::oplog_panel::preview_not_loaded())
                        .child(crate::ui::e2e::measure_inside(
                            "restore-preview-unavailable",
                        )),
                )
                .into_any_element()
        }
        RestorePreview::TagChange => {
            return section
                .child(
                    div()
                        .relative()
                        .text_color(rgb(theme().color_warning))
                        .child(Msg::OplogPanel(OplogPanelMsg::PreviewTagChange).t())
                        .child(crate::ui::e2e::measure_inside(
                            "restore-preview-unavailable",
                        )),
                )
                .into_any_element()
        }
        RestorePreview::Graph {
            rows,
            removed,
            hidden_above,
            hidden_below,
        } => (rows, *removed, *hidden_above, *hidden_below),
    };
    let columns = graph_window::lane_columns(preview.rails.iter().flat_map(|graph| {
        std::iter::once(graph.lane).chain(
            graph
                .edges
                .iter()
                .flat_map(|edge| [edge.from_lane, edge.to_lane]),
        )
    }));
    let width = graph_window::gutter_width(columns.len());
    let focus_lane = rows
        .iter()
        .position(|row| {
            preview
                .head_branch
                .as_deref()
                .is_some_and(|head| row.moved_here.iter().any(|b| b == head))
        })
        .or_else(|| rows.iter().position(|row| !row.moved_here.is_empty()))
        .and_then(|n| preview.rails.get(n))
        .map(|graph| graph_window::column_of(&columns, graph.lane))
        .unwrap_or(0);
    let scroll = preview_scroll(columns.len(), focus_lane, width, theme::scaled(1.));
    let rail = Rail {
        width,
        scroll,
        columns: &columns,
        avatars: None,
    };
    let list = div()
        .id("restore-preview-rows")
        .relative()
        .flex()
        .flex_col()
        .min_h(px(0.))
        // Fit short previews; cap long histories at six rows with independent scrolling.
        .h(theme::scaled_px(
            rows.len().min(6) as f32 * graph_view::ROW_H,
        ))
        .overflow_y_scroll()
        .child(crate::ui::e2e::measure_inside("restore-preview-rows"))
        .children(rows.iter().enumerate().map(|(n, row)| {
            let graph = &preview.rails[n];
            let is_head = preview.head_branch.as_deref().is_some_and(|head| {
                !row.off_branch
                    && (row.moved_here.iter().any(|branch| branch == head)
                        || row.branches.iter().any(|branch| branch == head))
            });
            div()
                .relative()
                .flex_shrink_0()
                .h(theme::scaled_px(graph_view::ROW_H))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .when(row.off_branch, |el| {
                    el.opacity(graph_window::CONTEXT_OPACITY)
                })
                .child(graph_window::render_rail(
                    graph.lane,
                    graph.color,
                    &graph.edges,
                    graph_view::GraphNode::Commit {
                        is_head,
                        is_merge: preview.merges[n],
                    },
                    None,
                    graph_view::ROW_H,
                    &rail,
                ))
                .children(row.moved_here.iter().map(|b| {
                    chip(
                        format!("{b} ←"),
                        if preview.head_branch.as_deref() == Some(b.as_str()) {
                            theme().color_head
                        } else {
                            theme().color_warning
                        },
                    )
                }))
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
                        .child(preview.summaries[n].clone()),
                )
                .child(muted(row.id.short().to_string()))
                .when(!row.moved_here.is_empty(), |d| {
                    d.child(crate::ui::e2e::measure_inside(format!(
                        "restore-preview-moved-{}-{}",
                        row.moved_here.join(","),
                        row.id.short()
                    )))
                })
                .when(row.off_branch, |d| {
                    d.child(crate::ui::e2e::measure_inside(format!(
                        "restore-preview-ghost-{}",
                        row.id.short()
                    )))
                })
                .child(crate::ui::e2e::measure_inside(format!(
                    "restore-preview-row-{n}"
                )))
        }));
    section
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(Msg::OplogPanel(OplogPanelMsg::RestoreAfter).t())
                .child(chip(
                    i18n::oplog_panel::preview_heading(removed),
                    theme().color_blocker,
                )),
        )
        .child(crate::ui::e2e::measure_inside(format!(
            "restore-preview-removed-{removed}"
        )))
        .when(above > 0, |d| {
            d.child(muted(i18n::oplog_panel::preview_more(above)))
        })
        .child(list)
        .when(below > 0, |d| {
            d.child(muted(i18n::oplog_panel::preview_more(below)))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_last_lane_stays_inside_restoration_rail() {
        let zoom = 2.;
        let lanes = 10;
        let focus = 9;
        let width = graph_window::gutter_width(lanes);
        let painted_width = width * zoom;
        let lane_w = graph_view::LANE_W * zoom;
        let scroll = preview_scroll(lanes, focus, width, zoom);
        let node_center = (focus as f32 + 0.5) * lane_w - scroll;
        assert!(
            node_center >= 0. && node_center <= painted_width,
            "zoomed focus node {node_center} outside clip 0..{painted_width} (scroll {scroll})"
        );
    }
}
