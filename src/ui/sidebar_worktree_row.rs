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
use kagi_ui_core::slow_read::SlowRead;

use super::sidebar::{name_tooltip, SIDEBAR_AUX_TEXT, SIDEBAR_WORKTREE_ROW_H};
use super::theme::{self, theme};
use super::{KagiApp, Msg};

pub(super) struct InspectionEntry {
    report: WorktreeInspection,
    read: crate::app::ReadKey,
    measured_at: SystemTime,
}

/// Cached observations and the cancellable request belong to one tab session.
///
/// `entries` is a per-path cache, never a claim about the tab as a whole: a
/// request retired mid-sweep (tab switch, new read, selection) leaves the
/// worktrees it never reached unmeasured, and a worktree created since the
/// last sweep was never a target at all. Coverage is therefore asked per
/// target, not by "the cache is non-empty" (#779).
#[derive(Default)]
pub(super) struct WorktreeInspections {
    entries: HashMap<PathBuf, InspectionEntry>,
    pub(super) selected: Option<PathBuf>,
    pending: HashSet<PathBuf>,
    revision: u64,
    read: Option<crate::app::ReadKey>,
    cancel: Option<Arc<AtomicBool>>,
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
        }
    }
}

impl Drop for WorktreeInspections {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl KagiApp {
    /// Measure every displayed linked worktree this tab has no observation for.
    /// A cached entry covers its own path only, so this resumes a sweep that a
    /// tab switch retired and picks up newly added linked worktrees. The main
    /// worktree has no navigator row and is not a removal candidate, so walking
    /// its disk would not inform this view. Entries already held are kept:
    /// remeasurement is the explicit refresh's job, while staleness is a
    /// display rule (`observed_verdict`), not a reason to re-walk the disk.
    pub(super) fn ensure_worktree_inspections(&mut self, cx: &mut Context<Self>) {
        let state = &self.ui().worktree_inspections;
        // A live request already owns its targets; retiring it here would throw
        // away the measurement in flight and, under a reload storm, restart the
        // sweep forever instead of finishing it.
        if !state.pending.is_empty() {
            return;
        }
        let unmeasured: Vec<PathBuf> = self
            .view()
            .worktrees
            .iter()
            .filter(|worktree| !worktree.is_main && !state.entries.contains_key(&worktree.path))
            .map(|worktree| worktree.path.clone())
            .collect();
        if unmeasured.is_empty() {
            return;
        }
        self.refresh_worktree_inspections(Some(&unmeasured), cx);
    }

    /// Selection retires the old request even when the selected value is cached.
    /// Re-selecting the row whose measurement is already in flight keeps that
    /// request: the row hover re-selects every time the pointer comes back
    /// from its card, and restarting would never let a slow walk finish.
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
        if !cached && !state.pending.contains(&path) {
            self.refresh_worktree_inspections(Some(std::slice::from_ref(&path)), cx);
        }
        cx.notify();
    }

