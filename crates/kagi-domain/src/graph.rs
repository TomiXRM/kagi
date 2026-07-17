//! Commit graph layout — pure Rust, no gpui / git2 dependency.
//!
//! # Algorithm overview (T006 contract, 5 steps)
//!
//! Input: `&[Commit]` in topological order (children before parents).
//! State: `active: Vec<Option<CommitId>>` — lane `i` holds the commit that
//! this lane is "waiting for" next.
//!
//! For each commit `C` (top-to-bottom, one row per commit):
//!
//! 1. **Find waiting lanes** — collect every lane index `i` where
//!    `active[i] == Some(C.id)`.
//!
//! 2. **Assign node lane** —
//!    - If no lane is waiting (branch tip / new root): take the leftmost
//!      `None` slot, or append a new lane if all slots are occupied.
//!    - If one or more lanes are waiting: `lane = min(waiting)`.
//!
//! 3. **Generate edges for the top half of this row** —
//!    - For each `j` in `waiting`: emit `(j → lane, IntoNode)`.
//!      If `j != lane`, free the slot (`active[j] = None`).
//!    - For every other occupied lane `k` (`active[k] = Some(_)` and
//!      `k ∉ waiting`): emit `(k → k, Pass)`.
//!
//! 4. **Place parents into `active` (bottom half)** —
//!    - No parents (root commit): `active[lane] = None`.
//!    - `parents[0]` (first parent):
//!      - *Exception*: if `parents[0]` is already waited on by lane `j`,
//!        emit `(lane → j, OutOfNode)` and `active[lane] = None` (prevents
//!        duplicate lane occupation).
//!      - Otherwise: `active[lane] = Some(parents[0])`, emit
//!        `(lane → lane, OutOfNode)`.
//!    - `parents[1..]` (merge parents):
//!      - If already waited on by lane `j`: emit `(lane → j, OutOfNode)`.
//!      - Otherwise: find leftmost `None` slot (or append), set
//!        `active[slot] = Some(p)`, emit `(lane → slot, OutOfNode)`.
//!
//! 5. **Trim trailing `None` lanes** — `lane_count` is the maximum number of
//!    occupied lanes seen across all rows (1-based).
//!
//! The 5 steps above describe the **`Stable`** ([`GraphLayoutMode::Stable`])
//! layout. [`GraphLayoutMode::Compact`] additionally *compacts* freed lanes
//! (Gitru-style swimlanes): columns shift left when a lane is reclaimed, which
//! is emitted as a `Pass` edge with `from_lane != to_lane`. Both modes carry a
//! **stable colour** on each lane ([`GraphRow::color`] / [`GraphEdge::color`]),
//! so a branch keeps its colour across rows and column shifts.
//!
//! Both modes also apply the step-4 *first-parent exception*: if `parents[0]`
//! is already awaited by another open lane, the node's line joins that lane at
//! the node's own row instead of opening a duplicate lane. Consequently open
//! lane targets are unique — any commit is approached by **at most one lane**,
//! and long branch lines never converge with a sideways bend at their target
//! commit's row.
//!
//! Public entry points: [`layout`] (Stable) and [`layout_with`].

use crate::commit::{Commit, CommitId};

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// Number of distinct lane colours the layout cycles through.
///
/// This is the modulus of the monotonic colour counter that gives every lane a
/// **stable** colour (see [`GraphRow::color`] / [`GraphEdge::color`]). It is the
/// domain-side palette size; the UI palette may be the same length or smaller
/// (the renderer takes `color % palette_len`).
///
/// 8 colours: enough to keep concurrently-visible lanes distinct without
/// over-cycling (lanes are short-lived, so same-colour coexistence is rare).
/// The UI palette in `src/ui/theme.rs` matches this length 1:1.
pub const NUM_COLORS: usize = 8;

/// Which lane-assignment strategy [`layout_with`] uses.
///
/// `Stable` reproduces the historical gitk-style layout (ADR-0003): lane columns
/// never shift — a freed lane becomes a gap that is reused leftmost-first.
///
/// `Compact` ports Gitru's VS Code-style swimlane **compaction**: freed lanes are
/// removed and the lanes to their right shift left, keeping the graph narrow.
/// Compaction can therefore move a lane between columns from one row to the next,
/// which is expressed as a [`EdgeKind::Pass`] edge with `from_lane != to_lane`
/// (a "shift" edge). Lane colours are carried on the lane, so a branch keeps its
/// colour even as its column shifts.
///
/// The two modes are intentionally separable: `Compact` is opt-in and the whole
/// path can be removed later by deleting the compaction branch + its tests, with
/// `Stable` (and the stable-colour work) untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphLayoutMode {
    /// Historical gitk-style layout — columns never shift. The default.
    #[default]
    Stable,
    /// Gitru-style swimlane compaction — freed lanes are reclaimed.
    Compact,
}

