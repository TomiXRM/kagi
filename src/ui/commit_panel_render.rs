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
use gpui_component::Disableable as _;

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from the
// `CommitPanelView` entity (NOT a captured-by-value clone). They are invoked
// from the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// The four Commit Panel file rows (unstaged/staged × flat/tree) differ only in
/// data lookup, padding, action button and the discard menu — ADR-0132's lesson
/// applied to this panel: one renderer, four thin callers.
///
/// Returns a `Stateful<Div>` (like `kagi_ui_core::commit_row::render_commit_row`)
/// so a caller can still attach its own interactions on top.
///
/// Per-variant differences kept deliberately:
/// * `tree` rows are indented (`pl(8+indent)` / `pr(2)`) where flat rows use `px_2`.
/// * staged rows get an **Unstage** button, unstaged rows a **Stage** button.
/// * only unstaged rows get the right-click file menu (Discard lives there,
///   W17-DISCARD / ADR-0083) and the conflicted treatment.
/// * element ids keep their per-variant prefixes, and the staged diffstat keeps
///   its `fi + 100_000` id seed so the two lists never collide.
fn render_cp_file_row(
    staged: bool,
    tree: bool,
    fi: usize,
    name: SharedString,
    change: Option<&kagi_git::ChangeKind>,
    is_conflicted: bool,
    is_sel: bool,
    wip_hit: bool,
    indent: f32,
    stat: Option<&kagi_git::FileDiffStat>,
    convention: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::Stateful<gpui::Div> {
    let (row_id, btn_id, conflict_id) = match (staged, tree) {
        (false, false) => (
            "cp-us-flat-file",
            "cp-us-flat-stage-btn",
            "cp-us-flat-conflict-badge",
        ),
        (false, true) => ("cp-us-file", "cp-us-stage-btn", "cp-us-conflict-badge"),
        (true, false) => ("cp-st-flat-file", "cp-st-flat-unstage-btn", ""),
        (true, true) => ("cp-st-file", "cp-st-unstage-btn", ""),
    };
    let (badge, badge_color, _) = status_badge(change, is_conflicted);
    let file_ref = if staged {
        CommitPanelFileRef::Staged { index: fi }
    } else {
        CommitPanelFileRef::Unstaged { index: fi }
    };
    let file_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        view.defer_select_file(file_ref.clone(), window, cx);
    });
    // Row background: conflicted files get red tint
    let row_bg = if is_conflicted {
        theme().diff_removed_bg
    } else if is_sel {
        theme().selected
    } else {
        theme().panel
    };
    let mut file_row = div()
        .id((row_id, fi))
        .when(wip_hit, |el| el.bg(rgb(theme().selected)))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .map(|el| {
            if tree {
                el.pl(theme::scaled_px(8.0 + indent))
                    .pr(theme::scaled_px(2.0))
            } else {
                el.px_2()
            }
        })
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
                .child(name),
        )
        .when(convention, |el| {
            // issue #338: emphasis badge on a convention-body file — these steer
            // agent behaviour, so they are highlighted, never folded.
            el.child(
                div()
                    .ml_1()
                    .mr_2()
                    .px(px(3.))
                    .rounded_sm()
                    .flex_shrink_0()
                    .bg(rgb(theme().color_warning))
                    .text_size(px(9.))
                    .line_height(px(14.))
                    .text_color(rgb(theme().bg_base))
                    .child(SharedString::from(Msg::AgentConventionBadge.t())),
            )
        })
        .child(diffstat_bar::diffstat_unit(
            if staged { fi + 100_000 } else { fi },
            stat,
        ));
    if is_conflicted {
        // Conflicted rows can be neither staged nor discarded from here.
        return file_row.child(
            div()
                .id((conflict_id, fi))
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
    if !staged {
        // W17-DISCARD / ADR-0083: right-click opens the file context menu
        // (Discard lives there). Tracked rows are restored from the index;
        // untracked rows are deleted (after an ODB backup).
        let menu_click = cx.listener(move |view, e: &gpui::MouseDownEvent, window, cx| {
            cx.stop_propagation();
            view.defer_open_file_menu(fi, e.position, window, cx);
        });
        file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
    }
    let (label, accent) = if staged {
        ("Unstage", theme().color_warning)
    } else {
        ("Stage", theme().color_success)
    };
    let action_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        if staged {
            view.defer_unstage_file(fi, window, cx);
        } else {
            view.defer_stage_file(fi, window, cx);
        }
    });
    file_row.child(
        KagiButton::accent((btn_id, fi), label, accent, cx)
            .xsmall()
            .ml_2()
            .flex_shrink_0()
            .on_click(action_click),
    )
}