    /// `only` names the worktrees to measure; `None` takes every worktree in
    /// the view. Cached entries outside the request are left alone — the sweep
    /// adds coverage, it never resets the tab's cache.
    pub(super) fn refresh_worktree_inspections(
        &mut self,
        only: Option<&[PathBuf]>,
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
            .filter(|worktree| {
                !worktree.is_main && only.is_none_or(|only| only.contains(&worktree.path))
            })
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        let read = self.reads.current_key(owner);
        let cancel = Arc::new(AtomicBool::new(false));
        // #355: explained once slow; its Skip is this request's `cancel`.
        let slow = self.begin_slow_read(owner, Some(SlowRead::WorktreeSize), cx);
        let Some(ui) = self.ui.get_mut(&owner) else {
            return;
        };
        let state = &mut ui.worktree_inspections;
        state.cancel();
        state.read = Some(read);
        state.cancel = Some(cancel.clone());
        state.pending = targets.iter().map(|target| target.path.clone()).collect();
        let revision = state.revision;
        #[cfg(feature = "gui-e2e")]
        let mut injected = super::e2e::worktree_inspection::take();
        #[cfg(not(feature = "gui-e2e"))]
        let mut injected: Option<gpui::Task<WorktreeInspection>> = None;
        cx.spawn(async move |this, acx| {
            // The read lasts exactly as long as this sweep.
            let _slow = slow;
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

/// The row's at-a-glance size and verdict. It carries no tooltip of its own:
/// the row's hover card is the one place the full verdict reads.
fn inspection_badge(app: &KagiApp, path: &Path, name: &str) -> gpui::AnyElement {
    let state = &app.ui().worktree_inspections;
    if state.pending.contains(path) {
        return div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(super::render_overlay::sync_spinner(
                12.,
                theme().text_muted,
                SharedString::from(format!("worktree-measuring-{name}")),
            ))
            .child(
                div()
                    .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
                    .child(Msg::WorktreeMeasuring.t()),
            )
            .into_any_element();
    }
    let Some(entry) = state.entries.get(path) else {
        return div()
            .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
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
        .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
        .text_color(rgb(theme().text_sub))
        .child(text)
        .child(div().text_color(rgb(color)).child(status.t()))
        .into_any_element()
}

/// Hover card width before UI scale: wide enough for the removal guide to wrap
/// into a few lines, narrow enough to sit beside a default-width sidebar.
const CARD_W: f32 = 320.;
/// A stable hover-card height before UI scale: a pending measurement must not
/// shrink the tooltip and flip it across the pointer, hiding Refresh mid-click.
const CARD_H: f32 = 300.;

/// A local WORKTREES row's inspection, shown while the row is hovered (#864).
///
/// GPUI's hoverable tooltip owns the card's lifetime: it shows after the native
/// delay, stays while the pointer is on the row or has moved onto the card (so
/// Refresh is reachable), and hides once the pointer has left both. The card
/// only reads the tab's cached observation; the row hover's
/// `select_worktree_inspection` is what starts a measurement.
struct WorktreeHoverCard {
    app: gpui::WeakEntity<KagiApp>,
    path: PathBuf,
    name: SharedString,
    port: Option<u16>,
    /// An observation landing is a `KagiApp` notify; the card repaints with it.
    _app_changed: Option<gpui::Subscription>,
}

impl WorktreeHoverCard {
    fn build(
        app: gpui::WeakEntity<KagiApp>,
        path: PathBuf,
        name: SharedString,
        port: Option<u16>,
        cx: &mut gpui::App,
    ) -> gpui::AnyView {
        cx.new(|cx: &mut Context<Self>| {
            let _app_changed = app
                .upgrade()
                .map(|entity| cx.observe(&entity, |_, _, cx| cx.notify()));
            Self {
                app,
                path,
                name,
                port,
                _app_changed,
            }
        })
        .into()
    }
}

impl gpui::Render for WorktreeHoverCard {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(entity) = self.app.upgrade() else {
            return div().into_any_element();
        };
        // GPUI lays a tooltip out against no available space and only flips it
        // at the window edge, so the card bounds itself to the viewport.
        let viewport = f32::from(window.viewport_size().height);
        let height = px((viewport - 16.)
            .min(f32::from(theme::scaled_px(CARD_H)))
            .max(0.));
        inspection_card(entity.read(cx), self, height)
    }
}

fn inspection_card(
    app: &KagiApp,
    card: &WorktreeHoverCard,
    height: gpui::Pixels,
) -> gpui::AnyElement {
    let path = card.path.as_path();
    // A tab switch or reload can leave the card alive for a frame while its
    // row is gone; it never shows another tab's (or an SSH tab's) worktree.
    // KagiApp's context menus are drawn in its root, but GPUI paints native
    // tooltips afterward. Keep the hover card out of the way while a worktree
    // menu is open so its items remain visible and clickable.
    if app.remote_view.is_some()
        || app.worktree_menu.is_some()
        || !app.view().worktrees.iter().any(|w| w.path == path)
    {
        return div().into_any_element();
    }
    let state = &app.ui().worktree_inspections;
    let path_label = kagi_domain::text_safety::sanitize_control_bytes(&path.display().to_string());
    let refresh_app = card.app.clone();
    let target = card.path.clone();
    let heading = div()
        .id("worktree-inspection-heading")
        .relative()
        .child(super::e2e::measure_inside("worktree-inspection-heading"))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(theme().text_main))
                .child(format!(
                    "{}: {}",
                    Msg::WorktreeInspectionTitle.t(),
                    card.name
                )),
        )
        .child(
            div()
                .id("worktree-inspection-path")
                .child(SharedString::from(path_label)),
        )
        .children(card.port.map(|port| {
            let url = format!("http://localhost:{port}");
            div()
                .id("worktree-inspection-port")
                .cursor_pointer()
                .text_color(rgb(theme().color_branch))
                .child(SharedString::from(format!("localhost:{port}")))
                .on_click(move |_, _, cx| cx.open_url(&url))
        }))
        .child(
            div().flex().child(
                div()
                    .id("worktree-inspection-refresh")
                    .cursor_pointer()
                    .relative()
                    .child(super::e2e::measure_inside("worktree-inspection-refresh"))
                    .text_color(rgb(theme().color_branch))
                    .child(Msg::WorktreeInspectionRefresh.t())
                    .on_click(move |_, _, cx| {
                        let _ = refresh_app.update(cx, |app, cx| {
                            app.refresh_worktree_inspections(
                                Some(std::slice::from_ref(&target)),
                                cx,
                            );
                        });
                    }),
            ),
        );
    let mut body = div()
        .id("worktree-inspection-body")
        .relative()
        .child(super::e2e::measure_inside("worktree-inspection-body"))
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .min_w(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1();
    if state.pending.contains(path) {
        body = body.child(super::e2e::measure_control(
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
        body = body.child(super::e2e::measure_control(
            "worktree-inspection-verdict",
            div().child(verdict_text(app, path, entry)),
        ));
        match &entry.report.disk_usage {
            Ok(usage) => {
                body = body
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
                body = body
                    .child(Msg::WorktreeObservationFailed.t())
                    .child(kagi_domain::text_safety::sanitize_control_bytes(error));
            }
        }
        let timestamp: chrono::DateTime<chrono::Local> = entry.measured_at.into();
        body = body.child(format!(
            "{}: {}",
            Msg::WorktreeMeasuredAt.t(),
            timestamp.format("%H:%M:%S")
        ));
    } else {
        body = body.child(Msg::WorktreeNotMeasured.t());
    }
    let body = body
        .child(Msg::WorktreeLocalRefEvidence.t())
        .child(Msg::WorktreeIgnoredWarning.t())
        .child(Msg::WorktreeRemovalGuide.t());
    div()
        .id("worktree-inspection")
        .relative()
        .child(super::e2e::measure_inside("worktree-inspection"))
        // The card overlaps the sidebar: a wheel, click, or hover on it must
        // not reach the panes and worktree rows beneath.
        .occlude()
        .w(theme::scaled_px(CARD_W))
        .h(height)
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().surface))
        .bg(rgb(theme().panel))
        .shadow_md()
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(heading)
        .child(body)
        .into_any_element()
}

/// What one WORKTREES row shows (the `SidebarRow::Worktree` fields).
pub(super) struct WorktreeRowFacts<'a> {
    pub(super) name: &'a str,
    /// The working-tree path itself, never `path_label`: the label is display
    /// text — lossy for non-UTF-8 paths and control-byte sanitized — so the
    /// menu's path actions would target the wrong directory if parsed back
    /// from it.
    pub(super) path: &'a std::path::Path,
    pub(super) path_label: &'a str,
    pub(super) is_current: bool,
    pub(super) locked: bool,
    /// First port of the worktree's stored block (#855).
    pub(super) port: Option<u16>,
}

/// A worktree leaf (✓ marks the current worktree). Right-click opens the shared
/// worktree menu.
pub(super) fn build_worktree_row(
    facts: WorktreeRowFacts<'_>,
    app: &KagiApp,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let WorktreeRowFacts {
        name,
        path,
        path_label,
        is_current,
        locked,
        port,
    } = facts;
    let is_remote = app.remote_view.is_some();
    // issue #356: worktree name/path are remote/filesystem-origin text —
    // neutralize control bytes in the visible label.
    let name_s = kagi_domain::text_safety::sanitize_control_bytes(name);
    let path_s = kagi_domain::text_safety::sanitize_control_bytes(path_label);
    let name_label = if is_current {
        SharedString::from(format!("\u{2713} {}", name_s))
    } else {
        SharedString::from(name_s.to_string())
    };
    // What a narrow row gives up — the path and the port link (#855) — a local
    // row's hover card carries; an SSH row has no card, so its plain tooltip
    // carries them instead.
    let remote_tooltip = is_remote.then(|| {
        SharedString::from(match port {
            Some(port) => format!("{}  {}  localhost:{}", name_label, path_s, port),
            None => format!("{}  {}", name_label, path_s),
        })
    });
    let text_color = if is_current {
        theme().color_success
    } else {
        theme().text_sub
    };
    let mut row = div()
        .id(SharedString::from(format!("sidebar-worktree-{}", name)))
        .h(theme::scaled_px(SIDEBAR_WORKTREE_ROW_H))
        // w_full: without it the row sizes to its content and runs past the
        // sidebar clip — the trailing lock chip was never visible and the
        // label never ellipsized (other sidebar rows are width-bounded).
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_3()
        .text_xs()
        .text_color(rgb(text_color))
        .overflow_hidden()
        // #855: the name is what identifies the row, so it never shrinks.
        // `measure_inside`, not `measure_control`: a wrapper div would put its
        // own flex sizing between this element and the row.
        .child(
            div()
                .relative()
                .flex_shrink_0()
                .whitespace_nowrap()
                .child(name_label)
                .child(super::e2e::measure_inside(format!(
                    "sidebar-worktree-name-{name}"
                ))),
        )
        // Path, then the port link (#855), in one row-high wrapping box. The
        // path takes only the room the link leaves (basis 0), so it truncates
        // first, down to nothing. When even the link alone does not fit, it
        // wraps onto a second line that the box clips: it disappears whole
        // instead of being cut to "localhost:30", and the path takes the room
        // back. The row's hover card still names it. min_w(0): without it the box
        // sizes to the (long) path's min-content width and pushes the lock chip
        // past the clipped row edge (user report).
        .child(
            div()
                .ml_2()
                .flex_1()
                .min_w(px(0.))
                .h(theme::scaled_px(SIDEBAR_WORKTREE_ROW_H))
                .overflow_hidden()
                .flex()
                .flex_row()
                .flex_wrap()
                .child(
                    div()
                        .h(theme::scaled_px(SIDEBAR_WORKTREE_ROW_H))
                        .flex_grow(1.)
                        .flex_basis(px(0.))
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
                                .child(SharedString::from(path_s.to_string())),
                        ),
                )
                .children(
                    port.filter(|_| !is_remote)
                        .map(|port| port_link(name, port, cx)),
                ),
        );
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
    if let Some(tooltip) = remote_tooltip {
        return row.tooltip(name_tooltip(tooltip)).into_any();
    }
    // Hovering the row is what selects its inspection, and the card is its
    // only presentation: moving to another row retires this row's request
    // (`select_worktree_inspection`), and leaving the row and its card hides
    // the card without touching the cache.
    let path_for_select = path.to_path_buf();
    let card_app = cx.weak_entity();
    let card_path = path.to_path_buf();
    let card_name = SharedString::from(name_s.to_string());
    row = row
        .gap_1()
        .child(inspection_badge(app, path, name))
        .on_hover(cx.listener(move |app, hovered: &bool, _, cx| {
            if *hovered {
                app.select_worktree_inspection(path_for_select.clone(), cx);
            }
        }))
        .hoverable_tooltip(move |_window, cx| {
            WorktreeHoverCard::build(
                card_app.clone(),
                card_path.clone(),
                card_name.clone(),
                port,
                cx,
            )
        });
    // Sidebar leaves are linked worktrees; the shared menu's main-worktree
    // path remains available from graph badges and WIP rows (#733).
    let name_for_menu = name.to_string();
    let path_for_menu = path.to_path_buf();
    let menu_handler = cx.listener(
        move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
            this.open_worktree_menu(
                name_for_menu.clone(),
                locked,
                false,
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

/// `localhost:<port>` for a worktree's stored port block (#855): the first port
/// of the block its terminal receives as `KAGI_PORT`. A click opens it in the
/// browser.
fn port_link(name: &str, port: u16, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let url = format!("http://localhost:{port}");
    let open = cx.listener(move |_app: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        cx.stop_propagation();
        cx.open_url(&url);
    });
    // Always whole: when it does not fit, the wrapping box around it moves it
    // to a clipped second line rather than cutting the text.
    div()
        .id(SharedString::from(format!("sidebar-worktree-port-{name}")))
        .relative()
        .flex_shrink_0()
        .h(theme::scaled_px(SIDEBAR_WORKTREE_ROW_H))
        .flex()
        .items_center()
        .whitespace_nowrap()
        .px_1()
        .rounded_sm()
        .text_size(theme::scaled_px(SIDEBAR_AUX_TEXT))
        .text_color(rgb(theme().color_branch))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(open)
        .child(SharedString::from(format!("localhost:{port}")))
        .child(super::e2e::measure_inside(format!(
            "sidebar-worktree-port-{name}"
        )))
        .into_any_element()
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