/// An open branch line ("swimlane"): the commit it is next waiting for plus the
/// stable colour carried with the lane across rows / column shifts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lane {
    /// The commit id this lane is currently waiting to reach.
    target: CommitId,
    /// Stable colour index assigned when the lane was born (see [`NUM_COLORS`]).
    color: usize,
}

/// The computed layout of an entire commit graph.
///
/// Each element of `rows` corresponds 1-to-1 with the input `commits` slice
/// (same index, same order).  `lane_count` is the maximum number of
/// simultaneously active lanes seen across all rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphLayout {
    /// One row per commit, in the same order as the input `commits` slice.
    pub rows: Vec<GraphRow>,
    /// Maximum simultaneous lane count (0 when `rows` is empty).
    pub lane_count: usize,
}

/// Layout information for a single commit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// The commit rendered on this row.
    pub commit: CommitId,
    /// The lane index on which this commit's node (●) is drawn.
    pub lane: usize,
    /// Stable colour index for this node's lane (`% NUM_COLORS`). Carried with
    /// the lane so a branch keeps its colour across rows / column shifts.
    pub color: usize,
    /// All edges that pass through (or originate / terminate at) this row.
    pub edges: Vec<GraphEdge>,
}

/// A single directed edge within one row.
///
/// Coordinates are in "lane space": `from_lane` is the lane at the **top**
/// of the row and `to_lane` is the lane at the **bottom** of the row.
/// Because every edge is contained within one row, no multi-row tracking
/// is needed by the rendering layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    /// Lane index at the top of this row.
    pub from_lane: usize,
    /// Lane index at the bottom of this row.
    pub to_lane: usize,
    /// Semantic kind of this edge.
    pub kind: EdgeKind,
    /// Stable colour index of the branch line this edge belongs to
    /// (`% NUM_COLORS`). The renderer paints the edge with this colour instead
    /// of deriving it from the column index, so colours stay branch-stable.
    pub color: usize,
}

/// The semantic role an edge plays within its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    /// The edge passes through this row without touching the node.
    ///
    /// In `Stable` mode `from_lane == to_lane` always (a straight vertical). In
    /// `Compact` mode a lane that shifts column between rows is also a `Pass`
    /// with `from_lane != to_lane` (a "shift" edge); the renderer draws it as a
    /// short S-curve from the top column to the bottom column.
    Pass,
    /// The edge comes **into** the node from a lane above
    /// (`to_lane == row.lane`).
    IntoNode,
    /// The edge leaves **out of** the node toward a parent below
    /// (`from_lane == row.lane`).
    OutOfNode,
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Compute the full [`GraphLayout`] for a topologically-sorted commit slice.
///
/// # Preconditions
///
/// `commits` must be in **topological order**: every child appears before all
/// of its parents (i.e., the output of `git log --topo-order`).  Violating
/// this ordering produces an unspecified (but safe) layout.
///
/// # Complexity
///
/// O(commits × active\_lanes).  For repositories with ≤ 10 000 commits and
/// typical lane counts (< 50) this runs in well under a millisecond.
pub fn layout(commits: &[Commit]) -> GraphLayout {
    layout_with(commits, GraphLayoutMode::Stable)
}

/// Compute the [`GraphLayout`] using the chosen [`GraphLayoutMode`].
///
/// `Stable` is the historical gitk-style layout (identical lane indices to the
/// pre-colour `layout`, now also carrying stable per-lane colours). `Compact`
/// applies Gitru-style swimlane compaction. See [`GraphLayoutMode`].
pub fn layout_with(commits: &[Commit], mode: GraphLayoutMode) -> GraphLayout {
    match mode {
        GraphLayoutMode::Stable => layout_stable(commits),
        GraphLayoutMode::Compact => layout_compact(commits),
    }
}

/// Allocate the next stable colour index (monotonic counter, `% NUM_COLORS`).
fn alloc_color(counter: &mut usize) -> usize {
    let c = *counter % NUM_COLORS;
    *counter += 1;
    c
}

// ────────────────────────────────────────────────────────────
// Stable (gitk-style) layout — columns never shift
// ────────────────────────────────────────────────────────────

