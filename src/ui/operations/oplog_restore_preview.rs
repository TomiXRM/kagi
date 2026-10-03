//! The restore card's "graph after" (#334 slice 2c, ADR-0214 §6): built once
//! when the card opens from the tab's loaded commits and the plan's restores
//! (`kagi_domain::restore_preview`), drawn with the commit graph's own row
//! painter. Display only — nothing is read from or written to the repository.

use crate::ui::graph_view;
use crate::ui::modal_renderers::PlanCardExtra;
use crate::ui::modals::oplog_restore::RestoreGraphPreview;
use crate::ui::tab_view::TabViewState;
use crate::ui::theme::{self, theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled,
};
use kagi_domain::restore_preview::{self, FixedRoots, LoadedCommit, RestorePreview};
use kagi_git::OperationPlan;
use kagi_ui_core::i18n::{self, oplog_panel::OplogPanelMsg, Msg};
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
    // Tags can point to annotated tag objects, not graph commit IDs. Do not
    // compute a branch-only graph or count disappearing commits for tag moves.
    if restores.iter().any(|r| r.refname.starts_with("refs/tags/")) {
        return Some(Arc::new(RestoreGraphPreview {
            graph: RestorePreview::TagChange,
            summaries: Vec::new(),
        }));
    }
    // Branch Solo only filters what the graph shows; the full loaded rows are
    // kept aside while it is on (#883 review).
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
    // Branch-only restore: remote branches, tags, fetched PR heads (graph
    // roots no branch names, #883 review), detached worktree HEADs (an
    // attached one follows its branch), and stashes keep their commits.
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
    let graph = restore_preview::preview(&commits, &branches, &fixed, &restores);
    let summaries = match &graph {
        RestorePreview::Graph { rows: drawn, .. } => drawn
            .iter()
            .map(|row| {
                row_index
                    .get(&row.id)
                    .and_then(|i| rows.get(*i))
                    .map(|r| r.summary.clone())
                    .unwrap_or_default()
            })
            .collect(),
        RestorePreview::NotLoaded { .. } | RestorePreview::TagChange => Vec::new(),
    };
    Some(Arc::new(RestoreGraphPreview { graph, summaries }))
}

/// The section heading. A preview that could not be drawn claims nothing
/// about disappearing commits (#883 review).
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

/// The preview as text, for the card's `Copy all` (#883 review): heading,
/// then each drawn row (short id, branches moving there, branches staying,
/// summary), or the reason no graph is drawn.
pub(crate) fn clipboard_text(preview: &RestoreGraphPreview) -> String {
    let mut out = format!("{}\n", heading_text(&preview.graph));
    match &preview.graph {
        RestorePreview::NotLoaded { .. } => {
            out.push_str(&format!("  {}\n", i18n::oplog_panel::preview_not_loaded()));
        }
        RestorePreview::TagChange => {
            out.push_str(&format!(
                "  {}\n",
                Msg::OplogPanel(OplogPanelMsg::PreviewTagChange).t()
            ));
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
                let refs = if refs.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", refs.join(", "))
                };
                let summary = preview.summaries.get(n).map(|s| s.as_ref()).unwrap_or("");
                out.push_str(&format!("  {}{refs} {summary}\n", row.id.short()));
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

/// The card section after the warnings, with its `Copy all` text.
pub(crate) fn card_extra(preview: &RestoreGraphPreview) -> PlanCardExtra {
    PlanCardExtra {
        element: render(preview),
        clipboard: clipboard_text(preview),
    }
}

fn render(preview: &RestoreGraphPreview) -> gpui::AnyElement {
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
        .child(muted(heading_text(&preview.graph)))
        .child(crate::ui::e2e::measure_inside("restore-preview"));
    let (rows, lane_count, removed, above, below) = match &preview.graph {
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
                .into_any_element();
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
    let more = |n: usize| (n > 0).then(|| muted(i18n::oplog_panel::preview_more(n)));
    // Up to 40 rows of 29px do not fit a card on an ordinary window, and the
    // card body does not scroll: the rows scroll in their own capped box
    // (the modal list rule, #883 review).
    let list = div()
        .id("restore-preview-rows")
        .relative()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .max_h(crate::ui::modal_shell::modal_list_max_h(
            restore_preview::PREVIEW_MAX_ROWS,
        ))
        .overflow_y_scroll()
        .child(crate::ui::e2e::measure_inside("restore-preview-rows"))
        .children(rows.iter().enumerate().map(|(n, row)| {
            let summary = preview.summaries.get(n).cloned().unwrap_or_default();
            div()
                .relative()
                .flex_shrink_0()
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
        }));
    section
        .child(crate::ui::e2e::measure_inside(format!(
            "restore-preview-removed-{removed}"
        )))
        .children(more(above))
        .child(list)
        .children(more(below))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::graph::GraphRow;
    use kagi_domain::restore_preview::PreviewRow;
    use kagi_git::CommitId;

    fn row(id: &str, moved: &[&str], stay: &[&str]) -> PreviewRow {
        PreviewRow {
            id: CommitId(id.into()),
            graph: GraphRow {
                commit: CommitId(id.into()),
                lane: 0,
                color: 0,
                edges: Vec::new(),
            },
            branches: stay.iter().map(|s| s.to_string()).collect(),
            moved_here: moved.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn an_undrawn_preview_claims_no_disappearing_count() {
        let unavailable = RestorePreview::NotLoaded {
            refname: "refs/heads/main".into(),
            oid: "a".repeat(40),
        };
        assert_ne!(
            heading_text(&unavailable),
            i18n::oplog_panel::preview_heading(0),
            "\"no commit disappears\" would be a guess"
        );
        assert_eq!(
            heading_text(&unavailable),
            Msg::OplogPanel(OplogPanelMsg::PreviewUnavailableHeading).t()
        );
    }

    #[test]
    fn copy_all_carries_the_count_the_moves_and_the_hidden_rows() {
        let preview = RestoreGraphPreview {
            graph: RestorePreview::Graph {
                rows: vec![
                    row(&"1".repeat(40), &["main"], &[]),
                    row(&"2".repeat(40), &[], &["keep"]),
                ],
                lane_count: 1,
                removed: 3,
                hidden_above: 0,
                hidden_below: 5,
            },
            summaries: vec!["second".into(), "first".into()],
        };
        let text = clipboard_text(&preview);
        assert!(
            text.starts_with(&i18n::oplog_panel::preview_heading(3)),
            "{text}"
        );
        assert!(text.contains("  11111111 [main ←] second\n"), "{text}");
        assert!(text.contains("  22222222 [keep] first\n"), "{text}");
        assert!(text.contains(&i18n::oplog_panel::preview_more(5)), "{text}");
    }
}
