//! The sidebar's WORKTREES leaf row.
//!
//! Split out of `sidebar.rs` (a known oversized god-file, see CLAUDE.md) when
//! #733 gave the row its context menu.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::SystemTime,
};

use gpui::{div, prelude::*, px, rgb, Context, SharedString};
use kagi_git::worktree_inspection::WorktreeInspection;

use super::sidebar::{name_tooltip, SIDEBAR_ROW_H};
use super::theme::{self, theme};
use super::{KagiApp, Msg};

pub(super) struct InspectionEntry {
    report: WorktreeInspection,
    read: crate::app::ReadKey,
    measured_at: SystemTime,
}

/// Cached observations and the cancellable request belong to one tab session.
#[derive(Default)]
pub(super) struct WorktreeInspections {
    entries: HashMap<PathBuf, InspectionEntry>,
    pub(super) selected: Option<PathBuf>,
    pending: HashSet<PathBuf>,
    revision: u64,
    read: Option<crate::app::ReadKey>,
    cancel: Option<Arc<AtomicBool>>,
    started: bool,
}

impl WorktreeInspections {
    pub(super) fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.revision = self.revision.wrapping_add(1);
        self.pending.clear();
    }

    pub(super) fn invalidate_read(&mut self, read: crate::app::ReadKey) {
        if self.read.is_some_and(|old| old != read) {
            self.cancel();
            self.started = !self.entries.is_empty();
        }
    }
}

impl Drop for WorktreeInspections {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl KagiApp {
    pub(super) fn ensure_worktree_inspections(&mut self, cx: &mut Context<Self>) {
        if !self.ui().worktree_inspections.started {
            self.refresh_worktree_inspections(None, cx);
        }
    }

    /// Selection retires the old request even when the selected value is cached.
    pub(super) fn select_worktree_inspection(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let read = self
            .active_session()
            .map(|owner| self.reads.current_key(owner));
        let Some(ui) = self.ui_mut() else { return };
        let state = &mut ui.worktree_inspections;
        if state.selected.as_ref() != Some(&path) {
            state.cancel();
        }
        state.selected = Some(path.clone());
        let cached = state
            .entries
            .get(&path)
            .is_some_and(|entry| Some(entry.read) == read);
        if !cached {
            self.refresh_worktree_inspections(Some(path), cx);
        }
        cx.notify();
    }

