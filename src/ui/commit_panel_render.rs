//! Commit Panel rendering, split out of `render_helpers.rs` (T-SPLIT-HELPERS-001
//! / ADR-0116 Wave 3). These build the Commit Panel view tree.
//!
//! ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the Commit Panel is now an
//! `Entity<CommitPanelView>` (correction #6). The per-row builders are pure
//! `&CommitPanelView` reads; the 22 listeners are `|view: &mut CommitPanelView|`.
//! Every listener that touches `app.commit_panel` (stage/unstage, file select,
//! commit/amend, discard, smart-commit, the parent `file_menu` overlay) DEFERS to
//! the parent via `cx.spawn_in(window, …)` + `weak_app.update_in(acx, …)` so the
//! leased entity is never re-entered. Pure entity-internal mutations (tree↔flat,
//! the co-author picker) stay synchronous + a child `cx.notify()`.
//! Element tree / styles / [kagi] lines / i18n are byte-identical to the
//! pre-entity version.

#![allow(clippy::too_many_arguments)]

use super::commit_panel::CommitPanelView;
use super::render_helpers::*;
use super::*;
use crate::ui::button_style::KagiButton;
use gpui_component::button::{Button, ButtonVariants};

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from the
// `CommitPanelView` entity (NOT a captured-by-value clone). They are invoked
// from the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// PERF: build one unstaged row in flat view (index `fi` into `unstaged`).
pub(crate) fn render_unstaged_flat_row(
    view: &CommitPanelView,
    fi: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let f = panel.unstaged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = view.active_wip.clone();

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let is_conflicted_file = panel.is_conflicted(&f.path);
    let (badge, badge_color, _) = status_badge(Some(&f.change), is_conflicted_file);
    let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
    let stat = panel.unstaged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| !*st && &f.path == p);

    let file_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        view.defer_select_file(CommitPanelFileRef::Unstaged { index: fi }, window, cx);
    });
    let stage_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        view.defer_stage_file(fi, window, cx);
    });
    // Row background: conflicted files get red tint
    let row_bg = if is_conflicted_file {
        theme().diff_removed_bg
    } else if is_sel {
        theme().selected
    } else {
        theme().panel
    };
    let mut file_row = div()
        .id(("cp-us-flat-file", fi))
        .when(wip_hit, |el| el.bg(rgb(theme().selected)))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_px()
        .bg(rgb(row_bg))
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(file_click)
        .child(
            div()
                .w(theme::scaled_px(12.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_main))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(name)),
        )
        .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
    // Stage button only for non-conflicted files
    if !is_conflicted_file {
        // W17-DISCARD / ADR-0083: right-click opens the file context menu
        // (Discard lives there). Tracked rows are restored from the index;
        // untracked rows are deleted (after an ODB backup).
        let menu_click = cx.listener(move |view, e: &gpui::MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.defer_open_file_menu(fi, e.position, window, cx);
        });
        file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
        file_row = file_row.child(
            KagiButton::accent(
                ("cp-us-flat-stage-btn", fi),
                "Stage",
                theme().color_success,
                cx,
            )
            .xsmall()
            .ml_2()
            .flex_shrink_0()
            .on_click(stage_click),
        );
    } else {
        file_row = file_row.child(
            div()
                .id(("cp-us-flat-conflict-badge", fi))
                .ml_2()
                .px_1()
                .py_px()
                .rounded_sm()
                .flex_shrink_0()
                .bg(rgb(theme().color_blocker)) // red
                .text_xs()
                .text_color(rgb(theme().bg_base))
                .child(SharedString::from("Conflict")),
        );
    }
    Some(file_row.into_any_element())
}