/// A directory row in either tree list (`prefix` is the id namespace).
fn render_cp_dir_row(prefix: &str, depth: usize, name: &SharedString) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("{}-{}", prefix, name.as_ref())))
        .pl(theme::scaled_px(8.0 + (depth as f32) * 12.0))
        .py_px()
        .text_xs()
        .text_color(rgb(theme().change_dir))
        .child(name.clone())
        .into_any_element()
}

/// issue #348: the collapsible "Generated (N)" disclosure header row for a
/// section. Clicking it toggles `generated_expanded` (shared by both sections).
fn render_cp_generated_header(
    staged: bool,
    count: usize,
    expanded: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::AnyElement {
    let id = if staged {
        "cp-st-generated-header"
    } else {
        "cp-us-generated-header"
    };
    render_cp_fold_header(
        id,
        i18n::Msg::GeneratedFilesSection.t(),
        count,
        expanded,
        false,
        cx,
    )
}

/// issue #338: the collapsible "Agent artifacts (N)" disclosure header row.
/// Clicking it toggles `agent_expanded` (shared by both sections).
fn render_cp_agent_header(
    staged: bool,
    count: usize,
    expanded: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::AnyElement {
    let id = if staged {
        "cp-st-agent-header"
    } else {
        "cp-us-agent-header"
    };
    render_cp_fold_header(
        id,
        i18n::Msg::AgentArtifactsSection.t(),
        count,
        expanded,
        true,
        cx,
    )
}

/// Shared collapsible fold header (issue #348 generated + #338 agent). `agent`
/// selects which expansion flag the click toggles.
fn render_cp_fold_header(
    id: &'static str,
    section_label: &str,
    count: usize,
    expanded: bool,
    agent: bool,
    cx: &mut Context<CommitPanelView>,
) -> gpui::AnyElement {
    let arrow = if expanded { "▾" } else { "▸" };
    let label = format!("{arrow} {section_label} ({count})");
    let toggle = cx.listener(
        move |view: &mut CommitPanelView, _e: &gpui::ClickEvent, _window, cx| {
            if agent {
                view.state.agent_expanded = !view.state.agent_expanded;
            } else {
                view.state.generated_expanded = !view.state.generated_expanded;
            }
            cx.notify();
        },
    );
    div()
        .id(id)
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_px()
        .hover(|s| s.bg(rgb(theme().surface)).cursor_pointer())
        .on_click(toggle)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_label))
                .truncate()
                .child(SharedString::from(label)),
        )
        .into_any_element()
}

/// issue #338: whether `path` is an agent convention body (badge, never folded).
fn is_convention(path: &std::path::Path) -> bool {
    use kagi_domain::agent_artifacts::{classify_agent_artifact, AgentArtifactKind};
    classify_agent_artifact(&path.to_string_lossy()) == AgentArtifactKind::ConventionBody
}

