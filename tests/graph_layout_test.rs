//! Exhaustive graph layout tests — T007.
//!
//! Topology coverage + invariant checker for `kagi::graph::layout`.
//! All writes during end-to-end tests are confined to a temporary directory.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::graph::{layout, layout_with, EdgeKind, GraphLayout, GraphLayoutMode};
use kagi_git::{commit_log, Commit, CommitId, Signature};

// ────────────────────────────────────────────────────────────
// Test helpers — commit construction
// ────────────────────────────────────────────────────────────

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

// ────────────────────────────────────────────────────────────
// Invariant checker (5 items per T007 spec)
// ────────────────────────────────────────────────────────────

/// Check all five layout invariants against the given commits and layout.
///
/// Panics with a descriptive message on the first violation.
fn check_invariants(commits: &[Commit], gl: &GraphLayout) {
    // ── Invariant 1: rows.len() == commits.len(), rows[i].commit == commits[i].id ──
    assert_eq!(
        gl.rows.len(),
        commits.len(),
        "invariant 1 violated: rows.len() ({}) != commits.len() ({})",
        gl.rows.len(),
        commits.len()
    );
    for (i, (row, commit)) in gl.rows.iter().zip(commits.iter()).enumerate() {
        assert_eq!(
            row.commit, commit.id,
            "invariant 1 violated at row {}: row.commit ({}) != commits[{}].id ({})",
            i, row.commit, i, commit.id
        );
    }

    // ── Invariant 2: edge kinds must be consistent with row.lane ──
    for (i, row) in gl.rows.iter().enumerate() {
        for edge in &row.edges {
            match edge.kind {
                EdgeKind::IntoNode => {
                    assert_eq!(
                        edge.to_lane, row.lane,
                        "invariant 2 violated at row {} (commit {}): IntoNode.to_lane ({}) != row.lane ({})",
                        i, row.commit, edge.to_lane, row.lane
                    );
                }
                EdgeKind::OutOfNode => {
                    assert_eq!(
                        edge.from_lane, row.lane,
                        "invariant 2 violated at row {} (commit {}): OutOfNode.from_lane ({}) != row.lane ({})",
                        i, row.commit, edge.from_lane, row.lane
                    );
                }
                EdgeKind::Pass => {
                    assert_eq!(
                        edge.from_lane, edge.to_lane,
                        "invariant 2 violated at row {} (commit {}): Pass.from_lane ({}) != Pass.to_lane ({})",
                        i, row.commit, edge.from_lane, edge.to_lane
                    );
                    assert_ne!(
                        edge.from_lane, row.lane,
                        "invariant 2 violated at row {} (commit {}): Pass edge is on the node lane ({})",
                        i, row.commit, row.lane
                    );
                }
            }
        }
    }

    // ── Invariant 3: edge continuity between adjacent rows ──
    // For each pair (r, r+1):
    //   "bottom-open lanes of r" = Pass∪OutOfNode with to_lane
    //   "top-open lanes of r+1"  = Pass∪IntoNode with from_lane
    // These sets must be equal.
    for i in 0..gl.rows.len().saturating_sub(1) {
        let row_r = &gl.rows[i];
        let row_next = &gl.rows[i + 1];

        // Lanes open at the bottom of row r (what continues downward).
        let bottom_open: HashSet<usize> = row_r
            .edges
            .iter()
            .filter_map(|e| match e.kind {
                EdgeKind::Pass => Some(e.to_lane),
                EdgeKind::OutOfNode => Some(e.to_lane),
                EdgeKind::IntoNode => None,
            })
            .collect();

        // Lanes open at the top of row r+1 (what comes from above).
        let top_open: HashSet<usize> = row_next
            .edges
            .iter()
            .filter_map(|e| match e.kind {
                EdgeKind::Pass => Some(e.from_lane),
                EdgeKind::IntoNode => Some(e.from_lane),
                EdgeKind::OutOfNode => None,
            })
            .collect();

        assert_eq!(
            bottom_open,
            top_open,
            "invariant 3 violated between rows {} ({}) and {} ({}): \
             bottom-open lanes {:?} != top-open lanes {:?}",
            i,
            row_r.commit,
            i + 1,
            row_next.commit,
            bottom_open,
            top_open
        );
    }

    // ── Invariant 4: lane_count consistency ──
    // lane_count >= max used lane index + 1, and all edge lanes < lane_count.
    let max_used_lane = gl
        .rows
        .iter()
        .flat_map(|row| {
            let mut lanes = vec![row.lane];
            for edge in &row.edges {
                lanes.push(edge.from_lane);
                lanes.push(edge.to_lane);
            }
            lanes
        })
        .max();

    if let Some(max) = max_used_lane {
        assert!(
            gl.lane_count > max,
            "invariant 4 violated: lane_count ({}) < max_used_lane + 1 ({})",
            gl.lane_count,
            max + 1
        );
    }

    for (i, row) in gl.rows.iter().enumerate() {
        assert!(
            row.lane < gl.lane_count,
            "invariant 4 violated at row {} (commit {}): row.lane ({}) >= lane_count ({})",
            i,
            row.commit,
            row.lane,
            gl.lane_count
        );
        for edge in &row.edges {
            assert!(
                edge.from_lane < gl.lane_count,
                "invariant 4 violated at row {}: edge.from_lane ({}) >= lane_count ({})",
                i,
                edge.from_lane,
                gl.lane_count
            );
            assert!(
                edge.to_lane < gl.lane_count,
                "invariant 4 violated at row {}: edge.to_lane ({}) >= lane_count ({})",
                i,
                edge.to_lane,
                gl.lane_count
            );
        }
    }

    // ── Invariant 5: no duplicate (from, to, kind) edges in the same row ──
    for (i, row) in gl.rows.iter().enumerate() {
        let mut seen: HashSet<(usize, usize, &str)> = HashSet::new();
        for edge in &row.edges {
            let kind_str = match edge.kind {
                EdgeKind::Pass => "pass",
                EdgeKind::IntoNode => "into",
                EdgeKind::OutOfNode => "out",
            };
            let key = (edge.from_lane, edge.to_lane, kind_str);
            assert!(
                seen.insert(key),
                "invariant 5 violated at row {} (commit {}): duplicate edge ({:?} → {:?}, {:?})",
                i,
                row.commit,
                edge.from_lane,
                edge.to_lane,
                edge.kind
            );
        }
    }
}