fn layout_stable(commits: &[Commit]) -> GraphLayout {
    // Fast path: empty input.
    if commits.is_empty() {
        return GraphLayout {
            rows: Vec::new(),
            lane_count: 0,
        };
    }

    // `active[i]` = the lane occupying column `i`, or None if the column is
    // free. Each lane carries the commit it waits for plus its stable colour.
    let mut active: Vec<Option<Lane>> = Vec::new();
    let mut color_counter: usize = 0;

    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());
    let mut max_lanes: usize = 0;

    for commit in commits {
        // ── Step 1: find all lanes waiting for this commit ──────────────
        let waiting: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| match slot {
                Some(l) if l.target == commit.id => Some(i),
                _ => None,
            })
            .collect();

        // ── Step 2: assign the node lane + its (stable) colour ──────────
        let node_lane: usize = if waiting.is_empty() {
            // Branch tip or new root — take the leftmost free slot.
            find_or_push_free_lane(&mut active)
        } else {
            // At least one lane is waiting; use the leftmost.
            *waiting.iter().min().unwrap()
        };
        // The node's colour: an existing waiting lane keeps its colour (branch
        // continuity); a fresh tip mints a new colour that the first parent (or
        // the lone node) will carry.
        let node_color: usize = if waiting.is_empty() {
            alloc_color(&mut color_counter)
        } else {
            active[node_lane].as_ref().unwrap().color
        };

        // ── Step 3: generate top-half edges ─────────────────────────────
        let mut edges: Vec<GraphEdge> = Vec::new();

        // IntoNode edges for every waiting lane (each keeps its own colour).
        for &j in &waiting {
            let color = active[j].as_ref().unwrap().color;
            edges.push(GraphEdge {
                from_lane: j,
                to_lane: node_lane,
                kind: EdgeKind::IntoNode,
                color,
            });
            // Free non-primary waiting lanes (they've merged into node_lane).
            if j != node_lane {
                active[j] = None;
            }
        }

        // Pass edges for all other occupied lanes.
        for (k, slot) in active.iter().enumerate() {
            if let Some(l) = slot {
                if !waiting.contains(&k) {
                    edges.push(GraphEdge {
                        from_lane: k,
                        to_lane: k,
                        kind: EdgeKind::Pass,
                        color: l.color,
                    });
                }
            }
        }

        // ── Step 4: place parents into `active` (bottom-half edges) ─────
        if commit.parents.is_empty() {
            // Root commit — free its lane.
            active[node_lane] = None;
        } else {
            // --- first parent ---
            let p0 = &commit.parents[0];
            if let Some(existing_j) = find_lane_for(&active, p0) {
                // Exception: parents[0] is already waited on by another lane.
                // Merge this node's lane into that existing lane (keep its colour).
                let color = active[existing_j].as_ref().unwrap().color;
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: existing_j,
                    kind: EdgeKind::OutOfNode,
                    color,
                });
                active[node_lane] = None;
            } else {
                // Normal case: claim the node's lane for the first parent,
                // carrying the node's colour onward.
                active[node_lane] = Some(Lane {
                    target: p0.clone(),
                    color: node_color,
                });
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: node_lane,
                    kind: EdgeKind::OutOfNode,
                    color: node_color,
                });
            }

            // --- merge parents (parents[1..]) ---
            for p in &commit.parents[1..] {
                if let Some(j) = find_lane_for(&active, p) {
                    // Already waited on — just draw the edge in its colour.
                    let color = active[j].as_ref().unwrap().color;
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: j,
                        kind: EdgeKind::OutOfNode,
                        color,
                    });
                } else {
                    // Allocate a new (leftmost free) lane + a new colour.
                    let new_j = find_or_push_free_lane(&mut active);
                    let color = alloc_color(&mut color_counter);
                    active[new_j] = Some(Lane {
                        target: p.clone(),
                        color,
                    });
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: new_j,
                        kind: EdgeKind::OutOfNode,
                        color,
                    });
                }
            }
        }

        // ── Step 5: track maximum lane count ────────────────────────────
        let highest_used = active
            .iter()
            .enumerate()
            .filter_map(|(i, s)| if s.is_some() { Some(i + 1) } else { None })
            .max()
            .unwrap_or(0)
            .max(node_lane + 1);
        max_lanes = max_lanes.max(highest_used);

        debug_assert!(directed_edges_touch_node(&edges, node_lane));

        rows.push(GraphRow {
            commit: commit.id.clone(),
            lane: node_lane,
            color: node_color,
            edges,
        });
    }

    GraphLayout {
        rows,
        lane_count: max_lanes,
    }
}

// ────────────────────────────────────────────────────────────
// Compact (Gitru swimlane) layout — freed lanes are reclaimed
// ────────────────────────────────────────────────────────────

