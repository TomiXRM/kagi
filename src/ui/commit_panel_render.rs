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
//! plain↔template, type-chip pick) stay synchronous + a child `cx.notify()`.
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

    fn defer_smart_toggle_lang(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| {
                app.smart_commit.toggle_lang();
                cx.notify();
            });
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
        let commit_input = self.commit_input.clone();
        let template_mode = self.commit_template_mode;
        let template_inputs = self.commit_template_inputs.clone();
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
        // T026 / T-COMMIT-009: can_commit uses the effective message — in template
        // mode the assembled fields, else the plain Input value (headless: commit_msg).
        let input_msg_nonempty = if template_mode {
            // Non-empty when summary or any field yields a non-empty assembled message.
            template_inputs
                .as_ref()
                .map(|inp| {
                    let fields = kagi_git::TemplateFields::new(
                        inp[0].read(cx).value().to_string(),
                        inp[1].read(cx).value().to_string(),
                        inp[2].read(cx).value().to_string(),
                        inp[3].read(cx).value().to_string(),
                        inp[4].read(cx).value().to_string(),
                        inp[5].read(cx).value().to_string(),
                    );
                    !kagi_git::assemble(&fields).trim().is_empty()
                })
                .unwrap_or(false)
        } else {
            commit_input
                .as_ref()
                .map(|e| !e.read(cx).value().trim().is_empty())
                .unwrap_or(!panel.commit_msg.trim().is_empty())
        };
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

        // ── plain ⇄ template mode toggle (T-COMMIT-009) ───────────────
        let mode_toggle = {
            let toggle_click = cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                    view.toggle_template_mode(window, cx);
                },
            );
            let label = if template_mode {
                "Plain message"
            } else {
                "Template fields"
            };
            div()
                .id("cp-template-toggle")
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .bg(rgb(theme().surface))
                .text_color(rgb(theme().color_branch))
                .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                .on_click(toggle_click)
                .child(SharedString::from(format!("⇄ {}", label)))
        };

        // ── Commit message input (T026/T-COMMIT-009) ──────────────────
        // Template mode renders the six structured fields (gpui-component Input for
        // each — no hand-written widgets); plain mode renders the single Input.
        let msg_input_wrapper: gpui::AnyElement = if template_mode {
            if let Some(inp) = template_inputs.clone() {
                let [ty, scope, summary, body, test, risk] = inp;

                // Labeled single-line field.
                let field = |label: &'static str, state: &Entity<InputState>| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_label))
                                .child(SharedString::from(label)),
                        )
                        .child(Input::new(state).appearance(true).bordered(true))
                };

                // type quick-pick chips (also free-typeable in the type field above).
                let mut chips = div().flex().flex_row().flex_wrap().gap_1();
                for &choice in kagi_git::TYPE_CHOICES {
                    let ty_state = ty.clone();
                    let pick = cx.listener(
                        move |_view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                            ty_state
                                .update(cx, |s, cx| s.set_value(choice.to_string(), window, cx));
                        },
                    );
                    chips = chips.child(
                        div()
                            .id(SharedString::from(format!("cp-type-chip-{}", choice)))
                            .px_1()
                            .py_px()
                            .rounded_sm()
                            .text_xs()
                            .bg(rgb(theme().surface))
                            .text_color(rgb(theme().text_main))
                            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                            .on_click(pick)
                            .child(SharedString::from(choice)),
                    );
                }

                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field("type", &ty))
                    .child(chips)
                    .child(field("scope", &scope))
                    .child(field("summary", &summary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_px()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().text_label))
                                    .child(SharedString::from("body")),
                            )
                            .child(Input::new(&body).appearance(true).bordered(true)),
                    )
                    .child(field("test", &test))
                    .child(field("risk", &risk))
                    .into_any_element()
            } else {
                // Template mode requested but inputs not yet created (no &mut Window
                // here) — should not occur because the toggle creates them.
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("(template fields unavailable)"))
                    .into_any_element()
            }
        } else if let Some(ref input_entity) = commit_input {
            // Use gpui-component Input element — handles IME, clipboard, arrow keys, etc.
            Input::new(input_entity)
                .appearance(true)
                .bordered(true)
                .into_any_element()
        } else {
            // Fallback for headless / no-window case (should not occur in normal UI flow).
            div()
                .px_2()
                .py_1()
                .bg(rgb(theme().bg_base))
                .rounded_sm()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(commit message input unavailable)"))
                .into_any_element()
        };

        // ── Commit button ─────────────────────────────────────────
        let commit_btn = if can_commit {
            let commit_click = cx.listener(
                |view: &mut CommitPanelView, _event: &gpui::ClickEvent, window, cx| {
                    view.defer_open_commit_plan_modal(window, cx);
                },
            );
            Button::new("cp-commit-btn")
                .label(SharedString::from(format!(
                    "Commit ({} file{})",
                    staged_count,
                    if staged_count == 1 { "" } else { "s" }
                )))
                .primary()
                .small()
                .mt_1()
                .w_full()
                .on_click(commit_click)
                .into_any_element()
        } else {
            // Tell the user exactly why the button is disabled.
            let reason = if staged_count == 0 && !input_msg_nonempty {
                "Commit — stage a file and enter a message first"
            } else if staged_count == 0 {
                "Commit — stage at least one file first"
            } else {
                "Commit — enter a commit message first"
            };
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
                .child(SharedString::from(reason))
                .into_any_element()
        };

        // ── Smart Commit Message toolbar (T-COMMIT-016) ───────────
        // Rule-based "Suggest" is always available; "Generate with Local LLM" is
        // offered only when an Ollama server is detected and the user opted in.
        let staged_empty = panel.staged.is_empty();
        let smart_toolbar = {
            // Small reusable button factory.
            let pill = |id: &'static str, label: SharedString, enabled: bool, accent: u32| {
                let mut b = div()
                    .id(id)
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .text_xs()
                    .bg(rgb(theme().surface))
                    .text_color(rgb(if enabled { accent } else { theme().text_muted }))
                    .child(label);
                if enabled {
                    b = b.hover(|s| s.bg(rgb(theme().selected)).cursor_pointer());
                }
                b
            };

            // Suggest — one button: uses the local LLM when it's usable (green),
            // otherwise the rule-based draft (blue). Shows "Generating…" while the
            // LLM runs. (The separate "Generate with Local LLM" button is gone.)
            let llm_on = smart.llm_offered();
            let suggest_enabled = !staged_empty && !smart.generating;
            let suggest_color = if llm_on {
                theme().color_success
            } else {
                theme().color_branch
            };
            let suggest_btn: gpui::AnyElement = if smart.generating {
                // Animated braille "dots" spinner while the LLM generates (user
                // request — the spinning-dots glyph). The whole panel re-renders each
                // animation frame, so the closure rebuilds a fresh single-child div.
                use gpui::AnimationExt as _;
                const FRAMES: [&str; 10] = [
                    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}",
                    "\u{2826}", "\u{2827}", "\u{2807}", "\u{280F}",
                ];
                let spinner = div()
                    .text_xs()
                    .text_color(rgb(suggest_color))
                    .with_animation(
                        "cp-smart-spinner",
                        gpui::Animation::new(Duration::from_millis(800)).repeat(),
                        |el, delta| {
                            let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
                            el.child(SharedString::from(FRAMES[i]))
                        },
                    );
                div()
                    .id("cp-smart-suggest")
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .text_xs()
                    .bg(rgb(theme().surface))
                    .text_color(rgb(suggest_color))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(spinner)
                    .child(SharedString::from("Generating…"))
                    .into_any_element()
            } else {
                let mut b = pill(
                    "cp-smart-suggest",
                    SharedString::from("Suggest"),
                    suggest_enabled,
                    suggest_color,
                );
                if suggest_enabled {
                    let suggest_click = cx.listener(
                        move |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                            if llm_on {
                                view.defer_smart_generate(window, cx);
                            } else {
                                view.defer_smart_suggest(window, cx);
                            }
                        },
                    );
                    b = b.on_click(suggest_click);
                }
                b.into_any_element()
            };

            // Lang toggle (En / 日本語).
            let lang_label = match smart.lang {
                message_gen::Lang::En => "Lang: EN",
                message_gen::Lang::Ja => "Lang: 日本語",
            };
            let lang_click = cx.listener(
                |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                    view.defer_smart_toggle_lang(window, cx);
                },
            );
            let lang_btn = pill(
                "cp-smart-lang",
                SharedString::from(lang_label),
                true,
                theme().text_main,
            )
            .on_click(lang_click);

            // ADR-0090: the Style (CC vs Plain) toggle was removed — style now
            // follows the commit-panel mode (template → Conventional, plain → Plain).

            let mut row = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(suggest_btn)
                .child(lang_btn);

            // "Generate with Local LLM" is folded into Suggest (above). When the LLM
            // is detected but not yet enabled, offer an opt-in affordance so the user
            // can turn it on (after which Suggest goes green and uses it).
            if smart.ollama_available && !smart.llm_enabled {
                let enable_click = cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.defer_smart_generate(window, cx);
                    },
                );
                let enable_btn = pill(
                    "cp-smart-enable-llm",
                    SharedString::from("Enable Local LLM…"),
                    !staged_empty,
                    theme().color_success,
                )
                .when(!staged_empty, |el| el.on_click(enable_click));
                row = row.child(enable_btn);
            }

            // "Local LLM available" indicator.
            let mut col = div().flex().flex_col().gap_px().child(row);
            if smart.ollama_available {
                col = col.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().color_success))
                        .child(SharedString::from("● Local LLM available")),
                );
            }
            // Transient status line (rule-based inserted / generating / fell back).
            if let Some(ref status) = smart.status {
                col = col.child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(status.clone())),
                );
            }
            col
        };

        // ── Commit preview header (T-COMMIT-001) ──────────────────
        // Shows what the *next* commit contains: staged count, A/M/D summary,
        // target branch (detached/unborn handled), and author.  Pure read from
        // `commit_preview()`; hidden if the preview could not be built.
        let preview_block: gpui::AnyElement = if let Some(ref pv) = preview {
            let count_line = format!(
                "{} file{} staged",
                pv.staged_count,
                if pv.staged_count == 1 { "" } else { "s" }
            );
            let summary = pv.summary();
            let mut col = div()
                .flex()
                .flex_col()
                .gap_px()
                // Line 1: count + A/M/D summary
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(count_line)),
                        )
                        .when(!summary.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().text_muted))
                                    .child(SharedString::from(summary)),
                            )
                        }),
                );
            // Line 2: target branch
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .overflow_hidden()
                    .truncate()
                    .child(SharedString::from(format!("→ {}", pv.target_branch))),
            );
            // Line 3: author
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .overflow_hidden()
                    .truncate()
                    .child(SharedString::from(format!("by {}", pv.author))),
            );
            col.into_any_element()
        } else {
            div().into_any_element()
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
                                    .track_scroll(unstaged_scroll_handle)
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
                                    .track_scroll(staged_scroll_handle)
                                    .flex_1()
                                    .min_h(px(0.)),
                                    false,
                                )
                            }),
                    ),
            )
            // Commit footer: message input + warning + button
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .bg(rgb(theme().surface))
                    // T-COMMIT-001: staged preview (count / A·M·D / branch / author)
                    .child(preview_block)
                    // Message label + plain⇄template toggle
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(div().text_xs().text_color(rgb(theme().text_label)).child(
                                SharedString::from(if template_mode {
                                    "Commit message (template)"
                                } else {
                                    "Commit message"
                                }),
                            ))
                            .child(mode_toggle),
                    )
                    // Template mode stacks six fields and overflows the footer; bound
                    // its height and let it scroll so the commit button stays reachable.
                    .child(if template_mode {
                        div()
                            .id("cp-template-scroll")
                            .max_h(theme::scaled_px(300.))
                            .overflow_y_scroll()
                            .child(msg_input_wrapper)
                            .into_any_element()
                    } else {
                        msg_input_wrapper
                    })
                    // Smart Commit Message toolbar (Suggest / Generate / toggles)
                    .child(smart_toolbar)
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
                    // Commit button
                    .child(commit_btn)
                    // T-COMMIT-011: Amend the previous commit (unpushed only —
                    // the plan blocks pushed/merge/etc.). Mode follows what the
                    // user has provided: staged changes, a new message, or both.
                    .child({
                        let amend_click = cx.listener(
                            |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                                view.defer_amend(window, cx);
                            },
                        );
                        div()
                            .id("cp-amend-btn")
                            .mt_1()
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(theme().surface))
                            .text_sm()
                            .text_color(rgb(theme().color_warning))
                            .on_click(amend_click)
                            .hover(|st| st.bg(rgb(theme().selected)))
                            .child(SharedString::from("Amend last commit…"))
                    }),
            )
    }
}