// ────────────────────────────────────────────────────────────
// Case 1: linear 10 commits — checker self-verification first
//
//   J → I → H → G → F → E → D → C → B → A  (root)
// All on lane 0, no Pass edges.
// ────────────────────────────────────────────────────────────

#[test]
fn test_linear_10() {
    let commits = vec![
        c("J", &["I"]),
        c("I", &["H"]),
        c("H", &["G"]),
        c("G", &["F"]),
        c("F", &["E"]),
        c("E", &["D"]),
        c("D", &["C"]),
        c("C", &["B"]),
        c("B", &["A"]),
        c("A", &[]),
    ];
    let gl = layout(&commits);

    // Check invariants first — this validates the checker itself on a known-good case.
    check_invariants(&commits, &gl);

    // All nodes on lane 0.
    for (i, row) in gl.rows.iter().enumerate() {
        assert_eq!(row.lane, 0, "row {}: expected lane 0, got {}", i, row.lane);
    }

    // No Pass edges in a straight line.
    for row in &gl.rows {
        for edge in &row.edges {
            assert_ne!(
                edge.kind,
                EdgeKind::Pass,
                "linear 10: unexpected Pass edge in row for {}",
                row.commit
            );
        }
    }

    assert_eq!(gl.lane_count, 1, "linear 10: expected lane_count == 1");
    assert_eq!(gl.rows.len(), 10, "linear 10: expected 10 rows");
}

// ────────────────────────────────────────────────────────────
// Case 2: branch + merge (T006 fixture equivalent)
//
//   M  (merge: parents=[B, D])   lane 0
//   |\
//   B |  (main)                  lane 0
//   | D  (feat)                  lane 1
//   |/
//   A  (root)                    lane 0
//
// Topo order: [M, B, D, A]
// ────────────────────────────────────────────────────────────

