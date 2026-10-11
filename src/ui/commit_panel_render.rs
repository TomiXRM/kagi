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

use super::button_style::{ButtonRole, KagiButton};
use super::commit_panel::CommitPanelView;
use super::*;

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from the
// `CommitPanelView` entity (NOT a captured-by-value clone). They are invoked
// from the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// Where a tree-view row sits in its section's `Tree` (#354): the tree id, the
/// flattened row index, its 1-based level and 1-based sibling position/size.
/// Flat view passes `None` and stays a plain run of rows.
#[derive(Clone, Copy)]
pub(crate) struct CpTreeSlot {
    tree: &'static str,
    index: usize,
    level: usize,
    position: (usize, usize),
}

/// Mark `el` as the `TreeItem` at `slot` (tree view only). The name is built
/// lazily so flat view never allocates it.
fn cp_tree_item(
    el: gpui::Stateful<gpui::Div>,
    slot: Option<CpTreeSlot>,
    label: impl FnOnce() -> String,
    expanded: Option<bool>,
    selected: Option<bool>,
) -> gpui::Stateful<gpui::Div> {
    match slot {
        Some(s) => kagi_ui_core::tree_a11y::tree_item(
            s.tree,
            el,
            s.index,
            &label(),
            s.level,
            expanded,
            selected,
            s.position,
        ),
        None => el,
    }
}

/// Accessible name of a file tree row: its base name (plus the convention
/// badge when shown) and its localized status. Filled by splitting the
/// template so a `{}` inside a file name is never taken for a placeholder.
fn cp_file_label(
    name: &str,
    change: Option<&kagi_git::ChangeKind>,
    is_conflicted: bool,
    convention: bool,
) -> String {
    let status = kagi_ui_core::tree_a11y::file_status(change, is_conflicted);
    let mut parts = Msg::A11yFileTreeFile.t().splitn(3, "{}");
    let mut s = String::from(parts.next().unwrap_or_default());
    s.push_str(name);
    if convention {
        s.push_str(" (");
        s.push_str(Msg::AgentConventionBadge.t());
        s.push(')');
    }
    s.push_str(parts.next().unwrap_or_default());
    s.push_str(status);
    s.push_str(parts.next().unwrap_or_default());
    s
}

/// Level and sibling position/size of every tree-view row of a section, in
/// display order (#354): the pruned file tree at depth + 1, then each
/// non-empty fold ("Generated", "Agent artifacts") as a top-level item with
/// its expanded files one level below. Linear; rebuilt on status changes or
/// fold toggles, never by the virtual list's per-scroll processor.
fn cp_tree_layout(
    panel: &super::commit_panel::CommitPanelState,
    staged: bool,
) -> Vec<(usize, (usize, usize))> {
    let tree = if staged {
        &panel.staged_tree
    } else {
        &panel.unstaged_tree
    };
    let mut levels: Vec<usize> = tree
        .iter()
        .map(|row| match row {
            file_tree::TreeRow::Dir { depth, .. } | file_tree::TreeRow::File { depth, .. } => {
                depth + 1
            }
        })
        .collect();
    for extra in [
        panel.generated_extra_rows(staged),
        panel.agent_extra_rows(staged),
    ] {
        if extra > 0 {
            levels.push(1);
            levels.extend(std::iter::repeat_n(2, extra - 1));
        }
    }
    let positions = kagi_ui_core::tree_a11y::sibling_positions(&levels);
    levels.into_iter().zip(positions).collect()
}