/// Gitru-style swimlane compaction. Lane positions are *packed* (no gaps):
/// `lanes[i]` is the branch line in column `i`. When a lane is consumed by a
/// merge (or a branch ends) the lanes to its right shift one column left, so the
/// graph never grows wider than it must. A column shift between rows is emitted
/// as a `Pass` edge with `from_lane != to_lane` (top column → bottom column).
fn layout_compact(commits: &[Commit]) -> GraphLayout {
    if commits.is_empty() {
        return GraphLayout {
            rows: Vec::new(),
            lane_count: 0,
        };
    }

    // Packed lane vector — position is the column, no `None` gaps.
    let mut lanes: Vec<Lane> = Vec::new();
    let mut color_counter: usize = 0;

    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());
    let mut max_lanes: usize = 0;

    for commit in commits {
        let input = lanes.clone();

        // Incoming lanes = input columns waiting for this commit.
        let incoming: Vec<usize> = input
            .iter()
            .enumerate()
            .filter_map(|(i, l)| if l.target == commit.id { Some(i) } else { None })
            .collect();
        let first_in = incoming.first().copied();
        let is_tip = incoming.is_empty();

        // Node colour: continue the first incoming lane's colour, or mint one.
        let node_color: usize = match first_in {
            Some(i) => input[i].color,
            None => alloc_color(&mut color_counter),
        };

        // First-parent join (Stable's step-4 exception): if another surviving
        // lane already waits for `parents[0]`, the node's line merges into that
        // lane at THIS row instead of opening a duplicate lane. Without this,
        // two lanes run in parallel toward the same commit and only converge —
        // with a sideways bend — at the target commit's own row.
        let p0_join: Option<usize> = commit.parents.first().and_then(|p0| {
            input
                .iter()
                .enumerate()
                .find(|(i, l)| !incoming.contains(i) && &l.target == p0)
                .map(|(i, _)| i)
        });

        // ── Build the output (next-row) lane vector, packed ─────────────
        // `old_to_new[i]` = output column of input lane i (None if consumed).
        let mut new_lanes: Vec<Lane> = Vec::new();
        let mut old_to_new: Vec<Option<usize>> = vec![None; input.len()];
        // Output column where the node's continuing (first-parent) line sits.
        let mut node_lane: usize = 0;

        for (i, lane) in input.iter().enumerate() {
            if first_in == Some(i) {
                // The node's own lane — continues as the first parent.
                node_lane = new_lanes.len();
                old_to_new[i] = Some(new_lanes.len());
                if !commit.parents.is_empty() && p0_join.is_none() {
                    new_lanes.push(Lane {
                        target: commit.parents[0].clone(),
                        color: node_color,
                    });
                }
                // Root (no parents) or first-parent join: the lane closes —
                // push nothing.
            } else if incoming.contains(&i) {
                // Secondary incoming lane — merged into the node, reclaimed.
            } else {
                // Unrelated lane — survives, possibly shifted left.
                old_to_new[i] = Some(new_lanes.len());
                new_lanes.push(lane.clone());
            }
        }

        // A tip opens a brand-new lane at the right for its first parent —
        // unless that parent is already awaited by an existing lane (join).
        if is_tip {
            node_lane = new_lanes.len();
            if !commit.parents.is_empty() && p0_join.is_none() {
                new_lanes.push(Lane {
                    target: commit.parents[0].clone(),
                    color: node_color,
                });
            }
            // Isolated commit (tip + root) or first-parent join: node occupies
            // a column but no lane persists — nothing pushed.
        }

        // Merge parents (parents[1..]): reuse an open lane or append a new one.
        let mut merge_targets: Vec<(usize, usize)> = Vec::new(); // (column, colour)
        for p in commit.parents.iter().skip(1) {
            if let Some(idx) = new_lanes.iter().position(|l| &l.target == p) {
                merge_targets.push((idx, new_lanes[idx].color));
            } else {
                let color = alloc_color(&mut color_counter);
                let idx = new_lanes.len();
                new_lanes.push(Lane {
                    target: p.clone(),
                    color,
                });
                merge_targets.push((idx, color));
            }
        }

        // ── Emit row-local edges (top column → bottom column) ───────────
        let mut edges: Vec<GraphEdge> = Vec::new();

        // Surviving lanes: Pass straight, or a shift if the column moved.
        for (i, lane) in input.iter().enumerate() {
            if incoming.contains(&i) {
                continue; // handled as IntoNode below
            }
            if let Some(no) = old_to_new[i] {
                edges.push(GraphEdge {
                    from_lane: i,
                    to_lane: no,
                    kind: EdgeKind::Pass,
                    color: lane.color,
                });
            }
        }

        // Incoming lanes converge into the node (each keeps its colour).
        for &j in &incoming {
            edges.push(GraphEdge {
                from_lane: j,
                to_lane: node_lane,
                kind: EdgeKind::IntoNode,
                color: input[j].color,
            });
        }

        // Outgoing first-parent + merge edges. On a join, the first-parent
        // edge bends into the existing lane's (possibly shifted) column and
        // takes that lane's colour, exactly like Stable's exception.
        if !commit.parents.is_empty() {
            let (p0_col, p0_color) = match p0_join {
                Some(k) => (
                    old_to_new[k].expect("join target lane survives this row"),
                    input[k].color,
                ),
                None => (node_lane, node_color),
            };
            edges.push(GraphEdge {
                from_lane: node_lane,
                to_lane: p0_col,
                kind: EdgeKind::OutOfNode,
                color: p0_color,
            });
            for (idx, color) in &merge_targets {
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: *idx,
                    kind: EdgeKind::OutOfNode,
                    color: *color,
                });
            }
        }

        let highest_used = input.len().max(new_lanes.len()).max(node_lane + 1).max(
            edges
                .iter()
                .map(|e| e.from_lane.max(e.to_lane) + 1)
                .max()
                .unwrap_or(0),
        );
        max_lanes = max_lanes.max(highest_used);

        debug_assert!(directed_edges_touch_node(&edges, node_lane));

        rows.push(GraphRow {
            commit: commit.id.clone(),
            lane: node_lane,
            color: node_color,
            edges,
        });

        lanes = new_lanes;
    }

    GraphLayout {
        rows,
        lane_count: max_lanes,
    }
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Invariant: when directed (IntoNode / OutOfNode) edges exist, the node's lane
/// must appear in at least one of them. A root branch-tip has no edges, which is
/// also valid.
fn directed_edges_touch_node(edges: &[GraphEdge], node_lane: usize) -> bool {
    let directed: Vec<_> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::IntoNode || e.kind == EdgeKind::OutOfNode)
        .collect();
    directed.is_empty()
        || directed
            .iter()
            .any(|e| e.from_lane == node_lane || e.to_lane == node_lane)
}