#[test]
fn test_branch_and_merge() {
    let commits = vec![
        c("M", &["B", "D"]),
        c("B", &["A"]),
        c("D", &["A"]),
        c("A", &[]),
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 4);
    assert_eq!(gl.rows[0].commit, cid("M"));
    assert_eq!(gl.rows[0].lane, 0);
    assert_eq!(gl.rows[1].commit, cid("B"));
    assert_eq!(gl.rows[1].lane, 0);
    assert_eq!(gl.rows[2].commit, cid("D"));
    assert_eq!(gl.rows[2].lane, 1);
    assert_eq!(gl.rows[3].commit, cid("A"));
    assert_eq!(gl.rows[3].lane, 0);
    assert_eq!(gl.lane_count, 2);

    // M must have OutOfNode to both B (lane 0) and D (lane 1).
    let row_m = &gl.rows[0];
    assert!(
        row_m
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 0),
        "M: expected OutOfNode 0→0 for first parent B"
    );
    assert!(
        row_m
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 0 && e.to_lane == 1),
        "M: expected OutOfNode 0→1 for second parent D"
    );

    // D: parents[0]=A is already waited by lane 0 → OutOfNode 1→0.
    let row_d = &gl.rows[2];
    assert!(
        row_d
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::OutOfNode && e.from_lane == 1 && e.to_lane == 0),
        "D: expected OutOfNode 1→0 (A already waited by lane 0)"
    );
}

// ────────────────────────────────────────────────────────────
// Case 3: octopus merge (3 parents)
//
//   O  (octopus merge: parents=[A, B, C])  lane 0
//   |\\
//   A | |  lane 0
//   | B |  lane 1
//   | | C  lane 2
//   | | |
//   R  (common root, all three branches converge)
//
// For simplicity: each branch tip is an independent root.
// Topo order: [O, A, B, C]  (A is p[0], B is p[1], C is p[2])
// ────────────────────────────────────────────────────────────

#[test]
fn test_octopus_merge_3parents() {
    let commits = vec![
        c("O", &["A", "B", "C"]),
        c("A", &[]),
        c("B", &[]),
        c("C", &[]),
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 4);

    // O is on lane 0 (first tip).
    let row_o = &gl.rows[0];
    assert_eq!(row_o.lane, 0, "octopus: O must be on lane 0");

    // O must emit 3 OutOfNode edges.
    let out_edges: Vec<_> = row_o
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::OutOfNode)
        .collect();
    assert_eq!(
        out_edges.len(),
        3,
        "octopus: O must have exactly 3 OutOfNode edges, got {}",
        out_edges.len()
    );

    // lane_count must accommodate 3 simultaneously active lanes.
    assert!(
        gl.lane_count >= 3,
        "octopus: lane_count must be >= 3, got {}",
        gl.lane_count
    );
    // Should not explode unreasonably.
    assert!(
        gl.lane_count <= 4,
        "octopus: lane_count should stay <= 4, got {}",
        gl.lane_count
    );

    // A is waited on by lane 0 (first parent).
    assert_eq!(gl.rows[1].commit, cid("A"));
    assert_eq!(gl.rows[1].lane, 0);
}

// ────────────────────────────────────────────────────────────
// Case 4: criss-cross merge
//
// Classic criss-cross: two branches each merge from the other.
//
//   A1  A2          (two independent tips, roots)
//    \ /
//     X  (merge: parents=[A1, A2])
//    / \
//     Y  (merge: parents=[A2, A1] — opposite order)
//
// Actual topo-order construction:
//   Y  (parents=[X1, X2])
//   X1 (parents=[A1, A2])
//   X2 (parents=[A2, A1])
//   A1 (root)
//   A2 (root)
//
// X and Y are 4-way ancestors forming a diamond.
// For this test we just check invariants; the exact shape is implementation-specific.
// ────────────────────────────────────────────────────────────

#[test]
fn test_criss_cross_merge() {
    // Construct: two commits A and B, two merges M1 and M2 each merging both.
    //   M2 (parents=[M1, B])  — latest
    //   M1 (parents=[A, B])
    //   A  (root)
    //   B  (root)
    let commits = vec![
        c("M2", &["M1", "B"]),
        c("M1", &["A", "B"]),
        c("A", &[]),
        c("B", &[]),
    ];
    let gl = layout(&commits);

    // Invariant check only — no shape assertions (implementation-dependent).
    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 4);
    assert!(gl.lane_count >= 1, "criss-cross: lane_count must be >= 1");
}