/// PERF: build one unstaged tree row (index `row_index` into `unstaged_tree`).
pub(crate) fn render_unstaged_tree_row(
    view: &CommitPanelView,
    row_index: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let row = panel.unstaged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = view.active_wip.clone();

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-us-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            // Look up the original path to check if conflicted
            let path = panel.unstaged.get(fi).map(|f| f.path.clone());
            let is_conflicted_file = path
                .as_ref()
                .map(|p| panel.is_conflicted(p))
                .unwrap_or(false);
            let (badge, badge_color, _) = status_badge(change.as_ref(), is_conflicted_file);
            let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
            let stat = path.as_ref().and_then(|p| panel.unstaged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| !*st && fp == p);

            let file_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
                view.defer_select_file(CommitPanelFileRef::Unstaged { index: fi }, window, cx);
            });
            let stage_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
                view.defer_stage_file(fi, window, cx);
            });
            let row_bg = if is_conflicted_file {
                theme().diff_removed_bg
            } else if is_sel {
                theme().selected
            } else {
                theme().panel
            };
            let mut file_row = div()
                .id(("cp-us-file", fi))
                .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .pl(theme::scaled_px(8.0 + indent))
                .pr(theme::scaled_px(2.0))
                .py_px()
                .bg(rgb(row_bg))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(file_click)
                .child(
                    div()
                        .w(theme::scaled_px(12.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(badge_color))
                        .child(SharedString::from(badge)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .truncate()
                        .child(name.clone()),
                )
                .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
            if !is_conflicted_file {
                // W17-DISCARD / ADR-0083: right-click opens the file context menu
                // (Discard lives there). Untracked rows are discardable too —
                // deleted from disk after an ODB backup.
                let menu_click = cx.listener(move |view, e: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    view.defer_open_file_menu(fi, e.position, window, cx);
                });
                file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
                file_row = file_row.child(
                    KagiButton::accent(("cp-us-stage-btn", fi), "Stage", theme().color_success, cx)
                        .xsmall()
                        .ml_2()
                        .flex_shrink_0()
                        .on_click(stage_click),
                );
            } else {
                file_row = file_row.child(
                    div()
                        .id(("cp-us-conflict-badge", fi))
                        .ml_2()
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .flex_shrink_0()
                        .bg(rgb(theme().color_blocker))
                        .text_xs()
                        .text_color(rgb(theme().bg_base))
                        .child(SharedString::from("Conflict")),
                );
            }
            Some(file_row.into_any_element())
        }
    }
}

/// PERF: build one staged row in flat view (index `fi` into `staged`).
pub(crate) fn render_staged_flat_row(
    view: &CommitPanelView,
    fi: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let f = panel.staged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = view.active_wip.clone();

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let (badge, badge_color, _conflicted) = status_badge(Some(&f.change), false);
    let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
    let stat = panel.staged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| *st && &f.path == p);

    let file_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        view.defer_select_file(CommitPanelFileRef::Staged { index: fi }, window, cx);
    });
    let unstage_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        view.defer_unstage_file(fi, window, cx);
    });
    Some(
        div()
            .id(("cp-st-flat-file", fi))
            .when(wip_hit, |el| el.bg(rgb(theme().selected)))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_2()
            .py_px()
            .bg(rgb(if is_sel {
                theme().selected
            } else {
                theme().panel
            }))
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(file_click)
            .child(
                div()
                    .w(theme::scaled_px(12.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(badge_color))
                    .child(SharedString::from(badge)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .text_color(rgb(theme().text_main))
                    .overflow_hidden()
                    .truncate()
                    .child(SharedString::from(name)),
            )
            .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
            .child(
                KagiButton::accent(
                    ("cp-st-flat-unstage-btn", fi),
                    "Unstage",
                    theme().color_warning,
                    cx,
                )
                .xsmall()
                .ml_2()
                .flex_shrink_0()
                .on_click(unstage_click),
            )
            .into_any_element(),
    )
}

/// PERF: build one staged tree row (index `row_index` into `staged_tree`).
pub(crate) fn render_staged_tree_row(
    view: &CommitPanelView,
    row_index: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let row = panel.staged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = view.active_wip.clone();

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-st-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            let (badge, badge_color, _conflicted) = status_badge(change.as_ref(), false);
            let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
            let path = panel.staged.get(fi).map(|f| f.path.clone());
            let stat = path.as_ref().and_then(|p| panel.staged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| *st && fp == p);

            let file_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
                view.defer_select_file(CommitPanelFileRef::Staged { index: fi }, window, cx);
            });
            let unstage_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
                view.defer_unstage_file(fi, window, cx);
            });
            Some(
                div()
                    .id(("cp-st-file", fi))
                    .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(theme::scaled_px(8.0 + indent))
                    .pr(theme::scaled_px(2.0))
                    .py_px()
                    .bg(rgb(if is_sel {
                        theme().selected
                    } else {
                        theme().panel
                    }))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(file_click)
                    .child(
                        div()
                            .w(theme::scaled_px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .overflow_hidden()
                            .truncate()
                            .child(name.clone()),
                    )
                    .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
                    .child(
                        KagiButton::accent(
                            ("cp-st-unstage-btn", fi),
                            "Unstage",
                            theme().color_warning,
                            cx,
                        )
                        .xsmall()
                        .ml_2()
                        .flex_shrink_0()
                        .on_click(unstage_click),
                    )
                    .into_any_element(),
            )
        }
    }
}