/// One unstaged file row (flat style) by index `fi` into `unstaged`. Shared by
/// the flat list, the tree list's folded section, and the folded "Generated"
/// rows (issue #348).
fn cp_unstaged_file_element(
    view: &CommitPanelView,
    fi: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let f = panel.unstaged.get(fi)?;
    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let wip_hit = view
        .active_wip
        .as_ref()
        .is_some_and(|(st, p)| !*st && &f.path == p);
    Some(
        render_cp_file_row(
            false,
            false,
            fi,
            SharedString::from(name),
            Some(&f.change),
            panel.is_conflicted(&f.path),
            panel.selected_file == Some(CommitPanelFileRef::Unstaged { index: fi }),
            wip_hit,
            0.0,
            panel.unstaged_stat(&f.path),
            is_convention(&f.path),
            cx,
        )
        .into_any_element(),
    )
}

/// One staged file row (flat style) by index `fi` into `staged` (issue #348).
fn cp_staged_file_element(
    view: &CommitPanelView,
    fi: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let f = panel.staged.get(fi)?;
    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let wip_hit = view
        .active_wip
        .as_ref()
        .is_some_and(|(st, p)| *st && &f.path == p);
    Some(
        render_cp_file_row(
            true,
            false,
            fi,
            SharedString::from(name),
            Some(&f.change),
            false,
            panel.selected_file == Some(CommitPanelFileRef::Staged { index: fi }),
            wip_hit,
            0.0,
            panel.staged_stat(&f.path),
            is_convention(&f.path),
            cx,
        )
        .into_any_element(),
    )
}

/// One file row (flat style) for either side, dispatched by `staged`.
fn cp_file_element(
    view: &CommitPanelView,
    staged: bool,
    fi: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    if staged {
        cp_staged_file_element(view, fi, cx)
    } else {
        cp_unstaged_file_element(view, fi, cx)
    }
}

/// The two trailing fold regions of a section, after the base (normal/tree)
/// rows: first "Generated (N)" (issue #348), then "Agent artifacts (N)"
/// (issue #338). `j` is the display-row offset past the base region.
fn render_cp_fold_regions(
    view: &CommitPanelView,
    j: usize,
    staged: bool,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let (gen_files, art_files) = if staged {
        (&panel.staged_gen_files, &panel.staged_artifact_files)
    } else {
        (&panel.unstaged_gen_files, &panel.unstaged_artifact_files)
    };
    let gen_extra = panel.generated_extra_rows(staged);
    if j < gen_extra {
        if j == 0 {
            return Some(render_cp_generated_header(
                staged,
                gen_files.len(),
                panel.generated_expanded,
                cx,
            ));
        }
        let fi = *gen_files.get(j - 1)?;
        return cp_file_element(view, staged, fi, cx);
    }
    // Agent-artifacts fold, immediately after the generated region.
    let k = j - gen_extra;
    if k == 0 {
        return Some(render_cp_agent_header(
            staged,
            art_files.len(),
            panel.agent_expanded,
            cx,
        ));
    }
    let fi = *art_files.get(k - 1)?;
    cp_file_element(view, staged, fi, cx)
}

/// PERF: build one unstaged row in flat view. `i` is a display-row index:
/// normal files first, then the "Generated" / "Agent artifacts" folds.
pub(crate) fn render_unstaged_flat_row(
    view: &CommitPanelView,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let normal = &panel.unstaged_normal_files;
    if i < normal.len() {
        return cp_unstaged_file_element(view, normal[i], cx);
    }
    render_cp_fold_regions(view, i - normal.len(), false, cx)
}

