//! Commit-row badge column + WIP diffstat rendering, split out of
//! `render_helpers.rs` (T-SPLIT-HELPERS-001 / ADR-0116 Wave 3).
//! Behaviour-preserving move — no DOM/style/handler/[kagi]/i18n change.

use super::render_helpers::*;
use super::*;

/// Render the badge chips for one commit row as a horizontal flex container.
///
/// Badge labels are capped at 24 visible chars with a trailing `…` to prevent
/// very long branch names from overflowing the commit list row (T019).
/// Sort key for badge priority: HeadBranch=0, Branch=1, Tag=2, Remote=3.
/// Right-aligned layout means the last-rendered badge is closest to the graph,
/// so we want the most important badge last → highest priority rendered last.
/// We render in priority order (0→3) so HeadBranch ends up leftmost and
/// Remote rightmost within the 150px column (closest to the graph).
pub(crate) fn badge_priority(kind: &BadgeKind) -> u8 {
    match kind {
        BadgeKind::HeadBranch => 0,
        BadgeKind::Branch => 1,
        BadgeKind::Tag => 2,
        BadgeKind::Remote => 3,
    }
}

/// What clicking a WIP row does.
pub(crate) enum WipRowClick {
    /// Open the commit panel for the currently-open repo (stage/unstage).
    CommitPanel,
    /// Switch the open repo to this linked worktree so its changes can be acted
    /// on there (the open repo's WIP row, in turn, opens the commit panel).
    OpenWorktree(std::path::PathBuf),
}

pub(crate) fn render_wip_diffstat(stat: WipDiffStat) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .text_sm()
        .font_weight(gpui::FontWeight::BOLD)
        .child(
            div()
                .text_color(rgb(theme().change_added))
                .child(SharedString::from(format!("+{}", stat.additions))),
        )
        .child(
            div()
                .text_color(rgb(theme().change_deleted))
                .child(SharedString::from(format!("-{}", stat.deletions))),
        )
}