/// The co-author popover, or `None` when the picker is closed.
///
/// Anchored above the icon row (the footer sits at the bottom of the panel, so
/// a downward menu would open off-screen).
fn render_coauthor_menu(
    candidates: Option<&[kagi_git::AuthorCandidate]>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let candidates = candidates?;
    let mut menu = div()
        .absolute()
        .bottom(theme::scaled_px(26.0))
        .left_0()
        .w(theme::scaled_px(260.0))
        .id("cp-coauthor-menu")
        .max_h(theme::scaled_px(220.0))
        .overflow_y_scroll()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().panel))
        .flex()
        .flex_col()
        .py_1();

    if candidates.is_empty() {
        return Some(
            menu.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::NoRecentAuthors.t())),
            )
            .into_any_element(),
        );
    }

    for (i, c) in candidates.iter().enumerate() {
        let candidate = c.clone();
        menu = menu.child(
            div()
                .id(("cp-coauthor-row", i))
                .px_2()
                .py_1()
                .flex()
                .flex_col()
                .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                .on_click(cx.listener(
                    move |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.add_coauthor(&candidate, window, cx);
                        view.coauthor_menu = None;
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(c.name.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(c.email.clone())),
                ),
        );
    }
    Some(menu.into_any_element())
}

// ──────────────────────────────────────────────────────────────
// CommitPanelView — deferred Backend dispatch (re-entrancy invariant)
// ──────────────────────────────────────────────────────────────
//
// Every method here marshals to the parent `KagiApp` via `spawn_in`/`update_in`:
// the called `KagiApp` method reads/updates `app.commit_panel` (this very
// entity), so calling it synchronously from a leased listener would re-lease the
// entity and panic ("already borrowed"). By the time the spawned task runs the
// listener has returned and the lease is released. Mirrors `ConflictView`.

impl CommitPanelView {
    fn defer_stage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.do_stage_file(fi, cx));
        })
        .detach();
    }

    fn defer_unstage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.do_unstage_file(fi, cx));
        })
        .detach();
    }

    fn defer_stage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.do_stage_all(cx));
        })
        .detach();
    }

    fn defer_unstage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.do_unstage_all(cx));
        })
        .detach();
    }

    fn defer_select_file(
        &self,
        file_ref: CommitPanelFileRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| {
                app.select_commit_panel_file(file_ref, cx)
            });
        })
        .detach();
    }

    fn defer_open_commit_plan_modal(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.open_commit_plan_modal(cx));
        })
        .detach();
    }

    fn defer_open_discard_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.open_discard_all_modal(cx));
        })
        .detach();
    }

    fn defer_open_file_menu(
        &self,
        fi: usize,
        pos: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `file_menu` is the shared parent overlay (correction #6b: kept on the
        // parent — its dismiss/discard/history actions read `app.commit_panel`).
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| {
                app.file_menu = Some((fi, pos));
                cx.notify();
            });
        })
        .detach();
    }

    fn defer_amend(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| app.commit_panel_amend(cx));
        })
        .detach();
    }

    fn defer_smart_suggest(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, window, cx| app.smart_suggest(window, cx));
        })
        .detach();
    }

    fn defer_smart_generate(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, window, cx| app.smart_generate(window, cx));
        })
        .detach();
    }
}

