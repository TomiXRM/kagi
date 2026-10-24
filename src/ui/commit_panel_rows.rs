//! Virtualized Commit Panel file rows and accessible tree positions.
#![allow(clippy::too_many_arguments)]
use super::commit_panel_navigation::{row_name, ChangesRows};
use super::*;
use kagi_ui_core::tooltip::HoverTooltip as _;

// Uniform slots must not inherit the inline button's natural outer height.
pub(super) const FILE_ROW_HEIGHT: f32 = 22.;

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
    label: impl FnOnce() -> SharedString,
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

/// The slot of display row `index` in a section laid out by
/// [`super::commit_panel_files::cp_tree_layout`].
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
    drawn: &ChangesRows,
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
    let selected = (is_sel || wip_hit) && !is_conflicted;
    let selection_wash = if is_conflicted {
        0.
    } else {
        view.file_motion[staged as usize].selection(&file.path)
    };
    let row_bg: gpui::Hsla = if is_conflicted {
        rgb(theme().diff_removed_bg).into()
    } else {
        let base: gpui::Hsla = rgb(theme().panel).into();
        let mut wash = theme().selected_for_window(window_active);
        wash.a *= selection_wash;
        base.blend(wash)
    };
    let ink = diffstat_bar::file_selection_ink(selection_wash, window_active, row_bg);
    let foreground = |normal| ink.map_or_else(|| rgb(normal).into(), |(primary, _)| primary);
    let mut file_row = div()
        .id((row_id, fi))
        .map(|el| {
            cp_tree_item(
                el,
                slot,
                || row_name(view, staged, fi),
                None,
                Some(is_sel || wip_hit),
            )
        })
        .aria_description(full_path.clone())
        .w_full()
        .h(theme::scaled_px(FILE_ROW_HEIGHT))
        .min_h(px(20.))
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
        .py_0()
        .bg(row_bg)
        .relative()
        .child(super::e2e::measure_inside(if staged {
            "cp-selected-staged-row"
        } else {
            "cp-selected-unstaged-row"
        }))
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(format!(
                "cp-{}-density-file-{fi}",
                if staged { "st" } else { "us" }
            )))
        })
        .when(
            !is_sel && !wip_hit && !is_conflicted && selection_wash == 0.,
            |el| el.hover(|s| s.bg(rgb(theme().surface))),
        )
        .on_click(file_click)
        .child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .line_height(theme::scaled_px(16.))
                .text_color(foreground(badge_color))
                .child(SharedString::from(badge))
                .when(cfg!(feature = "gui-e2e"), |badge| {
                    badge.relative().child(super::e2e::measure_inside(format!(
                        "cp-{}-file-badge-{fi}",
                        if staged { "st" } else { "us" }
                    )))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .id((if staged { "cp-st-path" } else { "cp-us-path" }, fi))
                .aria_label(full_path.clone())
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .line_height(theme::scaled_px(16.))
                .when(cfg!(feature = "gui-e2e") && selected, |el| {
                    el.relative().child(super::e2e::measure_inside(if staged {
                        "cp-selected-staged-label"
                    } else {
                        "cp-selected-unstaged-label"
                    }))
                })
                .text_color(foreground(theme().text_main))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(super::e2e::measure_control(
                        format!("cp-{}-file-label-{fi}", if staged { "st" } else { "us" }),
                        name.clone(),
                    ))
                })
                .when(!cfg!(feature = "gui-e2e"), |el| el.child(name))
                .hover_tooltip_builder(
                    (
                        if staged {
                            "cp-st-path-help"
                        } else {
                            "cp-us-path-help"
                        },
                        fi,
                    ),
                    move |window, cx| {
                        super::render_helpers::subject_tooltip(full_path.clone(), window, cx)
                    },
                ),
        )
        .when(convention, |el| {
            // issue #338: emphasis badge on a convention-body file — these steer
            // agent behaviour, so they are highlighted, never folded.
            el.child(
                super::button_style::status_badge(
                    ("cp-convention-badge", fi),
                    "cp-row-convention",
                    Msg::AgentConventionBadge.t(),
                    theme().color_warning,
                    false,
                    cx,
                )
                .ml_1()
                .mr_2()
                .px(px(3.))
                .font_weight(gpui::FontWeight::BOLD),
            )
        })
        .child(diffstat_bar::diffstat_unit(
            if staged { fi + 100_000 } else { fi },
            stat,
            ink.map(|(_, secondary)| secondary),
        ));
    file_row = super::commit_panel_navigation::file_row(
        view,
        drawn,
        staged,
        fi,
        is_sel || wip_hit,
        file_row,
        cx,
    );
    if is_conflicted {
        // A conflict is a status, not an unavailable Stage action.
        return file_row.child(super::button_style::status_badge(
            (conflict_id, fi),
            "cp-row-conflict",
            Msg::CommitPanelConflict.t(),
            theme().color_blocker,
            true,
            cx,
        ));
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
    file_row.child(super::commit_panel_controls::inline_file_action(
        view, drawn, staged, fi, btn_id, cx,
    ))
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
                || Msg::A11yFileTreeDir.t().replacen("{}", name, 1).into(),
                None,
                None,
            )
        })
        .pl(theme::scaled_px(8.0 + (depth as f32) * 12.0))
        .h(theme::scaled_px(FILE_ROW_HEIGHT))
        .min_h(px(20.))
        .flex()
        .items_center()
        .py_0()
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(theme::scaled_px(16.))
        .text_color(rgb(theme().change_dir))
        .relative()
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(super::e2e::measure_inside(format!(
                "{prefix}-{}",
                name.as_ref()
            )))
        })
        .child(
            div()
                .relative()
                .child(name.clone())
                .when(cfg!(feature = "gui-e2e"), |el| {
                    el.child(super::e2e::measure_inside(format!(
                        "{prefix}-label-{}",
                        name.as_ref()
                    )))
                }),
        )
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
    let row = div()
        .id(id)
        .map(|el| {
            cp_tree_item(
                el,
                slot,
                || format!("{section_label} ({count})").into(),
                Some(expanded),
                None,
            )
        })
        .w_full()
        .h(theme::scaled_px(FILE_ROW_HEIGHT))
        .min_h(px(20.))
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_0()
        .hover(|s| s.bg(rgb(theme().surface)).cursor_default())
        .on_click(toggle)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .line_height(theme::scaled_px(16.))
                .text_color(rgb(theme().text_label))
                .truncate()
                .child(SharedString::from(label)),
        )
        .into_any_element();
    super::e2e::measure_leaf(id, row)
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
    drawn: &ChangesRows,
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
            drawn,
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
    drawn: &ChangesRows,
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
            drawn,
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
    drawn: &ChangesRows,
    staged: bool,
    fi: usize,
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    if staged {
        cp_staged_file_element(view, drawn, fi, slot, cx)
    } else {
        cp_unstaged_file_element(view, drawn, fi, slot, cx)
    }
}