/// Render the badges column: user-resizable width (T030), **left-aligned**
/// (user request), `overflow_hidden`.  An empty badges list still occupies
/// the full width so that all rows share the same graph start position
/// (GitKraken layout, T021).  `badge_col_w` is the current column width.
pub(crate) fn render_badges_column(
    row_id: &CommitId,
    badges: &[commit_list::RefBadge],
    badge_col_w: f32,
    // When `Some`, draw a horizontal connector line filling the space between
    // the badges and the right edge of the column, so the badge→node line is
    // continuous *inside* the BRANCH/TAG pane (not stopping at the boundary).
    connector_color: Option<gpui::Hsla>,
    // Swimlane mode: when `Some`, every pill uses this lane colour (`0xRRGGBB`)
    // instead of its semantic HEAD/branch/remote/tag colour, so pills agree with
    // the graph line / node / band. `None` = classic semantic colours.
    lane_pill_color: Option<u32>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Content is built to fit rather than relying on clipping:
    //   - left-aligned, so the highest-priority chip (leftmost) is always
    //     fully visible and overflow happens rightward — the direction
    //     gpui's overflow_hidden actually clips,
    //   - the "+N" chip sits right after the primary chip so it can't be
    //     clipped,
    //   - the secondary chip flex-shrinks with an ellipsis; only its already
    //     ellipsized tail can ever be cut off.
    const MAX_BADGES: usize = 2;
    const MAX_BADGE_CHARS: usize = 20;

    let mut by_prio: Vec<&commit_list::RefBadge> = badges.iter().collect();
    by_prio.sort_by_key(|b| badge_priority(&b.kind));
    let extra = by_prio.len().saturating_sub(MAX_BADGES);
    let shown = &by_prio[..by_prio.len().min(MAX_BADGES)];

    let mut inner = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .gap_1()
        .overflow_hidden();

    // Badges in priority order: primary (HEAD/branch) leftmost.
    for (i, badge) in shown.iter().enumerate() {
        let target = row_id.clone();
        let color = match lane_pill_color {
            Some(c) => c,
            None => match badge.kind {
                BadgeKind::HeadBranch => theme().color_head,
                BadgeKind::Branch => theme().color_branch,
                BadgeKind::Remote => theme().color_remote,
                BadgeKind::Tag => theme().color_tag,
            },
        };
        // Char-truncate long labels.
        let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
            let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
            SharedString::from(format!("{}\u{2026}", s))
        } else {
            badge.label.clone()
        };
        let is_primary = i == 0;
        let (badge_bg, badge_border, badge_text) = theme::badge_style(color);
        let chip = div()
            // Stable element id so gpui interactivity (drag/drop) works. Keyed
            // by row position + badge label so a row with multiple branch chips
            // gets distinct ids (a commit can carry several branches).
            .id(SharedString::from(format!(
                "graph-badge-{i}-{}",
                badge.label
            )))
            .px_1()
            .rounded_sm()
            .bg(gpui::rgba(badge_bg))
            .border_1()
            .border_color(gpui::rgba(badge_border))
            .text_color(rgb(badge_text))
            .text_sm()
            .when(is_primary, |c| c.flex_shrink_0())
            // Secondary chips may shrink to fit; their text ellipsizes.
            .when(!is_primary, |c| c.min_w(px(20.)).truncate())
            .child(label);

        // T-DNDMERGE-001 / ADR-0079: wire drag/drop onto the chip based on kind.
        //   - `BadgeKind::Branch` / `BadgeKind::Remote` → INDEPENDENTLY draggable,
        //     carrying ITS OWN name (= the merge source) in `BranchDrag { name }`.
        //     For a remote chip the name is the full `remote/name` ref, so an
        //     upstream-only branch can be merged directly. Each visible chip
        //     carries its own name, so dragging a specific badge unambiguously
        //     selects that branch even when a commit has several. Tag chips are
        //     NOT draggable.
        //   - `BadgeKind::HeadBranch` (the current branch) → drop TARGET. It
        //     shows a valid-target highlight via `.drag_over::<BranchDrag>` and
        //     dispatches to `start_merge_from_drag` on drop. The drop is a
        //     TRIGGER only — it never calls git from the view (same as sidebar).
        let chip = match badge.kind {
            BadgeKind::Branch | BadgeKind::Remote => {
                if let Some(name) = draggable_branch_name(badge) {
                    chip.cursor_grab().on_drag(
                        BranchDrag { name: name.clone() },
                        move |drag: &BranchDrag, _pos, _window, cx| {
                            let name = SharedString::from(drag.name.clone());
                            cx.new(|_| BranchDragGhost { name })
                        },
                    )
                } else {
                    chip
                }
            }
            BadgeKind::HeadBranch => {
                let drop_handler = cx.listener(
                    move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                        this.start_merge_from_drag(payload.name.clone(), cx);
                        cx.notify();
                    },
                );
                chip.drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                    style
                        .bg(rgb(theme().selected))
                        .border_color(rgb(theme().color_branch))
                })
                .on_drop::<BranchDrag>(drop_handler)
            }
            BadgeKind::Tag => chip,
        };
        // Double-click a branch pill → switch. A local-branch pill checks out
        // the branch; a remote-branch pill switches to its latest (create/
        // fast-forward the tracking branch). A clean plan switches with no
        // popup; blockers/warnings open the relevant modal (see
        // `dblclick_checkout_branch` / `dblclick_switch_to_latest`). The
        // current-branch (HeadBranch) and tags are unaffected. Uses the full
        // `badge.label` (the displayed `label` may be truncated).
        let chip = match badge.kind {
            BadgeKind::Branch => {
                let dbl_branch = badge.label.to_string();
                chip.on_click(cx.listener(
                    move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.dblclick_checkout_branch(dbl_branch.clone(), cx);
                            cx.notify();
                        }
                    },
                ))
            }
            BadgeKind::Remote => {
                let dbl_remote = badge.label.to_string();
                chip.on_click(cx.listener(
                    move |this: &mut KagiApp, event: &gpui::ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.dblclick_switch_to_latest(dbl_remote.clone(), cx);
                            cx.notify();
                        }
                    },
                ))
            }
            BadgeKind::HeadBranch | BadgeKind::Tag => chip,
        };
        let chip = if let Some(branch_name) = context_branch_name(badge) {
            let badge_kind = badge.kind.clone();
            chip.on_mouse_down(
                MouseButton::Right,
                cx.listener(
                    move |this: &mut KagiApp, event: &gpui::MouseDownEvent, _window, cx| {
                        match badge_kind {
                            BadgeKind::HeadBranch | BadgeKind::Branch => {
                                this.open_local_branch_menu(branch_name.clone(), event.position);
                            }
                            BadgeKind::Remote => {
                                this.open_remote_branch_menu(
                                    branch_name.clone(),
                                    target.clone(),
                                    event.position,
                                );
                            }
                            BadgeKind::Tag => {}
                        }
                        cx.stop_propagation();
                        cx.notify();
                    },
                ),
            )
        } else {
            chip
        };
        inner = inner.child(chip);

        // "+N" chip directly after the primary chip (never clipped).
        // TODO(T-DNDMERGE-001): badges hidden behind the "+N" overflow are not
        // individually draggable yet (only the up-to-MAX_BADGES visible chips
        // are). Redesigning the overflow into a draggable popover is out of
        // scope for this lane.
        if is_primary && extra > 0 {
            inner = inner.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_color(rgb(theme().text_sub))
                    .text_sm()
                    .flex_shrink_0()
                    .child(SharedString::from(format!("+{extra}"))),
            );
        }
    }

    // User-resizable container (T030), overflow clipped so long badge lists don't push graph.
    div()
        .w(theme::scaled_px(badge_col_w))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .child(inner)
        // Connector line: fills the remaining width up to the column's right
        // edge so the line reaches into the BRANCH/TAG pane toward the badge.
        .when_some(connector_color, |el, color| {
            el.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(connector_line(color)),
            )
        })
}