// ────────────────────────────────────────────────────────────
// Case 5: parallel long branch (5-row concurrent run)
//
// Two branches run in parallel for 5 commits, sharing only the root R.
//
//   main branch: M5 → M4 → M3 → M2 → M1 → R
//   feat branch: F5 → F4 → F3 → F2 → F1 → R
//
// Topo order: [M5, F5, M4, F4, M3, F3, M2, F2, M1, F1, R]
// (interleaved, children before parents)
//
// During rows M4..M1 / F4..F1, both branches are active simultaneously
// → Pass edges must appear in the concurrent rows.
// ────────────────────────────────────────────────────────────

#[test]
fn test_parallel_long_branch_pass_edges() {
    let commits = vec![
        c("M5", &["M4"]),
        c("F5", &["F4"]),
        c("M4", &["M3"]),
        c("F4", &["F3"]),
        c("M3", &["M2"]),
        c("F3", &["F2"]),
        c("M2", &["M1"]),
        c("F2", &["F1"]),
        c("M1", &["R"]),
        c("F1", &["R"]),
        c("R", &[]),
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 11);

    // After M5 and F5 are both processed, two lanes are active.
    // The concurrent inner rows (indices 2..=9) should all have at least one Pass edge.
    // Rows 2..=7 (M4,F4,M3,F3,M2,F2) — both branches active.
    for i in 2..=7 {
        let row = &gl.rows[i];
        let has_pass = row.edges.iter().any(|e| e.kind == EdgeKind::Pass);
        assert!(
            has_pass,
            "parallel branch: row {} (commit {}) expected at least one Pass edge",
            i, row.commit
        );
    }

    assert!(
        gl.lane_count >= 2,
        "parallel branch: lane_count must be >= 2"
    );
}

// ────────────────────────────────────────────────────────────
// Case 6: multiple roots + lane reuse
//
// Two independent root branches. After both are exhausted, a new tip
// must reuse the freed lane (leftmost free = lane 0).
//
//   R1  (root, lane 0 - freed after processing)
//   R2  (root, lane 0 - reused because R1 freed it)
//   T   (tip of a new branch, lane 0 again)
//   P   (parent of T, lane 0)
// ────────────────────────────────────────────────────────────

#[test]
fn test_multiple_roots_lane_reuse() {
    // Two disconnected roots followed by a small linear chain.
    // Topo order: [T, P, R1, R2]
    // T and P are on their own chain; R1 and R2 are orphans.
    let commits = vec![
        c("T", &["P"]),
        c("P", &[]),  // root — frees its lane immediately after
        c("R1", &[]), // orphan root — takes leftmost free lane
        c("R2", &[]), // orphan root — takes leftmost free lane
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 4);

    // P is a root — after processing, its lane becomes free.
    // R1 and R2 should both reuse lane 0.
    assert_eq!(gl.rows[2].commit, cid("R1"));
    assert_eq!(gl.rows[2].lane, 0, "R1 must reuse lane 0 (leftmost free)");
    assert_eq!(gl.rows[3].commit, cid("R2"));
    assert_eq!(gl.rows[3].lane, 0, "R2 must reuse lane 0 after R1 frees it");
}

// ────────────────────────────────────────────────────────────
// Case 7: lane release after merge + new branch tip reuses lane
//
//   M  (merge: parents=[B, D])  — consumes lane 1 for D
//   B  (main, parent=A)          lane 0
//   D  (feat, parent=A)          lane 1 — freed after D processes
//   A  (root)                    lane 0
//   X  (new tip after A is gone) — lane 1 should be reused
//
// Topo order: [M, B, D, A, X]
// After A (root), active = [].  X is a new branch tip that should
// take the leftmost free slot (lane 0, since everything is freed).
// Then Y (X's parent) takes lane 0 as well.
// ────────────────────────────────────────────────────────────

#[test]
fn test_lane_reuse_after_merge() {
    // Build: merge M consumes lanes 0 and 1 temporarily,
    // then both are freed. New tip X should land on lane 0.
    let commits = vec![
        c("M", &["B", "D"]),
        c("B", &["A"]),
        c("D", &["A"]),
        c("A", &[]),    // root — after this, active is empty
        c("X", &["Y"]), // new tip: should get lane 0 (leftmost free)
        c("Y", &[]),    // root of X's chain
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 6);

    // After A (index 3) is processed, its lane is freed.
    // X (index 4) should reuse lane 0.
    let row_x = &gl.rows[4];
    assert_eq!(row_x.commit, cid("X"));
    assert_eq!(
        row_x.lane, 0,
        "X must reuse lane 0 (find_or_push_free_lane leftmost)"
    );
}