/// Return the index of the first `None` slot in `active`, extending the
/// vector by one entry if every slot is occupied.
fn find_or_push_free_lane(active: &mut Vec<Option<Lane>>) -> usize {
    if let Some(i) = active.iter().position(|s| s.is_none()) {
        i
    } else {
        let idx = active.len();
        active.push(None);
        idx
    }
}

/// Return the lane index that is currently waiting for `id`, or `None`.
fn find_lane_for(active: &[Option<Lane>], id: &CommitId) -> Option<usize> {
    active
        .iter()
        .position(|slot| matches!(slot, Some(l) if &l.target == id))
}

// ────────────────────────────────────────────────────────────
// Unit tests (sanity — exhaustive tests in T007)
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::Signature;

    // ── helpers ─────────────────────────────────────────────────────────

    fn sig() -> Signature {
        Signature {
            name: "Test".to_string(),
            email: "test@example.com".to_string(),
            time: 0,
        }
    }

    /// Build a minimal `Commit` from id string and parent id strings.
    fn c(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: CommitId(id.to_string()),
            parents: parents.iter().map(|p| CommitId(p.to_string())).collect(),
            author: sig(),
            committer: sig(),
            summary: format!("commit {}", id),
            message: format!("commit {}", id),
        }
    }

    fn cid(s: &str) -> CommitId {
        CommitId(s.to_string())
    }

    // ── test 1: empty input ──────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        let layout = layout(&[]);
        assert!(
            layout.rows.is_empty(),
            "empty input must produce empty rows"
        );
        assert_eq!(
            layout.lane_count, 0,
            "empty input must produce lane_count 0"
        );
    }

    // ── test 2: linear history (3 commits, single lane) ──────────────────
    //
    //  C  (tip)
    //  |
    //  B
    //  |
    //  A  (root)
    //
    // Topo order: [C, B, A]  (children first)

    #[test]
    fn test_linear_three_commits() {
        let commits = vec![c("C", &["B"]), c("B", &["A"]), c("A", &[])];
        let gl = layout(&commits);

        // All commits sit on lane 0.
        assert_eq!(gl.lane_count, 1);
        assert_eq!(gl.rows.len(), 3);
        for row in &gl.rows {
            assert_eq!(row.lane, 0, "linear history: all nodes on lane 0");
        }

        // No Pass edges expected in a straight line.
        for row in &gl.rows {
            for edge in &row.edges {
                assert_ne!(
                    edge.kind,
                    EdgeKind::Pass,
                    "linear history must have no Pass edges"
                );
            }
        }

        // Row C: one OutOfNode(0→0), no IntoNode (it's the tip).
        let row_c = &gl.rows[0];
        assert_eq!(row_c.commit, cid("C"));
        assert!(row_c
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 0));
        assert!(!row_c.edges.iter().any(|e| e.kind == EdgeKind::IntoNode));

        // Row B: IntoNode(0→0) from C, OutOfNode(0→0) to A.
        let row_b = &gl.rows[1];
        assert_eq!(row_b.commit, cid("B"));
        assert!(row_b
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::IntoNode && e.from_lane == 0 && e.to_lane == 0));
        assert!(row_b
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 0));

        // Row A (root): IntoNode(0→0), no OutOfNode.
        let row_a = &gl.rows[2];
        assert_eq!(row_a.commit, cid("A"));
        assert!(row_a.edges.iter().any(|e| e.kind == EdgeKind::IntoNode));
        assert!(!row_a.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode));
    }

    // ── test 3: branch + merge ───────────────────────────────────────────
    //
    //  M     (merge: parents = [B, D])    lane 0
    //  |\
    //  B |   (on main)                    lane 0
    //  | D   (on feat)                    lane 1
    //  |/
    //  A     (root, common ancestor)      lane 0
    //
    // Topo order: [M, B, D, A]

    #[test]
    fn test_branch_and_merge() {
        let commits = vec![
            c("M", &["B", "D"]), // merge commit
            c("B", &["A"]),      // main
            c("D", &["A"]),      // feat
            c("A", &[]),         // root
        ];
        let gl = layout(&commits);

        assert_eq!(gl.rows.len(), 4);

        // M is on lane 0 (first tip, no waiting lanes).
        let row_m = &gl.rows[0];
        assert_eq!(row_m.commit, cid("M"));
        assert_eq!(row_m.lane, 0);

        // After M: active = [Some(B), Some(D)]
        // B is waited on lane 0 → node lane 0.
        let row_b = &gl.rows[1];
        assert_eq!(row_b.commit, cid("B"));
        assert_eq!(row_b.lane, 0);

        // D is waited on lane 1 → node lane 1.
        let row_d = &gl.rows[2];
        assert_eq!(row_d.commit, cid("D"));
        assert_eq!(row_d.lane, 1);

        // A is waited on by both lanes (0 and 1) after B and D both point to A.
        // min(waiting) = 0.
        let row_a = &gl.rows[3];
        assert_eq!(row_a.commit, cid("A"));
        assert_eq!(row_a.lane, 0);

        // Lane count should be 2 (lanes 0 and 1 were simultaneously active).
        assert_eq!(gl.lane_count, 2);

        // Row A: only lane 0 was waiting, so IntoNode(0→0).
        assert!(
            row_a
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::IntoNode && e.from_lane == 0 && e.to_lane == 0),
            "A must have IntoNode 0→0 (lane 0 was the sole waiter)"
        );

        // Row D: parents[0]=A is already waited by lane 0 → step-4 exception.
        // D must emit OutOfNode(1→0) and must NOT keep lane 1 for A.
        let row_d_ref = &gl.rows[2];
        assert!(
            row_d_ref
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
            "D must OutOfNode 1→0 (first-parent exception: A already waited by lane 0)"
        );
    }

    // ── test 4: multiple roots (disconnected histories) ──────────────────
    //
    //  B  (tip of branch-2, no parents)   lane 1
    //  A  (tip of branch-1, no parents)   lane 0
    //
    // Both are root commits in topo order: [B, A]  (arbitrary ordering
    // since they are disconnected — B appears first in the slice).

    #[test]
    fn test_multiple_roots() {
        let commits = vec![
            c("B", &[]), // root tip 2 (first in slice)
            c("A", &[]), // root tip 1
        ];
        let gl = layout(&commits);

        assert_eq!(gl.rows.len(), 2);

        // B gets lane 0 (first commit, no waiting lanes, first free slot).
        let row_b = &gl.rows[0];
        assert_eq!(row_b.commit, cid("B"));
        assert_eq!(row_b.lane, 0);

        // B is a root — it frees lane 0.  A then also gets lane 0.
        let row_a = &gl.rows[1];
        assert_eq!(row_a.commit, cid("A"));
        assert_eq!(row_a.lane, 0);

        // lane_count is 1 because neither root had simultaneously active siblings.
        assert_eq!(gl.lane_count, 1);

        // Neither row has Pass or IntoNode edges (both are branch tips / roots).
        for row in &gl.rows {
            assert!(
                !row.edges.iter().any(|e| e.kind == EdgeKind::Pass),
                "root rows must not have Pass edges"
            );
            assert!(
                !row.edges.iter().any(|e| e.kind == EdgeKind::IntoNode),
                "root tips must not have IntoNode edges"
            );
            assert!(
                !row.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode),
                "root commits must not have OutOfNode edges"
            );
        }
    }

    // ── test 5: first-parent already-waiting exception (step 4 exception) ─
    //
    // This tests the critical edge case: two branches both converge on the
    // same parent A.
    //
    //   X  (tip of main, parent=A)   lane 0
    //   Y  (tip of feat, parent=A)   lane 1
    //   A  (root)                    lane 0
    //
    // Topo order: [X, Y, A]
    //
    // After X: active = [Some(A), None]   (lane 0 waiting for A)
    // Processing Y (tip, no waiting): lane 1 (first free).
    //   parents[0] = A, which is already waited on by lane 0.
    //   → exception fires: emit OutOfNode(1→0), active[1] = None.
    // After Y: active = [Some(A), None]  (unchanged)
    // Processing A: waiting = [0] → lane 0.

    #[test]
    fn test_first_parent_already_waiting_exception() {
        let commits = vec![c("X", &["A"]), c("Y", &["A"]), c("A", &[])];
        let gl = layout(&commits);

        assert_eq!(gl.rows.len(), 3);

        let row_x = &gl.rows[0];
        assert_eq!(row_x.lane, 0);

        let row_y = &gl.rows[1];
        assert_eq!(row_y.lane, 1);

        let row_a = &gl.rows[2];
        assert_eq!(row_a.lane, 0);

        // Y must emit OutOfNode from lane 1 to lane 0 (the exception).
        assert!(
            row_y
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
            "Y must OutOfNode 1→0 due to exception (parents[0]=A already waited by lane 0)"
        );

        // lane_count should be 2 (X and Y were simultaneously active).
        assert_eq!(gl.lane_count, 2);

        // Crucially, no lane_count explosion (lane_count must stay ≤ 2).
        assert!(
            gl.lane_count <= 2,
            "lane must not explode: got lane_count={}",
            gl.lane_count
        );
    }

    // ── test 6: stable colour is carried along a branch ──────────────────
    #[test]
    fn test_stable_color_carried_along_branch() {
        let commits = vec![c("C", &["B"]), c("B", &["A"]), c("A", &[])];
        let gl = layout_with(&commits, GraphLayoutMode::Stable);

        // One branch → one colour the whole way down.
        let color = gl.rows[0].color;
        for row in &gl.rows {
            assert_eq!(row.color, color, "linear branch keeps one stable colour");
            for e in &row.edges {
                assert_eq!(e.color, color, "edges on the branch share its colour");
            }
        }
    }

    // ── test 7: distinct branches get distinct colours ───────────────────
    #[test]
    fn test_stable_distinct_branch_colors() {
        // M(B,D), B(A), D(A), A()
        let commits = vec![
            c("M", &["B", "D"]),
            c("B", &["A"]),
            c("D", &["A"]),
            c("A", &[]),
        ];
        let gl = layout_with(&commits, GraphLayoutMode::Stable);

        // Mainline (M,B,A on lane 0) share colour 0; the feature lane (D) gets 1.
        assert_eq!(gl.rows[0].color, 0, "M mainline colour");
        assert_eq!(gl.rows[1].color, 0, "B mainline colour");
        assert_eq!(gl.rows[2].color, 1, "D feature lane colour differs");
        assert_eq!(gl.rows[3].color, 0, "A mainline colour");
    }

    // ── test 8: compact linear history ───────────────────────────────────
    #[test]
    fn test_compact_linear() {
        let commits = vec![c("C", &["B"]), c("B", &["A"]), c("A", &[])];
        let gl = layout_with(&commits, GraphLayoutMode::Compact);

        assert_eq!(gl.lane_count, 1);
        for row in &gl.rows {
            assert_eq!(row.lane, 0, "compact linear: all nodes on lane 0");
            assert_eq!(row.color, 0, "compact linear: one stable colour");
        }
    }

    // ── test 8b: compact — a commit is approached by at most one lane ────
    //
    // Repro of the "long line bends at the very end" artifact (stacked
    // branches merged into a mainline). Pre-fix, T3's first-parent
    // continuation opened a SECOND lane targeting T4 — duplicating the lane
    // already opened by P2's merge edge — so both lines ran in parallel and
    // only converged (with a sideways bend) at T4's own row. With the
    // first-parent join (Stable's step-4 exception ported to Compact), T3's
    // line joins the existing lane at T3's row instead.
    //
    //   P1 ─┬────────────── T1   (merge)
    //   P2 ─┼┬───────────── T4   (merge)
    //   P3 ─┼┼┬──────────── T5   (merge)
    //   T1..T2..T3 → T4 → T5 → P4 (branch chain)
    #[test]
    fn test_compact_first_parent_joins_existing_lane() {
        let commits = vec![
            c("P1", &["P2", "T1"]),
            c("P2", &["P3", "T4"]),
            c("P3", &["P4", "T5"]),
            c("T1", &["T2"]),
            c("T2", &["T3"]),
            c("T3", &["T4"]),
            c("T4", &["T5"]),
            c("T5", &["P4"]),
            c("P4", &[]),
        ];
        let gl = layout_with(&commits, GraphLayoutMode::Compact);

        // No duplicate lanes: every commit is reached by at most one lane,
        // so no row carries two IntoNode edges.
        for row in &gl.rows {
            let into = row
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::IntoNode)
                .count();
            assert!(
                into <= 1,
                "row {} has {} IntoNode edges (duplicate lanes waiting for it)",
                row.commit,
                into
            );
        }

        // T4 continues P2's merge line: same colour as the merge-out edge,
        // and the line arrives vertically (no bend at the target row).
        let row_p2 = gl.rows.iter().find(|r| r.commit == cid("P2")).unwrap();
        let merge_color = row_p2
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::OutOfNode && e.from_lane != e.to_lane)
            .expect("P2 must have a merge-parent edge")
            .color;
        let row_t4 = gl.rows.iter().find(|r| r.commit == cid("T4")).unwrap();
        assert_eq!(row_t4.color, merge_color, "T4 sits on P2's merge line");
        let into_t4 = row_t4
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::IntoNode)
            .expect("T4 has an incoming lane");
        assert_eq!(
            into_t4.from_lane, into_t4.to_lane,
            "the merge line must arrive at T4 vertically, not bend into it"
        );

        // T3 closes its lane by joining the merge line (join edge in the
        // existing lane's colour).
        let row_t3 = gl.rows.iter().find(|r| r.commit == cid("T3")).unwrap();
        assert!(
            row_t3
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::OutOfNode && e.color == merge_color),
            "T3 must join P2's merge lane at its own row"
        );
    }

    // ── test 8c: compact — a new tip joins an existing lane (no duplicate) ─
    #[test]
    fn test_compact_tip_joins_existing_lane() {
        // Y and X both point at A. X (the later tip) must not open a second
        // lane targeting A; it bends into Y's lane at its own row.
        let commits = vec![c("Y", &["A"]), c("X", &["A"]), c("A", &[])];
        let gl = layout_with(&commits, GraphLayoutMode::Compact);

        let row_x = &gl.rows[1];
        assert_eq!(row_x.lane, 1, "X sits on its own column");
        assert!(
            row_x
                .edges
                .iter()
                .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
            "X must join Y's lane (OutOfNode 1→0)"
        );

        let row_a = &gl.rows[2];
        assert_eq!(
            row_a
                .edges
                .iter()
                .filter(|e| e.kind == EdgeKind::IntoNode)
                .count(),
            1,
            "A must be reached by exactly one lane"
        );
        assert_eq!(gl.lane_count, 2);
    }

    // ── test 9: compact emits a shift edge + reclaims the column ──────────
    //
    //  M(A,B)  opens A@0, B@1
    //  A(C) → C(D) → D()   (mainline on lane 0, ending at root D)
    //  B()                  (the other branch, a root)
    //
    // When D (root) closes lane 0, the still-open B lane shifts from column 1
    // to column 0 — a `Pass` edge with from_lane=1, to_lane=0.
    #[test]
    fn test_compact_shift_edge_and_reclaim() {
        let commits = vec![
            c("M", &["A", "B"]),
            c("A", &["C"]),
            c("C", &["D"]),
            c("D", &[]),
            c("B", &[]),
        ];
        let gl = layout_with(&commits, GraphLayoutMode::Compact);

        // A shift edge (Pass with from != to) must appear somewhere.
        let has_shift = gl.rows.iter().any(|r| {
            r.edges
                .iter()
                .any(|e| e.kind == EdgeKind::Pass && e.from_lane != e.to_lane)
        });
        assert!(has_shift, "compaction must emit at least one shift edge");

        // The B branch keeps its colour across the shift.
        let row_d = gl.rows.iter().find(|r| r.commit == cid("D")).unwrap();
        let shift = row_d
            .edges
            .iter()
            .find(|e| e.kind == EdgeKind::Pass && e.from_lane != e.to_lane)
            .expect("D row carries the shift edge");
        assert_eq!(shift.from_lane, 1);
        assert_eq!(
            shift.to_lane, 0,
            "B reclaims column 0 after D closes lane 0"
        );
    }

    // ── test 10: compact never widens beyond stable on a typical graph ───
    #[test]
    fn test_compact_lane_count_not_worse() {
        let commits = vec![
            c("M", &["A", "B"]),
            c("A", &["C"]),
            c("C", &["D"]),
            c("D", &[]),
            c("B", &[]),
        ];
        let stable = layout_with(&commits, GraphLayoutMode::Stable);
        let compact = layout_with(&commits, GraphLayoutMode::Compact);
        assert_eq!(stable.rows.len(), compact.rows.len());
        assert!(
            compact.lane_count <= stable.lane_count,
            "compact lane_count {} must not exceed stable {}",
            compact.lane_count,
            stable.lane_count
        );
        // Row/commit correspondence preserved in both modes.
        for (i, row) in compact.rows.iter().enumerate() {
            assert_eq!(row.commit, commits[i].id);
        }
    }
}