/// PERF: build one unstaged tree row. `i` indexes the pruned tree first, then
/// the "Generated (N)" fold (issue #348).
pub(crate) fn render_unstaged_tree_row(
    view: &CommitPanelView,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.unstaged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, i - tree_len, false, cx);
    }
    match panel.unstaged_tree.get(i)? {
        file_tree::TreeRow::Dir { depth, name } => {
            Some(render_cp_dir_row("cp-us-dir", *depth, name))
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let fi = *file_index;
            // Look up the original path to check if conflicted
            let path = panel.unstaged.get(fi).map(|f| &f.path);
            let wip_hit = view
                .active_wip
                .as_ref()
                .zip(path)
                .is_some_and(|((st, p), fp)| !*st && fp == p);
            Some(
                render_cp_file_row(
                    false,
                    true,
                    fi,
                    name.clone(),
                    change.as_ref(),
                    path.map(|p| panel.is_conflicted(p)).unwrap_or(false),
                    panel.selected_file == Some(CommitPanelFileRef::Unstaged { index: fi }),
                    wip_hit,
                    (*depth as f32) * 12.0,
                    path.and_then(|p| panel.unstaged_stat(p)),
                    path.map(|p| is_convention(p)).unwrap_or(false),
                    cx,
                )
                .into_any_element(),
            )
        }
    }
}

/// PERF: build one staged row in flat view. `i` is a display-row index:
/// normal files first, then the "Generated (N)" fold (issue #348).
pub(crate) fn render_staged_flat_row(
    view: &CommitPanelView,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let normal = &panel.staged_normal_files;
    if i < normal.len() {
        return cp_staged_file_element(view, normal[i], cx);
    }
    render_cp_fold_regions(view, i - normal.len(), true, cx)
}

/// PERF: build one staged tree row. `i` indexes the pruned tree first, then the
/// "Generated (N)" fold (issue #348).
pub(crate) fn render_staged_tree_row(
    view: &CommitPanelView,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.staged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, i - tree_len, true, cx);
    }
    match panel.staged_tree.get(i)? {
        file_tree::TreeRow::Dir { depth, name } => {
            Some(render_cp_dir_row("cp-st-dir", *depth, name))
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let fi = *file_index;
            let path = panel.staged.get(fi).map(|f| &f.path);
            let wip_hit = view
                .active_wip
                .as_ref()
                .zip(path)
                .is_some_and(|((st, p), fp)| *st && fp == p);
            Some(
                render_cp_file_row(
                    true,
                    true,
                    fi,
                    name.clone(),
                    change.as_ref(),
                    false,
                    panel.selected_file == Some(CommitPanelFileRef::Staged { index: fi }),
                    wip_hit,
                    (*depth as f32) * 12.0,
                    path.and_then(|p| panel.staged_stat(p)),
                    path.map(|p| is_convention(p)).unwrap_or(false),
                    cx,
                )
                .into_any_element(),
            )
        }
    }
}