// ──────────────────────────────────────────────────────────────
// Commit Panel renderer (T025) — now self-rendering on the entity
// ──────────────────────────────────────────────────────────────

impl Render for CommitPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Correction #1/#2: input sync + draft autosave run on the entity's own
        // render path (with `&mut Window`), never as a parent per-frame read of
        // the child's input.
        self.sync_inputs(window, cx);
        let panel_width = self.panel_render_width;
        self.render_panel(panel_width, cx)
    }
}

impl CommitPanelView {
    /// Render the Commit Panel: unstaged/staged sections + diff viewer + message
    /// input + commit button. (Was `KagiApp::render_commit_panel`; retargeted to
    /// the entity — reads `self.state` + the entity's own inputs/scroll handles.
    /// `smart` is read off the parent `KagiApp` via the weak handle: it is safe
    /// because render runs after the parent's render returns, and the value is
    /// pushed in by the parent each frame via `set_smart_snapshot`.)
    pub(crate) fn render_panel(
        &self,
        panel_width: f32,
        cx: &mut Context<CommitPanelView>,
    ) -> impl IntoElement {
        let panel = &self.state;
        let preview = panel.preview.clone();
        let title_input = self.title_input.clone();
        let body_input = self.body_input.clone();
        let coauthor_menu = self.coauthor_menu.clone();
        let smart = self.smart_snapshot.clone();
        let unstaged_scroll_handle = self.unstaged_scroll_handle.clone();
        let staged_scroll_handle = self.staged_scroll_handle.clone();

        let tree_view = panel.tree_view;
        let unstaged_count = panel.unstaged.len();
        let staged_count = panel.staged.len();
        // W17-DISCARD: count discard-eligible unstaged files (exclude untracked,
        // which the panel surfaces as `Added` rows, and conflicted files).
        // ADR-0083: untracked (`Added`) rows ARE discardable (deleted with backup),
        // so they count toward enabling "Discard all" — only conflicted rows are
        // excluded. Must mirror `discard_partition`.
        let discard_eligible_count = panel
            .unstaged
            .iter()
            .filter(|f| !panel.is_conflicted(&f.path))
            .count();
        // T026: the subject line alone decides whether there is a message —
        // a body without a subject is not a commit.
        let input_msg_nonempty = title_input
            .as_ref()
            .map(|e| !e.read(cx).value().trim().is_empty())
            .unwrap_or(!panel.commit_msg.trim().is_empty());
        let can_commit = !panel.staged.is_empty() && input_msg_nonempty;
        let has_unstaged_warning = !panel.unstaged.is_empty() && staged_count > 0;
        // PERF: selected_file is read per visible row from the entity inside the
        // uniform_list processors, not captured here.

        // ── View switch: segmented [List | Tree] (T-UI-002) ──────
        let list_click = cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                view.state.tree_view = false;
                cx.notify();
            },
        );
        let tree_click = cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                view.state.tree_view = true;
                cx.notify();
            },
        );
        let seg = |id: &'static str, label: &'static str, active: bool| {
            div()
                .id(id)
                .px_1p5()
                .py_px()
                .text_xs()
                .bg(rgb(if active {
                    theme().selected
                } else {
                    theme().surface
                }))
                .text_color(rgb(if active {
                    theme().text_main
                } else {
                    theme().text_muted
                }))
                .hover(|st| st.text_color(rgb(theme().text_main)).cursor_pointer())
                .child(SharedString::from(label))
        };
        let toggle_btn = div()
            .flex()
            .flex_row()
            .rounded_sm()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(theme().surface))
            .child(seg("cp-view-list", "List", !tree_view).on_click(list_click))
            .child(seg("cp-view-tree", "Tree", tree_view).on_click(tree_click));

        // ── Helper: build file rows for a section ────────────────
        // Returns a Vec of (element, depth, name, is_conflicted) as IntoElement.
        // We render inline to avoid capture issues.

        // ── Unstaged section ─────────────────────────────────────
        // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

        // Unstaged ヘッダ行 (固定 — flex_shrink_0 で高さを保持)
        let unstaged_header = div()
            .flex()
            .flex_row()
            .items_center()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(format!("Unstaged ({})", unstaged_count))),
            )
            .when(unstaged_count > 0, |el| {
                let stage_all_click = cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.defer_stage_all(window, cx);
                    },
                );
                el.child(
                    div()
                        .id("cp-stage-all")
                        .mr_2()
                        .px_1p5()
                        .py_px()
                        .rounded_sm()
                        .bg(rgb(theme().surface))
                        .text_xs()
                        .text_color(rgb(theme().color_success))
                        .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(stage_all_click)
                        .child(SharedString::from("Stage all")),
                )
            })
            // W17-DISCARD: "Discard all" — disabled (muted, no handler) at 0 targets.
            .when(unstaged_count > 0, |el| {
                let discard_all_click = cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.defer_open_discard_all(window, cx);
                    },
                );
                let enabled = discard_eligible_count > 0;
                let mut btn = div()
                    .id("cp-discard-all")
                    .mr_2()
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .child(SharedString::from("Discard all"));
                if enabled {
                    btn = btn
                        .text_color(rgb(theme().color_blocker))
                        .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(discard_all_click);
                } else {
                    btn = btn.text_color(rgb(theme().text_muted));
                }
                el.child(btn)
            })
            .child(toggle_btn);

        // PERF: unstaged file rows are virtualized via `uniform_list` (built from
        // free row functions reading the entity), not a prebuilt div.
        let unstaged_row_count = if tree_view {
            panel.unstaged_tree.len()
        } else {
            unstaged_count
        };

        // ── Staged section ───────────────────────────────────────
        // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

        // Staged ヘッダ行 (固定)
        let staged_header = div()
            .flex()
            .flex_row()
            .items_center()
            .px_2()
            .py_1()
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(format!("Staged ({})", staged_count))),
            )
            .when(staged_count > 0, |el| {
                let unstage_all_click = cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.defer_unstage_all(window, cx);
                    },
                );
                el.child(
                    div()
                        .id("cp-unstage-all")
                        .px_1p5()
                        .py_px()
                        .rounded_sm()
                        .bg(rgb(theme().surface))
                        .text_xs()
                        .text_color(rgb(theme().color_warning))
                        .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(unstage_all_click)
                        .child(SharedString::from("Unstage all")),
                )
            });

        // PERF: staged file rows are virtualized via `uniform_list` (built from
        // free row functions reading the entity), not a prebuilt div.
        let staged_row_count = if tree_view {
            panel.staged_tree.len()
        } else {
            staged_count
        };

        // ── Commit message: subject + body (ADR-0134) ─────────────────
        // Two inputs mirroring git's own shape, replacing the single field and
        // the six-field template mode. The body is seeded from the user's
        // `commit.template` on first open and holds the co-author trailers.
        let msg_inputs: gpui::AnyElement = match (&title_input, &body_input) {
            (Some(title), Some(body)) => div()
                .flex()
                .flex_col()
                .gap_1()
                .child(Input::new(title).appearance(true).bordered(true))
                .child(Input::new(body).appearance(true).bordered(true))
                .into_any_element(),
            // No window yet (headless); the message still flows through
            // `state.commit_msg`, so this is a placeholder, not a failure.
            _ => div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(commit message input unavailable)"))
                .into_any_element(),
        };

        // ── Commit button ─────────────────────────────────────────
        // The destination branch is on the button rather than in a preview line
        // above it (ADR-0134) — one place to look before committing.
        let branch_label = preview
            .as_ref()
            .map(|p| p.target_branch.clone())
            .unwrap_or_default();
        let commit_label = SharedString::from(i18n::commit_to_branch(&branch_label));
        let commit_btn = if can_commit {
            let commit_click = cx.listener(
                |view: &mut CommitPanelView, _event: &gpui::ClickEvent, window, cx| {
                    view.defer_open_commit_plan_modal(window, cx);
                },
            );
            Button::new("cp-commit-btn")
                .label(commit_label)
                .primary()
                .small()
                .mt_1()
                .w_full()
                .on_click(commit_click)
                .into_any_element()
        } else {
            // Same label, visibly inert. The reason it is disabled is already on
            // screen (nothing staged / no summary typed), so it is not repeated.
            div()
                .id("cp-commit-btn-disabled")
                .mt_1()
                .w_full()
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .flex()
                .justify_center()
                .child(commit_label)
                .into_any_element()
        };

        // ── Footer icon row (ADR-0134) ────────────────────────────
        // Three icon actions replace the old pill toolbar + full-width Amend
        // button: sparkles = generate a message, person+ = add a co-author,
        // undo = amend the previous commit.
        let staged_empty = panel.staged.is_empty();
        let icon_row = {
            let icon_btn =
                |id: &'static str, path: &'static str, tip: String, enabled: bool, accent: u32| {
                    let mut b = div()
                        .id(id)
                        .p_1()
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(move |_w, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(_w, cx)
                        })
                        .child(
                            gpui::svg()
                                .path(path)
                                .w(theme::scaled_px(16.0))
                                .h(theme::scaled_px(16.0))
                                .text_color(rgb(if enabled { accent } else { theme().text_muted })),
                        );
                    if enabled {
                        b = b.hover(|s| s.bg(rgb(theme().selected)).cursor_pointer());
                    }
                    b
                };

            // Sparkles — generate a commit message. Uses the local LLM when it is
            // usable (green), otherwise the rule-based draft.
            let llm_on = smart.llm_offered();
            let suggest_enabled = !staged_empty && !smart.generating;
            let suggest_color = if llm_on {
                theme().color_success
            } else {
                theme().color_branch
            };
            let suggest_btn: gpui::AnyElement = if smart.generating {
                // Spin the same icon rather than swapping in a text spinner, so
                // the row does not change width mid-generation.
                use gpui::AnimationExt as _;
                div()
                    .id("cp-smart-suggest")
                    .p_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui::svg()
                            .path("icons/loader-circle.svg")
                            .w(theme::scaled_px(16.0))
                            .h(theme::scaled_px(16.0))
                            .text_color(rgb(suggest_color))
                            .with_animation(
                                "cp-smart-spinner",
                                gpui::Animation::new(Duration::from_millis(900)).repeat(),
                                |svg, delta| {
                                    svg.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(delta * std::f32::consts::TAU),
                                    ))
                                },
                            ),
                    )
                    .into_any_element()
            } else {
                let mut b = icon_btn(
                    "cp-smart-suggest",
                    "icons/sparkles.svg",
                    Msg::GenerateMessage.t().to_string(),
                    suggest_enabled,
                    suggest_color,
                );
                if suggest_enabled {
                    b = b.on_click(cx.listener(
                        move |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                            if llm_on {
                                view.defer_smart_generate(window, cx);
                            } else {
                                view.defer_smart_suggest(window, cx);
                            }
                        },
                    ));
                }
                b.into_any_element()
            };

            // Person+ — co-author picker.
            let coauthor_btn = icon_btn(
                "cp-coauthor",
                "icons/user-plus.svg",
                Msg::AddCoAuthor.t().to_string(),
                true,
                theme().text_main,
            )
            .on_click(cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _w, cx| {
                    view.toggle_coauthor_menu(cx);
                },
            ));

            // Undo — amend the previous commit (the plan still blocks pushed
            // commits; this only opens the modal).
            let amend_btn = icon_btn(
                "cp-amend-btn",
                "icons/undo-2.svg",
                Msg::AmendLastCommit.t().to_string(),
                true,
                theme().color_warning,
            )
            .on_click(cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                    view.defer_amend(window, cx);
                },
            ));

            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(suggest_btn)
                .child(coauthor_btn)
                .child(amend_btn);

            // Transient smart-commit status (generating / inserted / fell back)
            // stays as a quiet trailing line rather than its own toolbar row.
            if let Some(ref status) = smart.status {
                row = row.child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(status.clone())),
                );
            }
            div()
                .relative()
                .child(row)
                .children(render_coauthor_menu(coauthor_menu.as_deref(), cx))
        };

        // ── Assemble panel ───────────────────────────────────────
        // T-UI-003: diff ボックス廃止。Unstaged/Staged 箱が flex_1 で全体を占める(1:1)。
        div()
            // `panel_width` is the unscaled, persisted right-panel width; scale at
            // render so it tracks zoom (the Panel divider drag uses the same space).
            .w(theme::scaled_px(panel_width))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(theme().panel))
            // Header
            .child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .bg(rgb(theme().surface))
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from("Commit Panel")),
            )
            // T-UI-003: ファイル領域コンテナ (flex_1 + min_h(0)) — diff 廃止でフル高さ
            .child(
                div()
                    .id("cp-files-container")
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    // Unstaged ヘッダ (固定)
                    .child(unstaged_header)
                    // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                    .child(
                        div()
                            .id("cp-unstaged-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .mx_1()
                            .mb_px()
                            .border_1()
                            .border_color(rgb(theme().surface))
                            .rounded_sm()
                            .flex()
                            .flex_col()
                            .child({
                                let handle = unstaged_scroll_handle.clone();
                                with_vertical_scrollbar(
                                    "cp-unstaged-list-scroll",
                                    &handle,
                                    uniform_list(
                                        "cp-unstaged-list",
                                        unstaged_row_count,
                                        cx.processor(
                                            move |view,
                                                  range: std::ops::Range<usize>,
                                                  _window,
                                                  cx| {
                                                let tree = view.state.tree_view;
                                                range
                                                    .filter_map(|i| {
                                                        if tree {
                                                            render_unstaged_tree_row(view, i, cx)
                                                        } else {
                                                            render_unstaged_flat_row(view, i, cx)
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&unstaged_scroll_handle)
                                    .flex_1()
                                    .min_h(px(0.)),
                                    false,
                                )
                            }),
                    )
                    // Staged ヘッダ (固定)
                    .child(staged_header)
                    // Staged スクロールボックス — PERF: virtualized uniform_list.
                    .child(
                        div()
                            .id("cp-staged-scroll")
                            .flex_1()
                            .min_h(px(0.))
                            .mx_1()
                            .mb_px()
                            .border_1()
                            .border_color(rgb(theme().surface))
                            .rounded_sm()
                            .flex()
                            .flex_col()
                            .child({
                                let handle = staged_scroll_handle.clone();
                                with_vertical_scrollbar(
                                    "cp-staged-list-scroll",
                                    &handle,
                                    uniform_list(
                                        "cp-staged-list",
                                        staged_row_count,
                                        cx.processor(
                                            move |view,
                                                  range: std::ops::Range<usize>,
                                                  _window,
                                                  cx| {
                                                let tree = view.state.tree_view;
                                                range
                                                    .filter_map(|i| {
                                                        if tree {
                                                            render_staged_tree_row(view, i, cx)
                                                        } else {
                                                            render_staged_flat_row(view, i, cx)
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(&staged_scroll_handle)
                                    .flex_1()
                                    .min_h(px(0.)),
                                    false,
                                )
                            }),
                    ),
            )
            // Commit footer: subject + body, icon actions, commit button.
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .bg(rgb(theme().surface))
                    // Subject + body inputs
                    .child(msg_inputs)
                    // ✨ generate · 👤+ co-author · ⟲ amend
                    .child(icon_row)
                    // Unstaged warning
                    .when(has_unstaged_warning, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().color_warning))
                                .child(SharedString::from(i18n::unstaged_not_included(
                                    unstaged_count,
                                ))),
                        )
                    })
                    .child(commit_btn),
            )
    }
}