/// The two trailing fold regions of a section, after the base (normal/tree)
/// rows: first "Generated (N)" (issue #348), then "Agent artifacts (N)"
/// (issue #338). `j` is the display-row offset past the base region; `slot`
/// is the row's tree position in tree view.
fn render_cp_fold_regions(
    view: &CommitPanelView,
    drawn: &ChangesRows,
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
        return cp_file_element(view, drawn, staged, fi, slot, cx);
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
    cp_file_element(view, drawn, staged, fi, slot, cx)
}

/// PERF: build one unstaged row in flat view. `i` is a display-row index:
/// normal files first, then the "Generated" / "Agent artifacts" folds.
pub(crate) fn render_unstaged_flat_row(
    view: &CommitPanelView,
    drawn: &ChangesRows,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let normal = &panel.unstaged_normal_files;
    if i < normal.len() {
        return cp_unstaged_file_element(view, drawn, normal[i], None, cx);
    }
    render_cp_fold_regions(view, drawn, i - normal.len(), false, None, cx)
}

/// PERF: build one unstaged tree row. `i` indexes the pruned tree first, then
/// the "Generated (N)" fold (issue #348). `slot` is its place in the
/// `cp-unstaged-tree` (#354).
pub(crate) fn render_unstaged_tree_row(
    view: &CommitPanelView,
    drawn: &ChangesRows,
    i: usize,
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.unstaged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, drawn, i - tree_len, false, slot, cx);
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
                    drawn,
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
    drawn: &ChangesRows,
    i: usize,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let normal = &panel.staged_normal_files;
    if i < normal.len() {
        return cp_staged_file_element(view, drawn, normal[i], None, cx);
    }
    render_cp_fold_regions(view, drawn, i - normal.len(), true, None, cx)
}

/// PERF: build one staged tree row. `i` indexes the pruned tree first, then the
/// "Generated (N)" fold (issue #348). `slot` is its place in the
/// `cp-staged-tree` (#354).
pub(crate) fn render_staged_tree_row(
    view: &CommitPanelView,
    drawn: &ChangesRows,
    i: usize,
    slot: Option<CpTreeSlot>,
    cx: &mut Context<CommitPanelView>,
) -> Option<gpui::AnyElement> {
    let panel = &view.state;
    let tree_len = panel.staged_tree.len();
    if i >= tree_len {
        return render_cp_fold_regions(view, drawn, i - tree_len, true, slot, cx);
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
                    drawn,
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