/// The co-author popover, or `None` when the picker is closed.
///
/// Anchored above the icon row (the footer sits at the bottom of the panel, so
/// a downward menu would open off-screen) and wrapped in `gpui::deferred` so it
/// paints after all of its ancestors. Without that the body box's own border is
/// drawn over the menu, which overflows that box (user report).
fn render_coauthor_menu(
    candidates: Option<&[kagi_git::AuthorCandidate]>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let candidates = candidates?;
    let mut menu = div()
        .absolute()
        .bottom(theme::scaled_px(26.0))
        // Anchored to the RIGHT edge so the menu grows leftward: the commit
        // panel is the right-most pane, and a left-anchored 260px menu opened
        // past the window edge (user report).
        .right_0()
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
        .py_1()
        .on_mouse_down_out(cx.listener(
            |view: &mut CommitPanelView, _e: &gpui::MouseDownEvent, _w, cx| {
                view.coauthor_menu = None;
                cx.notify();
            },
        ));

    if candidates.is_empty() {
        return Some(
            gpui::deferred(
                menu.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(Msg::NoRecentAuthors.t())),
                ),
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
    Some(gpui::deferred(menu).into_any_element())
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
                // Issue #286: resolve the row index to a PATH now, at open time,
                // so a later renumber (external `git add`) can't shift Discard.
                if let Some(path) = app
                    .commit_panel
                    .as_ref()
                    .and_then(|e| e.read(cx).state.unstaged.get(fi).map(|f| f.path.clone()))
                {
                    app.file_menu = Some((path, pos));
                    cx.notify();
                }
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
            panel.unstaged_normal_files.len()
        } + panel.generated_extra_rows(false)
            + panel.agent_extra_rows(false);

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
            panel.staged_normal_files.len()
        } + panel.generated_extra_rows(true)
            + panel.agent_extra_rows(true);

        // ── Commit button ─────────────────────────────────────────
        // No destination branch in the label: ADR-0134 put it here as "one
        // place to look", but a real branch name overflows the button. The
        // header shows the branch at all times and has room for a long one.
        let commit_label = SharedString::from(i18n::commit_button());
        let commit_click = cx.listener(
            |view: &mut CommitPanelView, _event: &gpui::ClickEvent, window, cx| {
                view.defer_open_commit_plan_modal(window, cx);
            },
        );
        // One Button in both states (`.disabled`), so enabling it does not change
        // the footer's height, and the disabled form still reads as a button
        // rather than blending into the footer background.
        let commit_btn = Button::new("cp-commit-btn")
            .label(commit_label)
            .primary()
            .small()
            .mt_1()
            .w_full()
            .disabled(!can_commit)
            .when(can_commit, |b| b.on_click(commit_click))
            .into_any_element();

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

            let row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(suggest_btn)
                .child(coauthor_btn)
                .child(amend_btn);

            div()
                .relative()
                .child(row)
                .children(render_coauthor_menu(coauthor_menu.as_deref(), cx))
        };

        // The radius `Input` gives itself, read once so both boxes match.
        let input_radius = gpui_component::ActiveTheme::theme(&**cx).radius;

        // ── commit.template toggle (bottom-left of the body box) ──────
        // Only offered when the user actually has a `commit.template`; without
        // one the control would toggle nothing.
        let template_toggle: gpui::AnyElement = if self.commit_template.is_some() {
            let on = self.template_active(cx);
            div()
                .id("cp-template-toggle")
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .text_color(rgb(if on {
                    theme().color_branch
                } else {
                    theme().text_muted
                }))
                .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                .tooltip(|w, cx| {
                    gpui_component::tooltip::Tooltip::new(Msg::ToggleCommitTemplate.t())
                        .build(w, cx)
                })
                .on_click(cx.listener(
                    |view: &mut CommitPanelView, _e: &gpui::ClickEvent, window, cx| {
                        view.toggle_template(window, cx);
                    },
                ))
                .child(SharedString::from(Msg::Template.t()))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // ── Commit message: subject + body (ADR-0134) ─────────────────
        // Two inputs mirroring git's own shape, replacing the single field and
        // the six-field template mode. The body is seeded from the user's
        // `commit.template` on first open and holds the co-author trailers.
        //
        // The icon row lives INSIDE the body's box: the box is a plain div
        // wearing the Input's own border/background, with an unstyled `Input`
        // and the icons stacked in it. Drawn as a real row rather than absolutely
        // positioned so it can never overlap a long body.
        let msg_inputs: gpui::AnyElement = match (&title_input, &body_input) {
            (Some(title), Some(body)) => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(Input::new(title).appearance(true).bordered(true))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // Same radius the `Input` gives itself, so the two boxes
                        // do not have visibly different corners.
                        .rounded(input_radius)
                        .border_1()
                        .border_color(rgb(theme().text_muted))
                        .bg(rgb(theme().bg_base))
                        .child(Input::new(body).appearance(false).bordered(false))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px_1()
                                .pb_1()
                                .child(template_toggle)
                                .child(icon_row),
                        ),
                )
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
                    .px_3()
                    .py_2()
                    .gap_2()
                    .bg(rgb(theme().surface))
                    // Subject + body (the icon actions live inside the body box)
                    .child(msg_inputs)
                    // Transient smart-commit status. Its own full-width line:
                    // inside the right-aligned icon group it pushed the icons
                    // sideways as the text appeared (user report).
                    .when_some(smart.status.clone(), |el, status| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(status)),
                        )
                    })
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