/// The slot of display row `index` in a section laid out by [`cp_tree_layout`].
pub(super) fn cp_tree_slot(
    layout: &[(usize, (usize, usize))],
    tree: &'static str,
    index: usize,
) -> Option<CpTreeSlot> {
    let &(level, position) = layout.get(index)?;
    Some(CpTreeSlot {
        tree,
        index,
        level,
        position,
    })
}

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
    view: &CommitPanelView,
    staged: bool,
    tree: bool,
    fi: usize,
    name: SharedString,
    change: Option<&kagi_git::ChangeKind>,
    is_conflicted: bool,
    is_sel: bool,
    wip_hit: bool,
    window_active: bool,
    indent: f32,
    stat: Option<&kagi_git::FileDiffStat>,
    convention: bool,
    slot: Option<CpTreeSlot>,
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
    let state = &view.state;
    let file = if staged {
        &state.staged[fi]
    } else {
        &state.unstaged[fi]
    };
    let full_path = SharedString::from(file.path.to_string_lossy().into_owned());
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
        rgb(theme().diff_removed_bg).into()
    } else if is_sel || wip_hit {
        theme().selected_for_window(window_active)
    } else {
        rgb(theme().panel).into()
    };
    let mut file_row = div()
        .id((row_id, fi))
        .map(|el| {
            cp_tree_item(
                el,
                slot,
                || cp_file_label(&name, change, is_conflicted, convention),
                None,
                Some(is_sel),
            )
        })
        .aria_description(full_path.clone())
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
        .bg(row_bg)
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
                .id((if staged { "cp-st-path" } else { "cp-us-path" }, fi))
                .aria_label(full_path.clone())
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(full_path.clone()).build(window, cx)
                })
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
                    .font_weight(gpui::FontWeight::BOLD)
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
    // #476 slice 3: offered on a linked worktree's panel too — Discard resolves
    // the panel's own repository now, and its confirm names that worktree.
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
    let (label, role) = if staged {
        ("Unstage", ButtonRole::Warning)
    } else {
        ("Stage", ButtonRole::Success)
    };
    let action_click = cx.listener(move |view, _event: &gpui::ClickEvent, window, cx| {
        if staged {
            view.defer_unstage_file(fi, window, cx);
        } else {
            view.defer_stage_file(fi, window, cx);
        }
    });
    file_row.child(
        KagiButton::styled((btn_id, fi), label, role, cx)
            .xsmall()
            .ml_2()
            .flex_shrink_0()
            .relative()
            .child(super::e2e::measure_inside(if staged {
                "cp-row-unstage"
            } else {
                "cp-row-stage"
            }))
            .on_click(action_click),
    )
}

/// A directory row in either tree list (`prefix` is the id namespace). Commit
/// panel directories do not collapse, so the item carries no expanded state.
fn render_cp_dir_row(
    prefix: &str,
    depth: usize,
    name: &SharedString,
    slot: Option<CpTreeSlot>,
) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("{}-{}", prefix, name.as_ref())))
        .map(|el| {
            cp_tree_item(
                el,
                slot,
                || Msg::A11yFileTreeDir.t().replacen("{}", name, 1),
                None,
                None,
            )
        })
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
    slot: Option<CpTreeSlot>,
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
        slot,
        cx,
    )
}

/// issue #338: the collapsible "Agent artifacts (N)" disclosure header row.
/// Clicking it toggles `agent_expanded` (shared by both sections).
fn render_cp_agent_header(
    staged: bool,
    count: usize,
    expanded: bool,
    slot: Option<CpTreeSlot>,
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
        slot,
        cx,
    )
}

/// Shared collapsible fold header (issue #348 generated + #338 agent). `agent`
/// selects which expansion flag the click toggles. In tree view it is a
/// top-level, expandable `TreeItem` named without the disclosure arrow.
fn render_cp_fold_header(
    id: &'static str,
    section_label: &str,
    count: usize,
    expanded: bool,
    agent: bool,
    slot: Option<CpTreeSlot>,
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
        .map(|el| {
            cp_tree_item(
                el,
                slot,
                || format!("{section_label} ({count})"),
                Some(expanded),
                None,
            )
        })
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
    slot: Option<CpTreeSlot>,
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
            view,
            false,
            false,
            fi,
            SharedString::from(name),
            Some(&f.change),
            panel.is_conflicted(&f.path),
            panel.selected_file == Some(CommitPanelFileRef::Unstaged { index: fi }),
            wip_hit,
            view.window_active,
            0.0,
            panel.unstaged_stat(&f.path),
            is_convention(&f.path),
            slot,
            cx,
        )
        .into_any_element(),
    )
}

/// One staged file row (flat style) by index `fi` into `staged` (issue #348).
fn cp_staged_file_element(
    view: &CommitPanelView,
    fi: usize,
    slot: Option<CpTreeSlot>,
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
            view,
            true,
            false,
            fi,
            SharedString::from(name),
            Some(&f.change),
            false,
            panel.selected_file == Some(CommitPanelFileRef::Staged { index: fi }),
            wip_hit,
            view.window_active,
            0.0,
            panel.staged_stat(&f.path),
            is_convention(&f.path),
            slot,
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
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    if staged {
        cp_staged_file_element(view, fi, slot, cx)
    } else {
        cp_unstaged_file_element(view, fi, slot, cx)
    }
}