    pub(super) fn refresh_worktree_inspections(
        &mut self,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.remote_view.is_some() {
            return;
        }
        let (Some(owner), Some(repo)) = (self.active_session(), self.repo_path.clone()) else {
            return;
        };
        let targets: Vec<_> = self
            .view()
            .worktrees
            .iter()
            .filter(|worktree| path.as_ref().is_none_or(|path| path == &worktree.path))
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        let read = self.reads.current_key(owner);
        let cancel = Arc::new(AtomicBool::new(false));
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        let state = &mut ui.worktree_inspections;
        state.cancel();
        state.started = true;
        state.read = Some(read);
        state.cancel = Some(cancel.clone());
        state.pending = targets.iter().map(|target| target.path.clone()).collect();
        let revision = state.revision;
        #[cfg(feature = "gui-e2e")]
        let mut injected = super::e2e::worktree_inspection::take();
        #[cfg(not(feature = "gui-e2e"))]
        let mut injected: Option<gpui::Task<WorktreeInspection>> = None;
        cx.spawn(async move |this, acx| {
            for target in targets {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let worker_repo = repo.clone();
                let worker_cancel = cancel.clone();
                let target_path = target.path.clone();
                let report = match injected.take() {
                    Some(task) => task.await,
                    None => {
                        acx.background_executor()
                            .spawn(async move {
                                kagi_git::worktree_inspection::inspect_worktree(
                                    &worker_repo,
                                    &target,
                                    &worker_cancel,
                                )
                            })
                            .await
                    }
                };
                let accepted = this
                    .update(acx, |app, cx| {
                        let current =
                            app.active_session() == Some(owner) && app.reads.is_fresh(read);
                        let Some(ui) = app.ui.get_mut(&owner) else {
                            return false;
                        };
                        let state = &mut ui.worktree_inspections;
                        if state.revision != revision || cancel.load(Ordering::Relaxed) {
                            return false;
                        }
                        if !current {
                            state.cancel();
                            cx.notify();
                            return false;
                        }
                        state.pending.remove(&target_path);
                        state.entries.insert(
                            target_path,
                            InspectionEntry {
                                report,
                                read,
                                measured_at: SystemTime::now(),
                            },
                        );
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !accepted {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn observed_verdict(
    app: &KagiApp,
    path: &Path,
    entry: &InspectionEntry,
) -> Option<kagi_domain::remove::WorktreeRemovalVerdict> {
    if !app.reads.is_fresh(entry.read) || app.op_latched() {
        return None;
    }
    let mut facts = entry.report.removal;
    if app
        .view()
        .worktrees
        .iter()
        .any(|worktree| worktree.path == path && worktree.wip.is_some_and(|wip| wip.is_dirty()))
    {
        facts.clean = kagi_domain::remove::WorktreeEvidence::No;
    }
    Some(kagi_domain::remove::worktree_removal_verdict(&facts))
}

fn verdict_text(app: &KagiApp, path: &Path, entry: &InspectionEntry) -> &'static str {
    observed_verdict(app, path, entry)
        .map(super::i18n::worktree_removal_verdict_text)
        .unwrap_or_else(|| Msg::WorktreeInspectionStale.t())
}

fn inspection_badge(app: &KagiApp, path: &Path, name: &str) -> gpui::AnyElement {
    let state = &app.ui().worktree_inspections;
    if state.pending.contains(path) {
        return div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .id(SharedString::from(format!(
                "worktree-inspection-badge-{name}"
            )))
            .tooltip(name_tooltip(Msg::WorktreeMeasuring.t().into()))
            .child(super::render_overlay::sync_spinner(
                12.,
                theme().text_muted,
                SharedString::from(format!("worktree-measuring-{name}")),
            ))
            .child(div().text_xs().child(Msg::WorktreeMeasuring.t()))
            .into_any_element();
    }
    let Some(entry) = state.entries.get(path) else {
        return div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(Msg::WorktreeNotMeasured.t())
            .into_any_element();
    };
    let text = entry
        .report
        .disk_usage
        .as_ref()
        .map(|usage| size_text(usage.allocated_bytes))
        .unwrap_or_else(|_| Msg::WorktreeObservationFailed.t().to_string());
    use kagi_domain::remove::WorktreeRemovalVerdict as V;
    let (status, color) = match observed_verdict(app, path, entry) {
        Some(V::SafeMerged | V::SafePushed) => (Msg::WorktreeSafeShort, theme().color_success),
        Some(V::Unknown(_)) | None => (Msg::WorktreeUnknownShort, theme().text_muted),
        _ => (Msg::WorktreeKeepShort, theme().color_blocker),
    };
    div()
        .flex()
        .gap_1()
        .flex_shrink_0()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .id(SharedString::from(format!(
            "worktree-inspection-badge-{name}"
        )))
        .tooltip(name_tooltip(verdict_text(app, path, entry).into()))
        .child(text)
        .child(div().text_color(rgb(color)).child(status.t()))
        .into_any_element()
}

pub(super) fn inspection_panel(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let state = &app.ui().worktree_inspections;
    let Some(path) = state
        .selected
        .as_ref()
        .filter(|_| app.remote_view.is_none())
    else {
        return div().into_any_element();
    };
    let target = path.clone();
    let name = app
        .view()
        .worktrees
        .iter()
        .find(|worktree| &worktree.path == path)
        .map(|worktree| worktree.name.as_str());
    let name = name.map(std::borrow::Cow::Borrowed).unwrap_or_else(|| {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
    });
    let name = kagi_domain::text_safety::sanitize_control_bytes(&name);
    let path_label = kagi_domain::text_safety::sanitize_control_bytes(&path.display().to_string());
    let mut panel = div()
        .id("worktree-inspection-body")
        .relative()
        .child(super::e2e::measure_inside("worktree-inspection-body"))
        .flex_1()
        .min_h(px(0.))
        .gap_1()
        .w_full()
        .min_w(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .id("worktree-inspection-refresh")
                        .cursor_pointer()
                        .relative()
                        .child(super::e2e::measure_inside("worktree-inspection-refresh"))
                        .text_color(rgb(theme().color_branch))
                        .child(Msg::WorktreeInspectionRefresh.t())
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.refresh_worktree_inspections(Some(target.clone()), cx);
                        })),
                )
                .child(
                    div()
                        .id("worktree-inspection-close")
                        .cursor_pointer()
                        .child(Msg::AppNoticeDismiss.t())
                        .on_click(cx.listener(|app, _, _, cx| {
                            if let Some(ui) = app.ui_mut() {
                                ui.worktree_inspections.cancel();
                                ui.worktree_inspections.selected = None;
                            }
                            cx.notify();
                        })),
                ),
        );
    if state.pending.contains(path) {
        panel = panel.child(super::e2e::measure_control(
            "worktree-inspection-measuring",
            div()
                .flex()
                .gap_1()
                .child(super::render_overlay::sync_spinner(
                    12.,
                    theme().text_muted,
                    "worktree-inspection-spin",
                ))
                .child(Msg::WorktreeMeasuring.t()),
        ));
    } else if let Some(entry) = state.entries.get(path) {
        panel = panel.child(super::e2e::measure_control(
            "worktree-inspection-verdict",
            div().child(verdict_text(app, path, entry)),
        ));
        match &entry.report.disk_usage {
            Ok(usage) => {
                panel = panel
                    .child(format!(
                        "{}: {}",
                        Msg::WorktreeAllocated.t(),
                        size_text(usage.allocated_bytes)
                    ))
                    .child(format!(
                        "target/: {} · {}: {}",
                        size_text(usage.target_bytes),
                        Msg::WorktreeOtherBytes.t(),
                        size_text(usage.allocated_bytes.saturating_sub(usage.target_bytes))
                    ));
            }
            Err(error) => {
                panel = panel
                    .child(Msg::WorktreeObservationFailed.t())
                    .child(kagi_domain::text_safety::sanitize_control_bytes(error));
            }
        }
        let timestamp: chrono::DateTime<chrono::Local> = entry.measured_at.into();
        panel = panel.child(format!(
            "{}: {}",
            Msg::WorktreeMeasuredAt.t(),
            timestamp.format("%H:%M:%S")
        ));
    } else {
        panel = panel.child(Msg::WorktreeNotMeasured.t());
    }
    let body = panel
        .child(Msg::WorktreeLocalRefEvidence.t())
        .child(Msg::WorktreeIgnoredWarning.t())
        .child(Msg::WorktreeRemovalGuide.t());
    div()
        .id("worktree-inspection")
        .relative()
        .child(super::e2e::measure_inside("worktree-inspection"))
        .flex_shrink_0()
        .max_h(gpui::relative(0.4))
        .min_h(px(0.))
        .w_full()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .overflow_hidden()
        .border_t_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .id("worktree-inspection-heading")
                .relative()
                .child(super::e2e::measure_inside("worktree-inspection-heading"))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .truncate()
                        .child(format!("{}: {name}", Msg::WorktreeInspectionTitle.t())),
                )
                .child(
                    div()
                        .id("worktree-inspection-path")
                        .truncate()
                        .tooltip(name_tooltip(path_label.clone().into()))
                        .child(path_label),
                ),
        )
        .child(body)
        .into_any_element()
}

/// A worktree leaf (✓ marks the current worktree). Right-click opens the shared
/// worktree menu.
///
/// `path` is the working-tree path itself, never `path_label`: the label is
/// display text — lossy for non-UTF-8 paths and control-byte sanitized — so the
/// menu's path actions would target the wrong directory if parsed back from it.
pub(super) fn build_worktree_row(
    name: &str,
    path: &std::path::Path,
    path_label: &str,
    is_current: bool,
    is_main: bool,
    locked: bool,
    app: &KagiApp,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let is_remote = app.remote_view.is_some();
    // issue #356: worktree name/path are remote/filesystem-origin text —
    // neutralize control bytes in the visible label.
    let name_s = kagi_domain::text_safety::sanitize_control_bytes(name);
    let path_s = kagi_domain::text_safety::sanitize_control_bytes(path_label);
    let label = if is_current {
        SharedString::from(format!("\u{2713} {}  {}", name_s, path_s))
    } else {
        SharedString::from(format!("{}  {}", name_s, path_s))
    };
    let full_name = label.clone();
    let text_color = if is_current {
        theme().color_success
    } else {
        theme().text_sub
    };
    let mut row = div()
        .id(SharedString::from(format!("sidebar-worktree-{}", name)))
        .h(theme::scaled_px(SIDEBAR_ROW_H))
        // w_full: without it the row sizes to its content and runs past the
        // sidebar clip — the trailing lock chip was never visible and the
        // label never ellipsized (other sidebar rows are width-bounded).
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_sm()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .tooltip(name_tooltip(full_name))
        // min_w(0): without it the flex item sizes to the (long) path label's
        // min-content width and pushes the lock chip past the clipped row edge
        // — the indicator never showed at all (user report).
        .child(div().flex_1().min_w(px(0.)).truncate().child(label));
    if locked {
        // 🔐 reads at a glance where the muted "locked" text was easy to miss
        // next to the path label (user feedback).
        row = row.child(div().flex_shrink_0().text_xs().child("🔐"));
    }
    // An SSH tab's worktree is fabricated by `remote::` with the path as it
    // exists **on the remote host** (ADR-0089), while every menu action —
    // open, reveal, copy, prune, repair — runs locally. A local directory that
    // happens to share that absolute path would be opened instead of erroring,
    // so a remote view gets no worktree menu at all.
    if is_remote {
        return row.into_any();
    }
    let path_for_select = path.to_path_buf();
    row = row
        .gap_1()
        .child(inspection_badge(app, path, name))
        .on_click(cx.listener(move |app, _, _, cx| {
            app.select_worktree_inspection(path_for_select.clone(), cx);
        }));
    // The lifecycle items (remove / lock / unlock) are linked worktrees only —
    // the main worktree is never lockable or removable from kagi — but the path
    // items are not, so the main row gets the menu too: from a linked
    // worktree's tab, "Open in new tab" on the main row is the way back (#733).
    // `build_worktree_menu` drops the lifecycle groups when `is_main`.
    let name_for_menu = name.to_string();
    let path_for_menu = path.to_path_buf();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_worktree_menu(
                name_for_menu.clone(),
                locked,
                is_main,
                Some(path_for_menu.clone()),
                event.position,
            );
            cx.stop_propagation();
            cx.notify();
        },
    );
    row.on_mouse_down(gpui::MouseButton::Right, menu_handler)
        .relative()
        .child(super::e2e::measure_inside(format!(
            "sidebar-worktree-{name}"
        )))
        .hover(|style| style.bg(rgb(theme().surface)))
        .into_any()
}

#[cfg(feature = "gui-e2e")]
pub(super) fn inspection_status(app: &KagiApp, path: &Path) -> (bool, Option<u64>, Option<String>) {
    let state = &app.ui().worktree_inspections;
    let entry = state.entries.get(path);
    (
        state.pending.contains(path),
        entry.and_then(|entry| {
            entry
                .report
                .disk_usage
                .as_ref()
                .ok()
                .map(|usage| usage.allocated_bytes)
        }),
        entry.map(|entry| verdict_text(app, path, entry).to_string()),
    )
}
