//! WIP anchor paints through the production graph canvas (#767).
use super::{draw, GlobalSettings};
use crate::macos::unmount;
use gpui::{px, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::graph_view::{PaintedGraphDash, PaintedGraphNode};
use kagi::ui::{e2e, graph_view, graph_wip, KagiApp};
use kagi_ui_core::theme;
use std::path::Path;
// ─────────────────────────────────────────────────────────────────────────────
// Issue #767 — the WIP anchor node
//
// Every WIP row draws a HOLLOW ring in the column its next commit will land
// on (its worktree's HEAD lane), a dashed rail from its badge across to that
// ring, and the #472 dashed connector on down to the HEAD commit node.
//
// The oracle is the window's own paint trace (`graph_view::take_paint_trace`,
// `gui-e2e` only): the node/dash records are emitted by the real canvas paint
// closures, so this asserts what was painted rather than re-deriving lane
// geometry from `lane_center_x` and comparing the copy to itself. The only
// numbers taken from the view are *which* row a WIP anchors to and which lane
// colour that row carries — the join the feature is about.
// ─────────────────────────────────────────────────────────────────────────────

/// Two painted coordinates are the same column / row centre within this many
/// pixels: path endpoints are exact, but GPUI snaps canvas bounds to device
/// pixels, so one lane centre can land a fraction off between two rows.
const PAINT_EPS: f32 = 1.5;

/// One WIP row, as the real `TabViewState` describes it.
struct WipFact {
    /// Worktree path (or `current`) — for failure messages and row lookup.
    label: String,
    /// Row index of the commit this WIP's next commit will sit on, when that
    /// commit is in the loaded window at all.
    head_row: Option<usize>,
    /// The anchor the pipeline handed the renderer: `(lane, lane colour)`.
    anchor: Option<(usize, usize)>,
}

/// The slice of the view the paint assertions join against, read once a frame.
struct ViewFacts {
    /// `(lane, node colour index)` per commit row, in row order.
    rows: Vec<(usize, usize)>,
    /// WIP rows in draw order (open repo first, then dirty worktrees).
    wips: Vec<WipFact>,
}

fn view_facts(cx: &mut VisualTestAppContext, kagi: &gpui::Entity<KagiApp>) -> ViewFacts {
    cx.read(|app| {
        let view = kagi.read(app).view();
        let rows = view.rows.iter().map(|r| (r.lane, r.node_color)).collect();
        let wips = view
            .wip_lanes
            .iter()
            .map(|(target, anchor)| {
                let (label, head) = match target {
                    graph_wip::WipTarget::Current => (
                        "current".to_owned(),
                        view.rows.iter().find(|r| r.is_head).map(|r| r.id.clone()),
                    ),
                    graph_wip::WipTarget::Worktree(ix) => {
                        let wt = &view.worktrees[*ix];
                        (wt.path.display().to_string(), wt.head.clone())
                    }
                };
                WipFact {
                    label,
                    head_row: head.and_then(|id| view.commit_row_index.get(&id).copied()),
                    anchor: anchor.map(|a| (a.lane, a.color)),
                }
            })
            .collect();
        ViewFacts { rows, wips }
    })
}

/// Draw one frame and hand back only what that frame painted: the trace is
/// armed/drained first so a previous frame's records cannot answer for this one.
fn painted(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    dimensions: (f32, f32),
) -> (Vec<PaintedGraphNode>, Vec<PaintedGraphDash>) {
    graph_view::take_paint_trace(win.window_id());
    draw(cx, win, dimensions);
    let trace = graph_view::take_paint_trace(win.window_id());
    assert!(
        !trace.0.is_empty(),
        "the frame painted no graph nodes at all — the trace is not recording this window"
    );
    trace
}

fn hsla_eq(a: gpui::Hsla, b: gpui::Hsla) -> bool {
    (a.h - b.h).abs() <= 1e-3
        && (a.s - b.s).abs() <= 1e-3
        && (a.l - b.l).abs() <= 1e-3
        && (a.a - b.a).abs() <= 1e-3
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= PAINT_EPS
}

/// Can the segments be walked from `start` to `target` without a hole bigger
/// than `max_gap`? Segments are `(lo, hi)` on one axis; this is what makes a
/// row-by-row dashed run *a connector* rather than *some dashes*.
fn reaches(segments: &[(f32, f32)], start: f32, target: f32, max_gap: f32) -> bool {
    let mut sorted = segments.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cursor = start;
    for (lo, hi) in sorted {
        if hi <= cursor + 0.01 {
            continue; // behind the cursor: already covered
        }
        if lo > cursor + max_gap {
            break; // a hole no dash gap can explain
        }
        cursor = cursor.max(hi);
    }
    cursor + 0.5 >= target
}

/// Vertical dash segments painted in the column at `x`, in `color`.
fn vertical_dashes(dashes: &[PaintedGraphDash], x: f32, color: gpui::Hsla) -> Vec<(f32, f32)> {
    dashes
        .iter()
        .filter(|d| {
            near(f32::from(d.from.x), f32::from(d.to.x))
                && near(f32::from(d.from.x), x)
                && hsla_eq(d.color, color)
        })
        .map(|d| {
            let (a, b) = (f32::from(d.from.y), f32::from(d.to.y));
            (a.min(b), a.max(b))
        })
        .collect()
}

/// Horizontal dash segments painted across row centre `y`, in `color`.
fn horizontal_dashes(dashes: &[PaintedGraphDash], y: f32, color: gpui::Hsla) -> Vec<(f32, f32)> {
    dashes
        .iter()
        .filter(|d| {
            near(f32::from(d.from.y), f32::from(d.to.y))
                && near(f32::from(d.from.y), y)
                && hsla_eq(d.color, color)
        })
        .map(|d| {
            let (a, b) = (f32::from(d.from.x), f32::from(d.to.x));
            (a.min(b), a.max(b))
        })
        .collect()
}

/// Check every painted hollow anchor against the row it claims to anchor to.
/// Returns their centres so callers can compare frames (zoom, hover, selection)
/// without re-deriving any geometry.
fn check_anchors(
    facts: &ViewFacts,
    nodes: &[PaintedGraphNode],
    dashes: &[PaintedGraphDash],
    label: &str,
) -> Vec<(f32, f32)> {
    let mut hollow: Vec<&PaintedGraphNode> = nodes.iter().filter(|n| n.hollow).collect();
    let mut solid: Vec<&PaintedGraphNode> = nodes.iter().filter(|n| !n.hollow).collect();
    hollow.sort_by(|a, b| a.center.y.partial_cmp(&b.center.y).expect("finite y"));
    solid.sort_by(|a, b| a.center.y.partial_cmp(&b.center.y).expect("finite y"));

    let anchored: Vec<&WipFact> = facts.wips.iter().filter(|w| w.anchor.is_some()).collect();
    assert_eq!(
        hollow.len(),
        anchored.len(),
        "{label}: {} WIP rows have an anchor, but {} hollow nodes were painted — \
         a row without an anchor must paint nothing (never a lane-0 node)",
        anchored.len(),
        hollow.len(),
    );
    assert!(
        solid.len() >= 2,
        "{label}: the fixture must paint at least two commit rows, got {}",
        solid.len()
    );
    assert!(
        solid.len() <= facts.rows.len(),
        "{label}: {} painted commit nodes for {} view rows",
        solid.len(),
        facts.rows.len()
    );
    if let (Some(last_wip), Some(first_commit)) = (hollow.last(), solid.first()) {
        assert!(
            f32::from(last_wip.center.y) < f32::from(first_commit.center.y),
            "{label}: the WIP anchors must sit above the commit list, got {:?} vs {:?}",
            last_wip.center,
            first_commit.center
        );
    }
    // Allow the measured hollow-ring cutout, but not a missing row of dashes.
    let pitch = f32::from(solid[1].center.y) - f32::from(solid[0].center.y);
    assert!(
        pitch > 1.,
        "{label}: painted rows overlap ({pitch} px pitch)"
    );
    let max_gap = hollow
        .iter()
        // Ring diameter + 2px stroke + the ordinary 3px inter-dash gap.
        .map(|node| 2.0 * node.radius + theme::scaled(5.0))
        .fold(pitch * 0.5, f32::max)
        + PAINT_EPS;
    assert!(
        max_gap < pitch,
        "a missing whole row must fail the dash-chain oracle"
    );
    let visible_commit_radius = solid.iter().map(|node| node.radius).fold(0.0, f32::max);

    for (fact, node) in anchored.iter().zip(&hollow) {
        let what = format!("{label}/{}", fact.label);
        let (lane, color_idx) = fact.anchor.expect("filtered to anchored rows");
        let head_row = fact
            .head_row
            .unwrap_or_else(|| panic!("{what}: anchored to a HEAD that is not a loaded row"));
        assert!(
            head_row < solid.len(),
            "{what}: HEAD row {head_row} was not painted ({} rows on screen)",
            solid.len()
        );
        let head_node = solid[head_row];
        let (head_lane, head_color) = facts.rows[head_row];
        let expected = theme::theme().lane_color(head_color);

        // The anchor is the HEAD's colour — not the worktree's position.
        assert_eq!(
            color_idx, head_color,
            "{what}: anchor colour must be HEAD row {head_row}'s lane colour"
        );
        assert!(
            hsla_eq(head_node.color, expected),
            "{what}: painted row {head_row} is {:?}, the view says {expected:?} — the \
             painted-row/view-row join is off",
            head_node.color
        );
        assert!(
            hsla_eq(node.color, expected),
            "{what}: hollow ring painted {:?}, HEAD's lane colour is {expected:?}",
            node.color
        );
        assert!(
            node.radius >= visible_commit_radius,
            "{what}: virtual node is smaller than the visible commit node: {} < {}",
            node.radius,
            visible_commit_radius
        );
        if !theme::graph_lane_compact() {
            assert!(
                (node.radius - visible_commit_radius).abs() < 0.1,
                "{what}: the hollow ring must match the painted HEAD ring's radius"
            );
        }

        // ── node → HEAD: dashed, down this anchor's own column ──────────────
        let column = vertical_dashes(dashes, f32::from(node.center.x), expected);
        assert!(
            !column.is_empty(),
            "{what}: no dashed connector under the hollow node at {:?}",
            node.center
        );
        let mut unique = column.clone();
        unique.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        unique.dedup_by(|a, b| near(a.0, b.0) && near(a.1, b.1));
        assert_eq!(
            unique.len(),
            column.len(),
            "{what}: the same dash segment is painted twice — one WIP column is one trace"
        );
        // Same lane as HEAD: the dashes run all the way into the node. A
        // dedicated lane keeps #472's solid landing arc, so there the dashes
        // only have to reach the HEAD row's top edge.
        let target = if lane == head_lane {
            f32::from(head_node.center.y)
        } else {
            f32::from(head_node.center.y) - pitch / 2.
        };
        assert!(
            reaches(&column, f32::from(node.center.y), target, max_gap),
            "{what}: the dashed connector does not run from the hollow node \
             ({:?}) down to HEAD row {head_row} ({:?}); segments {column:?}",
            node.center,
            head_node.center,
        );

        // ── badge → node: dashed, across this row ───────────────────────────
        let rail = horizontal_dashes(dashes, f32::from(node.center.y), expected);
        assert!(
            !rail.is_empty(),
            "{what}: no dashed badge→node rail at the hollow node's row"
        );
        let left = rail.iter().map(|s| s.0).fold(f32::INFINITY, f32::min);
        assert!(
            left < f32::from(node.bounds.left()) - theme::scaled(4.0),
            "{what}: the badge rail starts at x={left}, not in the badge column \
             before the divider and graph canvas {:?}",
            node.bounds,
        );
        let right = rail.iter().map(|s| s.1).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            reaches(&rail, left, right, theme::scaled(3.0) + PAINT_EPS),
            "{what}: the badge rail is broken between x={left} and x={right}: {rail:?}"
        );
        // It ends AT the ring — just outside its stroke, never through it.
        assert!(
            right >= f32::from(node.center.x) - node.radius - theme::scaled(4.0) - PAINT_EPS
                && right <= f32::from(node.center.x) - node.radius + PAINT_EPS,
            "{what}: the badge rail ends at x={right}, not at the ring {:?} (r={})",
            node.center,
            node.radius
        );
    }

    hollow
        .iter()
        .map(|n| (f32::from(n.center.x), f32::from(n.center.y)))
        .collect()
}