/// The two trailing fold regions of a section, after the base (normal/tree)
/// rows: first "Generated (N)" (issue #348), then "Agent artifacts (N)"
/// (issue #338). `j` is the display-row offset past the base region; `slot`
/// is the row's tree position in tree view.
fn render_cp_fold_regions(
    view: &CommitPanelView,
    j: usize,
    staged: bool,
    slot: Option<CpTreeSlot>,
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
                slot,
                cx,
            ));
        }
        let fi = *gen_files.get(j - 1)?;
        return cp_file_element(view, staged, fi, slot, cx);
    }
    // Agent-artifacts fold, immediately after the generated region.
    let k = j - gen_extra;
    if k == 0 {
        return Some(render_cp_agent_header(
            staged,
            art_files.len(),
            panel.agent_expanded,
            slot,
            cx,
        ));
    }
    let fi = *art_files.get(k - 1)?;
    cp_file_element(view, staged, fi, slot, cx)
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
        return cp_unstaged_file_element(view, normal[i], None, cx);
    }
    render_cp_fold_regions(view, i - normal.len(), false, None, cx)
}

/// PERF: build one unstaged tree row. `i` indexes the pruned tree first, then
/// the "Generated (N)" fold (issue #348). `slot` is its place in the
/// `cp-unstaged-tree` (#354).
pub(crate) fn render_unstaged_tree_row(
    view: &CommitPanelView,
    i: usize,
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.unstaged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, i - tree_len, false, slot, cx);
    }
    match panel.unstaged_tree.get(i)? {
        file_tree::TreeRow::Dir { depth, name } => {
            Some(render_cp_dir_row("cp-us-dir", *depth, name, slot))
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
                    view,
                    false,
                    true,
                    fi,
                    name.clone(),
                    change.as_ref(),
                    path.map(|p| panel.is_conflicted(p)).unwrap_or(false),
                    panel.selected_file == Some(CommitPanelFileRef::Unstaged { index: fi }),
                    wip_hit,
                    view.window_active,
                    (*depth as f32) * 12.0,
                    path.and_then(|p| panel.unstaged_stat(p)),
                    path.map(|p| is_convention(p)).unwrap_or(false),
                    slot,
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
        return cp_staged_file_element(view, normal[i], None, cx);
    }
    render_cp_fold_regions(view, i - normal.len(), true, None, cx)
}

/// PERF: build one staged tree row. `i` indexes the pruned tree first, then the
/// "Generated (N)" fold (issue #348). `slot` is its place in the
/// `cp-staged-tree` (#354).
pub(crate) fn render_staged_tree_row(
    view: &CommitPanelView,
    i: usize,
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.staged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, i - tree_len, true, slot, cx);
    }
    match panel.staged_tree.get(i)? {
        file_tree::TreeRow::Dir { depth, name } => {
            Some(render_cp_dir_row("cp-st-dir", *depth, name, slot))
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
                    view,
                    true,
                    true,
                    fi,
                    name.clone(),
                    change.as_ref(),
                    false,
                    panel.selected_file == Some(CommitPanelFileRef::Staged { index: fi }),
                    wip_hit,
                    view.window_active,
                    (*depth as f32) * 12.0,
                    path.and_then(|p| panel.staged_stat(p)),
                    path.map(|p| is_convention(p)).unwrap_or(false),
                    slot,
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
pub(super) fn render_coauthor_menu(
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
    /// Marshal a panel action to the parent on the next tick — a listener leases
    /// this entity, so a synchronous `KagiApp` call would re-enter and panic —
    /// carrying the frozen `owner` so a tab switch before the task runs drops it
    /// instead of acting on the panel now on screen (ADR-0197 決定 5 / #722 P1-c).
    fn defer_owned(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut KagiApp, crate::app::SessionId, &mut Window, &mut Context<KagiApp>)
            + 'static,
    ) {
        let weak_app = self.app.clone();
        let owner = self.owner;
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, window, cx| action(app, owner, window, cx));
        })
        .detach();
    }

    fn defer_stage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_stage_file(owner, fi, cx)
        });
    }

    fn defer_unstage_file(&self, fi: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.do_unstage_file(owner, fi, cx)
        });
    }

    pub(super) fn defer_stage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| app.do_stage_all(owner, cx));
    }

    pub(super) fn defer_unstage_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.do_unstage_all(owner, cx)
        });
    }

    fn defer_select_file(
        &self,
        file_ref: CommitPanelFileRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.defer_owned(window, cx, move |app, owner, _w, cx| {
            app.select_commit_panel_file(owner, file_ref, cx)
        });
    }

    pub(super) fn defer_open_commit_plan_modal(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.open_commit_plan_modal(owner, cx)
        });
    }

    pub(super) fn defer_open_discard_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.open_discard_all_modal(owner, cx)
        });
    }

    pub(crate) fn defer_open_file_menu(
        &self,
        fi: usize,
        pos: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Freeze both identities while the row listener still leases this
        // panel; the deferred callback must never resolve them from active UI.
        let Some(path) = self.state.unstaged.get(fi).map(|file| file.path.clone()) else {
            return;
        };
        let menu = file_menu::FileMenu {
            owner: self.owner,
            path,
            anchor: pos,
        };
        let panel_repo = self.repo_path.clone();
        let weak_app = self.app.clone();
        cx.spawn_in(window, async move |_v, acx| {
            let _ = weak_app.update_in(acx, |app, _window, cx| {
                app.open_file_menu(menu, &panel_repo, cx)
            });
        })
        .detach();
    }

    pub(super) fn defer_amend(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, _w, cx| {
            app.commit_panel_amend(owner, cx)
        });
    }

    pub(super) fn defer_smart_suggest(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, window, cx| {
            app.smart_suggest(owner, window, cx);
        });
    }

    pub(super) fn defer_smart_generate(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.defer_owned(window, cx, |app, owner, window, cx| {
            app.smart_generate(owner, window, cx)
        });
    }
}