// ────────────────────────────────────────────────────────────
// Case 8: single commit (root only)
// ────────────────────────────────────────────────────────────

#[test]
fn test_single_commit() {
    let commits = vec![c("A", &[])];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 1, "single commit: 1 row expected");
    assert_eq!(gl.rows[0].lane, 0, "single commit: on lane 0");
    assert_eq!(gl.lane_count, 1, "single commit: lane_count == 1");
    assert!(
        gl.rows[0].edges.is_empty(),
        "single commit: no edges expected, got {:?}",
        gl.rows[0].edges
    );
}

// ────────────────────────────────────────────────────────────
// Case 9: stress — three-level nested branch+merge (12-15 commits)
//
// Structure (children-first):
//
//  LEVEL 3 merge:  M3 (parents=[L3B, L3F])
//  L3B → L2B → ... inner merges ...
//  L3F → L2F → ... inner merges ...
//
// We build:
//   M3  (top merge of feature3 and branch3)
//   B3  (branch3 child)        [main line]
//   F3  (feature3 child)
//   M2a (merge: branch2a and feature2a, parent of B3)
//   B2a (branch2a child)
//   F2a (feature2a child)
//   M2b (merge: branch2b and feature2b, parent of F3)
//   B2b
//   F2b
//   R2a (root: parent of B2a and F2a)
//   R2b (root: parent of B2b and F2b)
//   R   (common root of B3 and F3's chains)
//
// Topology:
//   M3  parents=[B3, F3]
//   B3  parents=[M2a]
//   F3  parents=[M2b]
//   M2a parents=[B2a, F2a]
//   B2a parents=[R2a]
//   F2a parents=[R2a]
//   M2b parents=[B2b, F2b]
//   B2b parents=[R2b]
//   F2b parents=[R2b]
//   R2a root
//   R2b root
// ────────────────────────────────────────────────────────────

#[test]
fn test_stress_three_level_nested() {
    let commits = vec![
        c("M3", &["B3", "F3"]),
        c("B3", &["M2a"]),
        c("F3", &["M2b"]),
        c("M2a", &["B2a", "F2a"]),
        c("B2a", &["R2a"]),
        c("F2a", &["R2a"]),
        c("M2b", &["B2b", "F2b"]),
        c("B2b", &["R2b"]),
        c("F2b", &["R2b"]),
        c("R2a", &[]),
        c("R2b", &[]),
    ];
    let gl = layout(&commits);

    // Invariant-only check — no shape assertions.
    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 11);
    assert!(gl.lane_count >= 1, "stress: lane_count must be >= 1");
    // Sanity: lane count should not explode beyond the number of leaves.
    assert!(
        gl.lane_count <= commits.len(),
        "stress: lane_count ({}) should not exceed commit count ({})",
        gl.lane_count,
        commits.len()
    );
}

// ────────────────────────────────────────────────────────────
// Case 10: end-to-end via git CLI + git2
//
// Builds a real repository in a TempDir using git CLI, then runs
// open → commit_log → layout → check_invariants.
//
// Topology built:
//   A (initial commit)
//   B (main: child of A)
//   C, D (feature/x: two commits on a branch from B)
//   E (main: merge of feature/x into main, parents=[B, D])
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
fn git_cmd(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_AUTHOR_DATE", "2020-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2020-01-01T00:00:00+00:00")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git command failed to start");
    assert!(
        status.success(),
        "git {} exited with {:?}",
        args.join(" "),
        status.code()
    );
}

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

