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
//! Public entry point: [`layout`].

use crate::commit::{Commit, CommitId};

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

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
}

/// The semantic role an edge plays within its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    /// The edge passes straight through this row without touching the node
    /// (`from_lane == to_lane`).
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
    // Fast path: empty input.
    if commits.is_empty() {
        return GraphLayout { rows: Vec::new(), lane_count: 0 };
    }

    // `active[i]` = the CommitId that lane i is currently waiting for, or
    // None if the lane is free.
    let mut active: Vec<Option<CommitId>> = Vec::new();

    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());
    let mut max_lanes: usize = 0;

    for commit in commits {
        // ── Step 1: find all lanes waiting for this commit ──────────────
        let waiting: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| {
                if slot.as_ref() == Some(&commit.id) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // ── Step 2: assign the node lane ────────────────────────────────
        let node_lane: usize = if waiting.is_empty() {
            // Branch tip or new root — take the leftmost free slot.
            find_or_push_free_lane(&mut active)
        } else {
            // At least one lane is waiting; use the leftmost.
            *waiting.iter().min().unwrap()
        };

        // ── Step 3: generate top-half edges ─────────────────────────────
        let mut edges: Vec<GraphEdge> = Vec::new();

        // IntoNode edges for every waiting lane.
        for &j in &waiting {
            edges.push(GraphEdge {
                from_lane: j,
                to_lane: node_lane,
                kind: EdgeKind::IntoNode,
            });
            // Free non-primary waiting lanes (they've merged into node_lane).
            if j != node_lane {
                active[j] = None;
            }
        }

        // Pass edges for all other occupied lanes.
        for (k, slot) in active.iter().enumerate() {
            if slot.is_some() && !waiting.contains(&k) {
                edges.push(GraphEdge {
                    from_lane: k,
                    to_lane: k,
                    kind: EdgeKind::Pass,
                });
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
                // Merge this node's lane into that existing lane.
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: existing_j,
                    kind: EdgeKind::OutOfNode,
                });
                active[node_lane] = None;
            } else {
                // Normal case: claim the node's lane for the first parent.
                active[node_lane] = Some(p0.clone());
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: node_lane,
                    kind: EdgeKind::OutOfNode,
                });
            }

            // --- merge parents (parents[1..]) ---
            for p in &commit.parents[1..] {
                if let Some(j) = find_lane_for(&active, p) {
                    // Already waited on — just draw the edge.
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: j,
                        kind: EdgeKind::OutOfNode,
                    });
                } else {
                    // Allocate a new (leftmost free) lane for this parent.
                    let new_j = find_or_push_free_lane(&mut active);
                    active[new_j] = Some(p.clone());
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: new_j,
                        kind: EdgeKind::OutOfNode,
                    });
                }
            }
        }

        // ── Step 5: track maximum lane count ────────────────────────────
        // Highest occupied lane index + 1, also counting the node lane
        // itself (it may have just been freed).
        let highest_used = active
            .iter()
            .enumerate()
            .filter_map(|(i, s)| if s.is_some() { Some(i + 1) } else { None })
            .max()
            .unwrap_or(0)
            .max(node_lane + 1);
        max_lanes = max_lanes.max(highest_used);

        // ── Invariant check ─────────────────────────────────────────────
        // When there are IntoNode or OutOfNode edges, at least one of them
        // must involve the node's lane.  A root branch-tip has no edges at
        // all, which is also valid.
        debug_assert!(
            {
                let directed: Vec<_> = edges.iter()
                    .filter(|e| e.kind == EdgeKind::IntoNode || e.kind == EdgeKind::OutOfNode)
                    .collect();
                directed.is_empty()
                    || directed.iter().any(|e| e.from_lane == node_lane || e.to_lane == node_lane)
            },
            "when directed edges exist, node lane must appear in at least one of them"
        );

        rows.push(GraphRow { commit: commit.id.clone(), lane: node_lane, edges });
    }

    // Final invariant: empty input → empty rows (already handled by fast path)
    debug_assert!(!commits.is_empty() || rows.is_empty());

    GraphLayout { rows, lane_count: max_lanes }
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Return the index of the first `None` slot in `active`, extending the
/// vector by one entry if every slot is occupied.
fn find_or_push_free_lane(active: &mut Vec<Option<CommitId>>) -> usize {
    if let Some(i) = active.iter().position(|s| s.is_none()) {
        i
    } else {
        let idx = active.len();
        active.push(None);
        idx
    }
}

/// Return the lane index that is currently waiting for `id`, or `None`.
fn find_lane_for(active: &[Option<CommitId>], id: &CommitId) -> Option<usize> {
    active
        .iter()
        .position(|slot| slot.as_ref() == Some(id))
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
        assert!(layout.rows.is_empty(), "empty input must produce empty rows");
        assert_eq!(layout.lane_count, 0, "empty input must produce lane_count 0");
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
        let commits = vec![
            c("C", &["B"]),
            c("B", &["A"]),
            c("A", &[]),
        ];
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
                    edge.kind, EdgeKind::Pass,
                    "linear history must have no Pass edges"
                );
            }
        }

        // Row C: one OutOfNode(0→0), no IntoNode (it's the tip).
        let row_c = &gl.rows[0];
        assert_eq!(row_c.commit, cid("C"));
        assert!(row_c.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 0));
        assert!(!row_c.edges.iter().any(|e| e.kind == EdgeKind::IntoNode));

        // Row B: IntoNode(0→0) from C, OutOfNode(0→0) to A.
        let row_b = &gl.rows[1];
        assert_eq!(row_b.commit, cid("B"));
        assert!(row_b.edges.iter().any(|e| e.kind == EdgeKind::IntoNode && e.from_lane == 0 && e.to_lane == 0));
        assert!(row_b.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 0));

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
            row_a.edges.iter().any(|e| e.kind == EdgeKind::IntoNode && e.from_lane == 0 && e.to_lane == 0),
            "A must have IntoNode 0→0 (lane 0 was the sole waiter)"
        );

        // Row D: parents[0]=A is already waited by lane 0 → step-4 exception.
        // D must emit OutOfNode(1→0) and must NOT keep lane 1 for A.
        let row_d_ref = &gl.rows[2];
        assert!(
            row_d_ref.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
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
        let commits = vec![
            c("X", &["A"]),
            c("Y", &["A"]),
            c("A", &[]),
        ];
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
            row_y.edges.iter().any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
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
}