// ──────────────────────────────────────────────────────────────
// Commit Panel renderer (T025) — now self-rendering on the entity
// ──────────────────────────────────────────────────────────────

impl Render for CommitPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_render(window, cx);
        let panel_width = self.panel_render_width;
        self.render_panel(panel_width, cx)
    }
}

impl CommitPanelView {
    pub(super) fn prepare_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window_activation_subscription.is_none() {
            self.window_activation_subscription =
                Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        self.window_active = window.is_window_active();
        #[cfg(feature = "gui-e2e")]
        {
            self.window_active = self.window_active_override.unwrap_or(self.window_active);
        }
        // Correction #1/#2: input sync + draft autosave run on the entity's own
        // render path (with `&mut Window`), never as a parent per-frame read of
        // the child's input.
        self.sync_inputs(window, cx);
        self.refresh_tree_layout();
    }

    /// Keep the accessible sibling positions in sync with the displayed tree.
    /// The virtual list can render many batches for one scroll; each batch
    /// only indexes the layout cached here.
    fn refresh_tree_layout(&mut self) {
        if !self.state.tree_view {
            return;
        }
        let key = (
            self.state.tree_revision,
            self.state.generated_expanded,
            self.state.agent_expanded,
        );
        if self.tree_layout_for == Some(key) {
            return;
        }
        self.unstaged_tree_layout = cp_tree_layout(&self.state, false);
        self.staged_tree_layout = cp_tree_layout(&self.state, true);
        self.tree_layout_for = Some(key);
    }

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
        let unstaged_header = self.render_unstaged_header(false, cx);
        let staged_header = self.render_staged_header(false, cx);

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
            // Header. #473: a worktree panel adds the worktree's chip, in the
            // WIP row's own lane colour, so it is obvious *which* working tree
            // the file list below belongs to.
            .child(self.render_panel_header())
            // File lists share the remaining height with the shrinking composer.
            .child(
                div()
                    .id("cp-files-container")
                    .flex_1()
                    // Keep both section headers while sharing space with editing.
                    .min_h(theme::scaled_px(80.))
                    .flex()
                    .flex_col()
                    .relative()
                    .child(super::e2e::measure_inside("cp-files"))
                    // Unstaged ヘッダ (固定)
                    .child(unstaged_header)
                    // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                    .child(self.render_unstaged_list(cx))
                    // Staged ヘッダ (固定)
                    .child(staged_header)
                    // Staged スクロールボックス — PERF: virtualized uniform_list.
                    .child(self.render_staged_list(cx)),
            )
            // Commit footer: subject + body, icon actions, commit button.
            // #476 slices 2–3: shown on a worktree panel too — every control in
            // it writes into that worktree.
            .child(self.render_composer(false, cx))
    }

    pub(super) fn render_panel_header(&self) -> gpui::Div {
        div()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(rgb(theme().surface))
            .text_sm()
            .text_color(rgb(theme().text_main))
            .child(SharedString::from("Commit Panel"))
            .when_some(self.foreign.clone(), |el, (label, color_idx)| {
                let color = theme().lane_color(color_idx);
                el.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .truncate()
                        .border_1()
                        .border_color(color)
                        .text_xs()
                        .text_color(color)
                        .child(SharedString::from(format!("🌲 {label}"))),
                )
            })
    }
}