#[test]
fn test_end_to_end_layout() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Initialize repo.
    git_cmd(dir, &["init", "-b", "main", "."]);
    git_cmd(dir, &["config", "user.name", "Test"]);
    git_cmd(dir, &["config", "user.email", "test@example.com"]);
    git_cmd(dir, &["config", "commit.gpgsign", "false"]);

    // A: initial commit on main.
    write_file(dir, "base.txt", "base\n");
    git_cmd(dir, &["add", "base.txt"]);
    git_cmd(dir, &["commit", "-m", "initial commit A"]);

    // B: second commit on main.
    write_file(dir, "b.txt", "b\n");
    git_cmd(dir, &["add", "b.txt"]);
    git_cmd(dir, &["commit", "-m", "commit B"]);

    // Branch off main → feature/x.
    git_cmd(dir, &["checkout", "-b", "feature/x"]);

    // C: first feature commit.
    write_file(dir, "feat.txt", "c\n");
    git_cmd(dir, &["add", "feat.txt"]);
    git_cmd(dir, &["commit", "-m", "commit C (feature)"]);

    // D: second feature commit.
    write_file(dir, "feat.txt", "d\n");
    git_cmd(dir, &["add", "feat.txt"]);
    git_cmd(dir, &["commit", "-m", "commit D (feature)"]);

    // Back to main, merge feature/x → E (merge commit).
    git_cmd(dir, &["checkout", "main"]);
    git_cmd(dir, &["merge", "--no-ff", "feature/x", "-m", "merge E"]);

    // Open repo and get commit log.
    let repo = Repository::open(dir).expect("failed to open repo");
    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    // Should have 5 commits: A, B, C, D, E.
    assert_eq!(
        commits.len(),
        5,
        "end-to-end: expected 5 commits, got {}: {:?}",
        commits.len(),
        commits.iter().map(|c| &c.summary).collect::<Vec<_>>()
    );

    // Compute layout.
    let gl = layout(&commits);

    // Check all invariants.
    check_invariants(&commits, &gl);

    // Find the merge commit (E) and verify it has 2 OutOfNode edges.
    let merge_idx = commits
        .iter()
        .position(|c| c.summary.starts_with("merge E"))
        .expect("merge commit E not found");
    let merge_row = &gl.rows[merge_idx];

    let out_count = merge_row
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::OutOfNode)
        .count();
    assert_eq!(
        out_count, 2,
        "end-to-end: merge commit E must have 2 OutOfNode edges, got {}",
        out_count
    );
}

// ────────────────────────────────────────────────────────────
// Case 11: criss-cross full variant (larger graph)
//
// A more elaborate criss-cross where two branches each create a commit
// that merges from the other, creating an "X" pattern in the graph.
//
// Layout:
//   Z  (final merge: parents=[X, Y])
//   X  (merge: parents=[A, B])
//   Y  (merge: parents=[B, A])
//   A  (root)
//   B  (root)
//
// Invariant check only.
// ────────────────────────────────────────────────────────────

#[test]
fn test_criss_cross_full() {
    let commits = vec![
        c("Z", &["X", "Y"]),
        c("X", &["A", "B"]),
        c("Y", &["B", "A"]),
        c("A", &[]),
        c("B", &[]),
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 5);
    assert!(gl.lane_count >= 1);
}

// ────────────────────────────────────────────────────────────
// Case 12: parallel branches (2 independent roots + shared tip)
//
// Two branches both sprouting from separate roots.
// After both roots are consumed, a new shared ancestor is reached.
// Tests lane reuse: after processing both roots, the new tip should reuse lane 0.
// ────────────────────────────────────────────────────────────

#[test]
fn test_two_roots_lane_concurrent_then_reuse() {
    // M  (merge: parents=[X, Y])   — lane 0
    // X  (branch a tip)            — lane 0
    // Y  (branch b tip)            — lane 1
    // Both X and Y are roots, so after X: lane 0 freed, after Y: lane 1 freed.
    // M is processed first: it takes lane 0, allocates lane 1 for Y.
    // After both X and Y, all active lanes clear.
    // New tip N for fresh lane-0 reuse:
    let commits = vec![
        c("M", &["X", "Y"]),
        c("X", &[]),
        c("Y", &[]),
        c("N", &["P"]), // new branch after M's subtree is done
        c("P", &[]),
    ];
    let gl = layout(&commits);

    check_invariants(&commits, &gl);

    assert_eq!(gl.rows.len(), 5);

    // After M,X,Y are processed, N should land on lane 0 (leftmost free).
    let row_n = &gl.rows[3];
    assert_eq!(row_n.commit, cid("N"));
    assert_eq!(
        row_n.lane, 0,
        "N must reuse lane 0 after all prior lanes freed"
    );
}