/// Issue #767 (Tier A): three dirty working trees on three different branch
/// HEADs paint three hollow anchor nodes, in three different columns, each in
/// its own HEAD's lane colour, each joined to its badge by a dashed rail and to
/// its HEAD commit node by a dashed connector that survives the WIP rows in
/// between.
///
/// Re-checked at 1.25× zoom (the geometry moves, the relationships must not),
/// with the pointer hovering a WIP row, and with a WIP row's commit panel open
/// — the ring must never fill in nor lose its lane colour (#767 review point).
pub fn scenario_commit_row_layout_wip(cx: &mut VisualTestAppContext, repo_path: &Path) {
    let restore = GlobalSettings::capture();
    theme::set_zoom(1.);
    const DIMENSIONS: (f32, f32) = (1440., 900.);
    let (kagi, win) = crate::macos::mount(cx, repo_path);

    let facts = view_facts(cx, &kagi);
    assert_eq!(
        facts.wips.len(),
        3,
        "fixture: three dirty working trees → three WIP rows"
    );
    assert!(
        facts.wips.iter().all(|w| w.anchor.is_some()),
        "every WIP row's HEAD is loaded here, so every row must get an anchor"
    );
    let heads: Vec<Option<usize>> = facts.wips.iter().map(|w| w.head_row).collect();
    assert_eq!(
        heads.iter().collect::<std::collections::HashSet<_>>().len(),
        3,
        "fixture: the three WIP rows must sit on three different commits, got {heads:?}"
    );

    let (nodes, dashes) = painted(cx, win, DIMENSIONS);
    let centers = check_anchors(&facts, &nodes, &dashes, "base");
    let radius = nodes
        .iter()
        .find(|n| n.hollow)
        .expect("a hollow node")
        .radius;
    // Three columns, not three rings stacked in one: any two rings are further
    // apart than a ring is wide.
    for (i, a) in centers.iter().enumerate() {
        for b in &centers[i + 1..] {
            assert!(
                (a.0 - b.0).abs() > 2. * radius,
                "three WIP rows on three HEADs must use three columns, got {centers:?}"
            );
        }
    }

    // ── zoom ────────────────────────────────────────────────────────────────
    theme::set_zoom(1.25);
    let (zoom_nodes, zoom_dashes) = painted(cx, win, DIMENSIONS);
    let zoom_centers = check_anchors(&facts, &zoom_nodes, &zoom_dashes, "zoom-1.25");
    let zoom_radius = zoom_nodes
        .iter()
        .find(|n| n.hollow)
        .expect("a hollow node at 1.25x")
        .radius;
    assert!(
        (zoom_radius / radius - 1.25).abs() <= 0.05,
        "the ring must scale with zoom: {radius} -> {zoom_radius}"
    );
    let spread = |c: &[(f32, f32)]| (c.last().unwrap().0 - c.first().unwrap().0).abs();
    assert!(
        spread(&zoom_centers) > spread(&centers),
        "lane pitch must grow with zoom: {centers:?} -> {zoom_centers:?}"
    );
    theme::set_zoom(1.);

    // ── hover ───────────────────────────────────────────────────────────────
    // The WIP row's own hover style repaints the row; the ring is not part of
    // that hover state and must come back identical.
    let hover_at = gpui::point(px(centers[0].0), px(centers[0].1));
    cx.simulate_mouse_move(win, hover_at, None, gpui::Modifiers::none());
    cx.run_until_parked();
    let (hover_nodes, hover_dashes) = painted(cx, win, DIMENSIONS);
    let hover_centers = check_anchors(&view_facts(cx, &kagi), &hover_nodes, &hover_dashes, "hover");
    assert_eq!(
        hover_centers, centers,
        "hovering a WIP row moved its anchor node"
    );

    // ── selection ───────────────────────────────────────────────────────────
    // Opening a worktree's commit panel is what paints a WIP row selected.
    let selected_wt = cx.read(|app| {
        kagi.read(app)
            .view()
            .worktrees
            .iter()
            .position(|w| !w.is_current && w.wip.is_some_and(|wip| wip.is_dirty()))
            .expect("a dirty linked worktree")
    });
    let (wt_path, wt_name) = cx.read(|app| {
        let wt = &kagi.read(app).view().worktrees[selected_wt];
        (wt.path.clone(), wt.name.clone())
    });
    kagi.update(cx, |app, cx| {
        e2e::open_worktree_panel_no_inputs(app, wt_path, &wt_name, selected_wt, cx)
    });
    cx.run_until_parked();
    let (sel_nodes, sel_dashes) = painted(cx, win, DIMENSIONS);
    let sel_centers = check_anchors(&view_facts(cx, &kagi), &sel_nodes, &sel_dashes, "selected");
    let column_of = |c: &[(f32, f32)]| c.iter().map(|p| p.0).collect::<Vec<_>>();
    // Columns, not full centres: the open panel may take vertical space, but it
    // must not move a WIP anchor off its lane or fill its ring.
    assert_eq!(
        column_of(&sel_centers),
        column_of(&centers),
        "opening a WIP row's commit panel moved its anchor off its lane"
    );
    assert_eq!(
        sel_nodes.iter().filter(|n| n.hollow).count(),
        centers.len(),
        "a selected WIP row must keep a HOLLOW ring, not a filled node"
    );

    unmount(cx, kagi, win);
    drop(restore);
    eprintln!(
        "[gui-e2e] PASS commit_row_layout_wip 3 hollow anchors at painted x {:?} \
         (zoomed {:?}), dashed badge→node→HEAD, unchanged under hover/selection",
        centers.iter().map(|c| c.0).collect::<Vec<_>>(),
        zoom_centers.iter().map(|c| c.0).collect::<Vec<_>>(),
    );
}

