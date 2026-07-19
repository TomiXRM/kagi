//! Branch Cleanup pane + operations (ADR-0128).
//!
//! A center-takeover table of merged/stale branch candidates. The rows are
//! snapshot-derived (`active_view.cleanup_rows`, per-tab) so the table is
//! always fresh after a reload; this module owns the pane's open flag, the
//! render, the copy actions, and the plan → confirm → execute pipeline glue
//! (the ops live in `kagi_git::ops::branch_cleanup`).
//!
//! Delete affordances follow the domain classification: `FullyMerged` rows
//! join the bulk action, `SquashMergedLikely` rows are individually deletable,
//! `MergedThenGrown` (WARN) and stale-only rows render with **no** delete
//! button at all — the UI physically can't build a deletion out of them
//! (`BranchCleanupRow::delete_target` returns `None`).

use gpui::prelude::*;
use gpui::ClipboardItem;

use kagi_git::ops::{copy_all_text, CleanupDeleteTarget, MergedBranchStatus};

use super::modals::BranchCleanupModal;
use super::*;

// ────────────────────────────────────────────────────────────
// KagiApp glue: open/close, copy, plan, execute
// ────────────────────────────────────────────────────────────

impl KagiApp {
    /// Toggle the Branch Cleanup takeover from the sidebar entry.
    pub fn toggle_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        if self.branch_cleanup_open {
            self.close_branch_cleanup_view(cx);
        } else {
            self.open_branch_cleanup_view(cx);
        }
    }

    /// Open the Branch Cleanup table. No-op when no repository is open — the
    /// rows come from the snapshot, so there is nothing to compute here.
    pub fn open_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        if self.repo_path.is_none() {
            return;
        }
        self.branch_cleanup_open = true;
        klog!("branch-cleanup: opened");
        cx.notify();
    }

    /// Close the Branch Cleanup table.
    pub fn close_branch_cleanup_view(&mut self, cx: &mut Context<Self>) {
        self.branch_cleanup_open = false;
        cx.notify();
    }

    /// Copy every listed branch name (newline-joined) to the clipboard.
    pub fn copy_branch_cleanup_names(&mut self, cx: &mut Context<Self>) {
        let text = copy_all_text(&self.active_view.cleanup_rows);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.push_toast(ToastKind::Info, Msg::CleanupNamesCopied.t(), cx);
    }

    /// Build the delete plan for `targets` and open the confirmation modal.
    /// Used by both the per-row trash button (one target) and the header bulk
    /// button (every `bulk_deletable` row).
    pub fn open_branch_cleanup_plan(
        &mut self,
        targets: Vec<CleanupDeleteTarget>,
        cx: &mut Context<Self>,
    ) {
        if self.busy_op.is_some() || targets.is_empty() {
            return;
        }
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "branch-cleanup: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_delete_merged_branches(now_secs(), &targets) {
            Ok(plan) => {
                klog!(
                    "plan: branch-cleanup targets={} blockers={}",
                    targets.len(),
                    plan.blockers.len()
                );
                self.set_branch_cleanup_modal(BranchCleanupModal {
                    targets,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
                cx.notify();
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "branch-cleanup plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_branch_cleanup_modal(&mut self) {
        self.clear_branch_cleanup_modal();
    }

    /// Confirm the cleanup: execute in the background (remote deletion is a
    /// network write), then oplog + reload. Per-branch failures come back in
    /// the outcome and are surfaced without discarding the successes.
    pub fn confirm_branch_cleanup(&mut self, cx: &mut Context<Self>) {
        let modal = match self.branch_cleanup_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            self.record_op(
                "branch-cleanup",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        if self.busy_op.is_some() {
            return;
        }
        self.busy_op = Some("branch-cleanup");

        let bg_path = repo_path.clone();
        let plan = modal.plan.clone();
        let targets = modal.targets.clone();
        let task = cx.background_spawn(async move {
            kagi_git::Backend::open(&bg_path)
                .and_then(|b| b.execute_delete_merged_branches(&plan, &targets))
        });

        cx.spawn(async move |app, acx| {
            let result = task.await;
            let _ = app.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(outcome) => {
                        klog!(
                            "executed: branch-cleanup deleted={} failed={}",
                            outcome.deleted.len(),
                            outcome.failed.len()
                        );
                        app.clear_branch_cleanup_modal();
                        // The oplog line carries every deleted tip OID — the
                        // recovery contract (ADR-0128): restore with
                        // `git branch <name> <oid>` / `git push origin <oid>:refs/heads/<name>`.
                        let mut parts: Vec<String> = outcome
                            .deleted
                            .iter()
                            .map(|d| {
                                let mut s = d.name.clone();
                                if let Some(l) = &d.local_tip {
                                    s.push_str(&format!(" @{}", l.short()));
                                }
                                if let Some(r) = &d.remote_tip {
                                    s.push_str(&format!(" origin@{}", r.short()));
                                }
                                s
                            })
                            .collect();
                        for (name, reason) in &outcome.failed {
                            parts.push(format!("FAILED {}: {}", name, reason));
                        }
                        let after = kagi_git::ops::StateSummary {
                            head: modal.plan.current.head.clone(),
                            dirty: format!(
                                "deleted {} branch(es): {}",
                                outcome.deleted.len(),
                                parts.join("; ")
                            ),
                        };
                        let outcome_kind = if outcome.failed.is_empty() {
                            kagi_git::oplog::OpOutcome::Success { after }
                        } else if outcome.deleted.is_empty() {
                            kagi_git::oplog::OpOutcome::Failed {
                                error: after.dirty.clone(),
                            }
                        } else {
                            // Partial: record as success (the deletions are
                            // real and recoverable) with the failures in-line.
                            kagi_git::oplog::OpOutcome::Success { after }
                        };
                        app.record_op(
                            "branch-cleanup",
                            modal.plan.current.clone(),
                            outcome_kind,
                            &repo_path,
                            cx,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "branch-cleanup: {} deleted, {} failed",
                            outcome.deleted.len(),
                            outcome.failed.len()
                        )));
                        app.reload(cx);
                    }
                    Err(e) => {
                        // Global refusal (HEAD moved / repo open failure) —
                        // nothing was deleted.
                        let err_msg = format!("Cleanup failed: {}", e);
                        app.record_op(
                            "branch-cleanup",
                            modal.plan.current.clone(),
                            kagi_git::oplog::OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        if let Some(m) = self_modal_with_error(&modal, &err_msg) {
                            app.set_branch_cleanup_modal(m);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Rebuild the modal with an error line (keeps plan + targets for a retry).
fn self_modal_with_error(modal: &BranchCleanupModal, err: &str) -> Option<BranchCleanupModal> {
    Some(BranchCleanupModal {
        targets: modal.targets.clone(),
        plan: modal.plan.clone(),
        error: Some(SharedString::from(err.to_string())),
    })
}

/// Wall-clock now in Unix seconds (staleness input for collect/plan).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ────────────────────────────────────────────────────────────
// Resizable columns (ADR-0128)
// ────────────────────────────────────────────────────────────

/// Left/right table padding (logical px, rendered via `scaled_px`).
pub(super) const CLEANUP_PAD: f32 = 16.0;
/// Width of the inter-column divider strip (doubles as the cell gap).
pub(super) const CLEANUP_GAP: f32 = 4.0;
/// Fixed width of the trailing actions (trash) cell.
const CLEANUP_ACTIONS_W: f32 = 40.0;

/// `(settings key, default, min, max)` per resizable column, indexed by
/// [`DividerKind::CleanupCol`]'s payload (0 = name, 1 = where, 2 = merged-at,
/// 3 = status).
const CLEANUP_COL_SPECS: [(&str, f32, f32, f32); 4] = [
    ("cleanup_name_w", 260.0, 80.0, 600.0),
    ("cleanup_where_w", 110.0, 56.0, 240.0),
    ("cleanup_merged_w", 90.0, 60.0, 240.0),
    ("cleanup_status_w", 170.0, 80.0, 420.0),
];

/// Branch Cleanup column widths (logical px), persisted to `settings.json`
/// via `theme::set_col_width` like the commit-list columns (T030).
#[derive(Clone, Copy, Debug)]
pub struct CleanupCols(pub [f32; 4]);

impl Default for CleanupCols {
    fn default() -> Self {
        Self::load()
    }
}

impl CleanupCols {
    /// Read the persisted widths (clamped), falling back to the defaults.
    pub fn load() -> Self {
        let mut w = [0.0f32; 4];
        for (i, (key, default, min, max)) in CLEANUP_COL_SPECS.iter().enumerate() {
            w[i] = theme::read_col_width(key)
                .map(|v| v.clamp(*min, *max))
                .unwrap_or(*default);
        }
        Self(w)
    }

    /// The column's left edge relative to the table's left padding edge.
    fn left_of(&self, idx: usize) -> f32 {
        self.0[..idx].iter().map(|w| w + CLEANUP_GAP).sum()
    }
}

impl KagiApp {
    /// Drag-move handler for a [`DividerKind::CleanupCol`] divider.
    /// `cursor_rel_x` is the cursor in logical px relative to the pane's left
    /// edge (the caller subtracts the sidebar and divides out the zoom).
    pub(super) fn handle_cleanup_col_drag(
        &mut self,
        idx: u8,
        cursor_rel_x: f32,
        cx: &mut Context<Self>,
    ) {
        let idx = (idx as usize).min(3);
        let (key, _, min, max) = CLEANUP_COL_SPECS[idx];
        let left = CLEANUP_PAD + self.cleanup_cols.left_of(idx);
        let new_w = (cursor_rel_x - left - CLEANUP_GAP / 2.0).clamp(min, max);
        if (new_w - self.cleanup_cols.0[idx]).abs() > 0.5 {
            self.cleanup_cols.0[idx] = new_w;
            theme::set_col_width(key, new_w);
            cx.notify();
        }
    }
}

// ────────────────────────────────────────────────────────────
// Render
// ────────────────────────────────────────────────────────────

/// `1_768_003_200 → "2026-01-10"` — UTC civil date without a chrono dep
/// (Howard Hinnant's `civil_from_days`).
fn format_date(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// One status badge chip: label + accent color.
fn badge(label: &'static str, color: u32) -> gpui::AnyElement {
    div()
        .px(theme::scaled_px(6.))
        .py(theme::scaled_px(1.))
        .rounded(theme::scaled_px(4.))
        .text_xs()
        .text_color(rgb(color))
        .border_1()
        .border_color(rgb(color))
        .child(SharedString::from(label))
        .into_any_element()
}

/// Small clickable header/action button.
fn action_button(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    accent: u32,
    handler: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded(theme::scaled_px(4.))
        .text_xs()
        .text_color(rgb(accent))
        .border_1()
        .border_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(handler)
        .child(label)
        .into_any_element()
}

/// The Branch Cleanup takeover pane (ADR-0128).
pub fn render_branch_cleanup(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let rows = app.active_view.cleanup_rows.clone();
    let cols = app.cleanup_cols;
    let bulk_count = rows.iter().filter(|r| r.bulk_deletable).count();

    // ── Header: title + bulk delete + copy-all + close ──────────
    let bulk_button: Option<gpui::AnyElement> = (bulk_count > 0).then(|| {
        let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            let targets: Vec<CleanupDeleteTarget> = this
                .active_view
                .cleanup_rows
                .iter()
                .filter(|r| r.bulk_deletable)
                .filter_map(|r| r.delete_target())
                .collect();
            this.open_branch_cleanup_plan(targets, cx);
        });
        action_button(
            "cleanup-bulk-delete",
            SharedString::from(format!("{} ({})", Msg::CleanupDeleteMerged.t(), bulk_count)),
            theme().color_blocker,
            handler,
        )
    });
    let copy_all_button = {
        let handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.copy_branch_cleanup_names(cx);
        });
        action_button(
            "cleanup-copy-all",
            SharedString::from(Msg::CleanupCopyAll.t()),
            theme().color_branch,
            handler,
        )
    };
    let close_button = {
        let handler = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.close_branch_cleanup_view(cx);
        });
        div()
            .id("cleanup-close")
            .px_2()
            .py_1()
            .rounded(theme::scaled_px(4.))
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .cursor_pointer()
            .hover(|s| {
                s.bg(rgb(theme().surface))
                    .text_color(rgb(theme().text_main))
            })
            .on_click(handler)
            .child(SharedString::from("✕"))
            .into_any_element()
    };

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px(theme::scaled_px(CLEANUP_PAD))
        .py_3()
        .child(
            div()
                .text_xl()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::CleanupTitle.t())),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{}", rows.len()))),
        )
        .child(div().flex_1())
        .children(bulk_button)
        .child(copy_all_button)
        .child(close_button);

    // ── Column header (with drag dividers between the cells) ────
    // Cell widths come from `app.cleanup_cols` (persisted); each divider
    // strip doubles as the cell gap so the header and the rows line up on
    // exactly the same x offsets — which is also what the drag-move math in
    // `handle_cleanup_col_drag` assumes.
    let col_divider = |idx: u8| {
        div()
            .id(("cleanup-col-div", idx as usize))
            .w(theme::scaled_px(CLEANUP_GAP))
            .h_full()
            .flex_shrink_0()
            .cursor_col_resize()
            .hover(|s| s.bg(rgb(theme().color_branch)))
            .on_drag(
                DividerDrag {
                    kind: DividerKind::CleanupCol(idx),
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            )
            .into_any_element()
    };
    let col_label = |w: f32, msg: Msg| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .child(SharedString::from(msg.t()))
            .into_any_element()
    };
    let col_header = div()
        .flex()
        .flex_row()
        .items_center()
        .h(theme::scaled_px(24.))
        .px(theme::scaled_px(CLEANUP_PAD))
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(col_label(cols.0[0], Msg::CleanupColBranch))
        .child(col_divider(0))
        .child(col_label(cols.0[1], Msg::CleanupColWhere))
        .child(col_divider(1))
        .child(col_label(cols.0[2], Msg::CleanupColMergedAt))
        .child(col_divider(2))
        .child(col_label(cols.0[3], Msg::CleanupColStatus))
        .child(col_divider(3))
        .child(div().w(theme::scaled_px(CLEANUP_ACTIONS_W)).flex_shrink_0());

    // ── Rows ────────────────────────────────────────────────────
    let mut list = div()
        .id("branch-cleanup-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();

    if rows.is_empty() {
        list = list.child(
            div()
                .p_4()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(if app.repo_path.is_some() {
                    Msg::CleanupEmpty.t()
                } else {
                    Msg::CleanupNoRepo.t()
                })),
        );
    }

    // Plain (non-draggable) spacer matching the header divider width, so row
    // cells line up with the header cells.
    let gap = || {
        div()
            .w(theme::scaled_px(CLEANUP_GAP))
            .flex_shrink_0()
            .into_any_element()
    };

    for (i, row) in rows.iter().enumerate() {
        // Branch name: click = copy (no separate copy button — user request);
        // truncated with the full name in a tooltip.
        let full_name = SharedString::from(row.name.clone());
        let name_for_copy = row.name.clone();
        let name_cell = div()
            .id(("cleanup-name", i))
            .w(theme::scaled_px(cols.0[0]))
            .flex_shrink_0()
            .min_w(px(0.))
            .overflow_hidden()
            .text_sm()
            .text_color(rgb(theme().text_main))
            .cursor_pointer()
            .hover(|s| s.text_color(rgb(theme().color_branch)))
            .tooltip({
                let full = full_name.clone();
                move |window, cx| Tooltip::new(full.clone()).build(window, cx)
            })
            .on_click(
                cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                    super::branch_menu::copy_branch_name(this, name_for_copy.clone(), cx);
                }),
            )
            .child(div().truncate().child(full_name));

        // Where chips.
        let mut where_cell = div()
            .w(theme::scaled_px(cols.0[1]))
            .flex_shrink_0()
            .overflow_hidden()
            .flex()
            .flex_row()
            .gap_1();
        if row.local_tip.is_some() {
            where_cell = where_cell.child(badge("local", theme().text_muted));
        }
        if row.remote_tip.is_some() {
            where_cell = where_cell.child(badge("origin", theme().text_muted));
        }

        // Merged-at cell.
        let merged_cell = div()
            .w(theme::scaled_px(cols.0[2]))
            .flex_shrink_0()
            .overflow_hidden()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(
                row.merged_at.map(format_date).unwrap_or_else(|| "—".into()),
            ));

        // Status badges (clipped to the column — the WARN hint truncates and
        // must never paint over the actions cell).
        let mut status_cell = div()
            .w(theme::scaled_px(cols.0[3]))
            .flex_shrink_0()
            .overflow_hidden()
            .flex()
            .flex_row()
            .items_center()
            .gap_1();
        status_cell = match &row.status {
            MergedBranchStatus::FullyMerged => {
                status_cell.child(badge(Msg::CleanupBadgeMerged.t(), theme().color_success))
            }
            MergedBranchStatus::SquashMergedLikely => {
                status_cell.child(badge(Msg::CleanupBadgeSquash.t(), theme().color_branch))
            }
            MergedBranchStatus::MergedThenGrown { ahead } => status_cell
                .child(badge(Msg::CleanupBadgeGrown.t(), theme().color_blocker))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(rgb(theme().color_blocker))
                        .truncate()
                        .child(SharedString::from(format!(
                            "{} +{}",
                            Msg::CleanupGrownHint.t(),
                            ahead
                        ))),
                ),
            MergedBranchStatus::NotMerged => status_cell,
        };
        if row.stale {
            status_cell = status_cell.child(badge(Msg::CleanupBadgeStale.t(), theme().text_muted));
        }

        // Actions: trash only, and only when the row can build a target.
        let trash_btn: Option<gpui::AnyElement> = row.delete_target().map(|target| {
            let handler = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                this.open_branch_cleanup_plan(vec![target.clone()], cx);
            });
            div()
                .id(("cleanup-delete", i))
                .px_1()
                .rounded(theme::scaled_px(4.))
                .text_xs()
                .text_color(rgb(theme().color_blocker))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(handler)
                .child(SharedString::from("🗑"))
                .into_any_element()
        });
        let actions_cell = div()
            .w(theme::scaled_px(CLEANUP_ACTIONS_W))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .children(trash_btn);

        list = list.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .px(theme::scaled_px(CLEANUP_PAD))
                .py(theme::scaled_px(4.))
                .overflow_hidden()
                .hover(|s| s.bg(rgb(theme().surface)))
                .child(name_cell)
                .child(gap())
                .child(where_cell)
                .child(gap())
                .child(merged_cell)
                .child(gap())
                .child(status_cell)
                .child(gap())
                .child(actions_cell),
        );
    }

    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        .child(header)
        .child(col_header)
        .child(list)
        .into_any_element()
}