// ────────────────────────────────────────────────────────────
// Case 13: stacked branches — Stable keeps every branch in its own column
//
// The shipped layout (`build_commit_rows` uses `GraphLayoutMode::Stable`,
// ADR-0122). Three merges into a mainline, each merging a segment of one
// branch chain:
//
//   P1 (merge: [P2, T1])   lane 0
//   P2 (merge: [P3, T4])   lane 0
//   P3 (merge: [P4, T5])   lane 0
//   T1 → T2 → T3           lane 1  (segment merged by P1)
//   T4                     lane 2  (segment merged by P2)
//   T5                     lane 3  (segment merged by P3)
//   P4 (root)              lane 0
//
// Each merge line must run straight down its own column from the merge
// commit to the merged segment ("staircase" shape); a long line must never
// bend sideways at its target commit's row.
// ────────────────────────────────────────────────────────────

#[test]
fn test_stable_stacked_branches_staircase() {
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
    let gl = layout_with(&commits, GraphLayoutMode::Stable);
    check_invariants(&commits, &gl);

    // Staircase: each branch segment keeps the column its merge line opened.
    let lane_of = |id: &str| gl.rows.iter().find(|r| r.commit == cid(id)).unwrap().lane;
    assert_eq!(lane_of("T1"), 1);
    assert_eq!(lane_of("T2"), 1);
    assert_eq!(lane_of("T3"), 1);
    assert_eq!(lane_of("T4"), 2, "T4 stays on P2's merge-line column");
    assert_eq!(lane_of("T5"), 3, "T5 stays on P3's merge-line column");

    // No row is approached by two lanes (joins happen at the child's row,
    // never as a sideways bend at the target commit's row).
    for row in &gl.rows {
        let into = row
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::IntoNode)
            .count();
        assert!(
            into <= 1,
            "row {}: {} IntoNode edges (duplicate lanes)",
            row.commit,
            into
        );
    }
}

// ════════════════════════════════════════════════════════════
// Compact (Gitru swimlane) mode — ADR-0104
// ════════════════════════════════════════════════════════════

/// Relaxed invariant checker for `GraphLayoutMode::Compact`.
///
/// Identical to [`check_invariants`] except invariant 2 allows a `Pass` edge to
/// shift columns (`from_lane != to_lane`) — that is exactly how compaction
/// expresses a lane being reclaimed. Continuity (inv 3), lane-count bounds
/// (inv 4) and no-duplicate-edges (inv 5) must still hold.
fn check_invariants_compact(commits: &[Commit], gl: &GraphLayout) {
    // Inv 1: row/commit correspondence.
    assert_eq!(gl.rows.len(), commits.len(), "compact inv1: row count");
    for (i, (row, commit)) in gl.rows.iter().zip(commits.iter()).enumerate() {
        assert_eq!(row.commit, commit.id, "compact inv1 at row {i}");
    }

    // Inv 2 (relaxed): directed edges must touch the node lane; Pass may shift.
    for (i, row) in gl.rows.iter().enumerate() {
        for edge in &row.edges {
            match edge.kind {
                EdgeKind::IntoNode => {
                    assert_eq!(edge.to_lane, row.lane, "compact inv2 IntoNode @{i}")
                }
                EdgeKind::OutOfNode => {
                    assert_eq!(edge.from_lane, row.lane, "compact inv2 OutOfNode @{i}")
                }
                EdgeKind::Pass => {} // shift edges are allowed in Compact
            }
        }
    }

    // Inv 3: edge continuity between adjacent rows (column-aware, so shifts OK).
    for i in 0..gl.rows.len().saturating_sub(1) {
        let bottom_open: HashSet<usize> = gl.rows[i]
            .edges
            .iter()
            .filter_map(|e| match e.kind {
                EdgeKind::Pass | EdgeKind::OutOfNode => Some(e.to_lane),
                EdgeKind::IntoNode => None,
            })
            .collect();
        let top_open: HashSet<usize> = gl.rows[i + 1]
            .edges
            .iter()
            .filter_map(|e| match e.kind {
                EdgeKind::Pass | EdgeKind::IntoNode => Some(e.from_lane),
                EdgeKind::OutOfNode => None,
            })
            .collect();
        assert_eq!(
            bottom_open,
            top_open,
            "compact inv3 between rows {i} and {}: {bottom_open:?} != {top_open:?}",
            i + 1
        );
    }

    // Inv 4: lane_count bounds every lane used.
    for (i, row) in gl.rows.iter().enumerate() {
        assert!(row.lane < gl.lane_count, "compact inv4 row.lane @{i}");
        for edge in &row.edges {
            assert!(edge.from_lane < gl.lane_count, "compact inv4 from @{i}");
            assert!(edge.to_lane < gl.lane_count, "compact inv4 to @{i}");
        }
    }

    // Inv 5: no duplicate (from, to, kind) edges within a row.
    for (i, row) in gl.rows.iter().enumerate() {
        let mut seen: HashSet<(usize, usize, &str)> = HashSet::new();
        for edge in &row.edges {
            let kind_str = match edge.kind {
                EdgeKind::Pass => "pass",
                EdgeKind::IntoNode => "into",
                EdgeKind::OutOfNode => "out",
            };
            assert!(
                seen.insert((edge.from_lane, edge.to_lane, kind_str)),
                "compact inv5: duplicate edge @{i}"
            );
        }
    }
}