/// Issue #767, the three cases the happy path cannot show: two dirty trees on
/// the SAME HEAD (one shared column, one trace, both rings in HEAD's colour),
/// a DETACHED worktree (its HEAD resolves to an OID, so it still anchors), and
/// an ORPHAN worktree whose HEAD is unborn (no anchor at all — and, the point
/// of the case, no invented lane-0 ring).
pub fn scenario_commit_row_layout_wip_edges(
    cx: &mut VisualTestAppContext,
    repo_path: &Path,
    shared_wt: &Path,
    detached_wt: &Path,
    orphan_wt: &Path,
) {
    let restore = GlobalSettings::capture();
    theme::set_zoom(1.);
    const DIMENSIONS: (f32, f32) = (1440., 900.);
    let (kagi, win) = crate::macos::mount(cx, repo_path);

    let facts = view_facts(cx, &kagi);
    let find = |path: &Path| -> &WipFact {
        facts
            .wips
            .iter()
            .find(|w| Path::new(&w.label) == path)
            .unwrap_or_else(|| {
                panic!(
                    "no WIP row for {}; rows were {:?}",
                    path.display(),
                    facts.wips.iter().map(|w| &w.label).collect::<Vec<_>>()
                )
            })
    };
    let current = facts
        .wips
        .iter()
        .find(|w| w.label == "current")
        .expect("the open repo's own WIP row");
    let shared = find(shared_wt);
    let detached = find(detached_wt);
    let orphan = find(orphan_wt);

    // Shared HEAD: same row, one lane, one colour — two rings, one trace.
    assert_eq!(
        current.head_row, shared.head_row,
        "fixture: the linked worktree must sit on the open repo's HEAD commit"
    );
    assert_eq!(
        current.anchor, shared.anchor,
        "two WIP rows on one HEAD share one lane and one colour"
    );
    // A detached HEAD still resolves to a commit, so it still anchors.
    let detached_head = detached
        .head_row
        .expect("a detached worktree's HEAD resolves to an OID");
    assert!(
        detached.anchor.is_some(),
        "a detached worktree's WIP row must still anchor"
    );
    assert_eq!(
        detached.anchor.map(|(_, c)| c),
        Some(facts.rows[detached_head].1),
        "the detached anchor carries its own HEAD row's lane colour"
    );
    // Unborn HEAD: nothing to anchor to, so nothing is drawn.
    assert_eq!(
        orphan.head_row, None,
        "an orphan worktree's HEAD is unborn — it has no row"
    );
    assert_eq!(
        orphan.anchor, None,
        "an unborn HEAD must not be given a lane"
    );

    let (nodes, dashes) = painted(cx, win, DIMENSIONS);
    // `check_anchors` counts hollow nodes against anchored rows: the orphan row
    // painting a lane-0 ring would fail here, as would a missing shared ring.
    let centers = check_anchors(&facts, &nodes, &dashes, "wip-edges");
    assert_eq!(
        centers.len(),
        facts.wips.len() - 1,
        "three of the four WIP rows anchor: {centers:?}"
    );

    // Which painted ring belongs to which row: `check_anchors` returns them in
    // the anchored rows' own order, and the worktree order is the snapshot's,
    // not the fixture's argument order.
    let ring = |want: &WipFact| -> (f32, f32) {
        let ix = facts
            .wips
            .iter()
            .filter(|w| w.anchor.is_some())
            .position(|w| w.label == want.label)
            .expect("an anchored row has a ring");
        centers[ix]
    };
    let (current_ring, shared_ring, detached_ring) = (ring(current), ring(shared), ring(detached));
    let top = if current_ring.1 <= shared_ring.1 {
        current_ring
    } else {
        shared_ring
    };

    // The two rings on the shared HEAD are vertically aligned in one column…
    let shared_x = current_ring.0;
    assert!(
        near(shared_ring.0, shared_x),
        "the shared-HEAD rings must line up in one column, got {current_ring:?} / {shared_ring:?}"
    );
    assert!(
        (current_ring.1 - shared_ring.1).abs() > 1.,
        "the shared-HEAD rings are separate rows, not one node drawn twice: \
         {current_ring:?} / {shared_ring:?}"
    );
    // …and that column carries exactly one trace: it starts at the TOP ring, so
    // nothing is painted above it.
    let shared_row = current.head_row.expect("the open repo's HEAD row");
    let shared_color = theme::theme().lane_color(facts.rows[shared_row].1);
    let column = vertical_dashes(&dashes, shared_x, shared_color);
    let above = column
        .iter()
        .filter(|(lo, _)| *lo < top.1 - PAINT_EPS)
        .count();
    assert_eq!(
        above, 0,
        "the shared column's trace must begin at the topmost WIP ring, \
         but {above} dash segments were painted above it: {column:?}"
    );
    // The detached ring is its own column.
    assert!(
        (detached_ring.0 - shared_x).abs() > 1.,
        "the detached worktree anchors on another commit, so another column: \
         {detached_ring:?} vs shared column x={shared_x}"
    );

    unmount(cx, kagi, win);
    drop(restore);
    eprintln!(
        "[gui-e2e] PASS commit_row_layout_wip_edges shared column x={shared_x} (2 rings, \
         1 trace), detached anchored, unborn drew nothing: {centers:?}"
    );
}