#[test]
fn test_compact_invariants_on_branch_merge_chain() {
    // M(A,B), A(C), C(D), D(), B()  — exercises lane reclaim + a shift edge.
    let commits = vec![
        c("M", &["A", "B"]),
        c("A", &["C"]),
        c("C", &["D"]),
        c("D", &[]),
        c("B", &[]),
    ];
    let stable = layout_with(&commits, GraphLayoutMode::Stable);
    let compact = layout_with(&commits, GraphLayoutMode::Compact);

    // Stable still satisfies the strict checker; Compact the relaxed one.
    check_invariants(&commits, &stable);
    check_invariants_compact(&commits, &compact);

    // Compaction must not widen the graph beyond the stable layout here.
    assert!(compact.lane_count <= stable.lane_count);

    // A shift edge (Pass with from != to) must be present somewhere.
    let has_shift = compact.rows.iter().any(|r| {
        r.edges
            .iter()
            .any(|e| e.kind == EdgeKind::Pass && e.from_lane != e.to_lane)
    });
    assert!(
        has_shift,
        "compact mode must emit a shift edge for this DAG"
    );
}

// ────────────────────────────────────────────────────────────
// Compact: first-parent join — a commit is approached by at most one lane
//
// Repro of the "long line bends at the very end" artifact (stacked branches
// merged into a mainline). Pre-fix, T3's first-parent continuation opened a
// SECOND lane targeting T4 — duplicating the lane already opened by P2's
// merge edge — so both lines ran in parallel and only converged (with a
// sideways bend) at T4's own row. With the first-parent join (Stable's
// step-4 exception ported to Compact), T3's line joins the existing lane at
// T3's row instead.
// ────────────────────────────────────────────────────────────

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
    check_invariants_compact(&commits, &gl);

    // No duplicate lanes: every commit is reached by at most one lane, so no
    // row carries two IntoNode edges.
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

    // T4 continues P2's merge line: same colour as the merge-out edge, and
    // the line arrives vertically (no bend at the target row).
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

#[test]
fn test_compact_tip_joins_existing_lane() {
    // Y and X both point at A. X (the later tip) must not open a second lane
    // targeting A; it bends into Y's lane at its own row.
    let commits = vec![c("Y", &["A"]), c("X", &["A"]), c("A", &[])];
    let gl = layout_with(&commits, GraphLayoutMode::Compact);
    check_invariants_compact(&commits, &gl);

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

#[test]
fn test_compact_invariants_on_octopus_and_multiroot() {
    // A diamond + an octopus merge + a second root, to stress lane packing.
    let commits = vec![
        c("T", &["O", "R"]),         // merge of octopus result O and root R
        c("O", &["P0", "P1", "P2"]), // octopus merge (3 parents)
        c("P0", &["Z"]),
        c("P1", &["Z"]),
        c("P2", &["Z"]),
        c("Z", &[]),
        c("R", &[]),
    ];
    let compact = layout_with(&commits, GraphLayoutMode::Compact);
    check_invariants_compact(&commits, &compact);
    // Every branch keeps a stable colour index within NUM_COLORS.
    for row in &compact.rows {
        assert!(row.color < kagi::graph::NUM_COLORS);
        for e in &row.edges {
            assert!(e.color < kagi::graph::NUM_COLORS);
        }
    }
}
