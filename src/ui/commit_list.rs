//! Commit list row data and badge helpers — T008 / T009
//!
//! All display strings are pre-computed at snapshot time; the render closure
//! only clones SharedString values, never calling format! per frame.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;

use kagi::git::{Commit, CommitId, Head, RepoSnapshot};
use kagi::graph::{layout_with, EdgeKind, GraphEdge, GraphLayoutMode};

use crate::ui::theme;

// ──────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────

/// Extract the HEAD commit target SHA from a [`Head`] value.
/// Returns `None` for unborn repos (no commits yet).
fn head_target(head: &Head) -> Option<&str> {
    match head {
        Head::Attached { target, .. } => Some(target.as_str()),
        Head::Detached { target } => Some(target.as_str()),
        Head::Unborn { .. } => None,
    }
}

// ──────────────────────────────────────────────────────────────
// Badge types
// ──────────────────────────────────────────────────────────────

/// The kind of a ref badge shown on a commit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeKind {
    /// Current HEAD (attached to this branch tip).
    HeadBranch,
    /// Local branch (not HEAD branch).
    Branch,
    /// Remote-tracking branch (e.g. `origin/main`).
    Remote,
    /// Tag.
    Tag,
}

/// A single ref badge to be displayed on a commit row.
#[derive(Debug, Clone)]
pub struct RefBadge {
    pub kind: BadgeKind,
    /// Display label, e.g. `"main ✓"`, `"origin/main"`, `"v0.1.0"`.
    pub label: SharedString,
}

// ──────────────────────────────────────────────────────────────
// Badge map helper
// ──────────────────────────────────────────────────────────────

/// Build a `CommitId → Vec<RefBadge>` map from a [`RepoSnapshot`].
///
/// The HEAD branch badge integrates the HEAD indicator (`✓`) so we don't
/// show a separate HEAD chip when attached.
pub fn build_badge_map(snap: &RepoSnapshot) -> HashMap<CommitId, Vec<RefBadge>> {
    let mut map: HashMap<CommitId, Vec<RefBadge>> = HashMap::new();

    // Determine the HEAD branch name and target (when attached).
    let head_branch_name: Option<&str> = match &snap.head {
        Head::Attached { branch, .. } => Some(branch.as_str()),
        _ => None,
    };

    // Branches checked out in *some* worktree. Tips of these (other than the
    // current HEAD branch) get a worktree glyph so the graph shows, at a glance,
    // which branches are live in a linked worktree (Model A+ multi-HEAD markers).
    let worktree_branches: std::collections::HashSet<&str> = snap
        .worktrees
        .iter()
        .filter_map(|w| w.branch.as_deref())
        .collect();

    // Local branches.
    for b in &snap.branches {
        let is_head_branch = head_branch_name == Some(b.name.as_str());
        let in_other_worktree = !is_head_branch && worktree_branches.contains(b.name.as_str());
        let label = if is_head_branch {
            SharedString::from(format!("{} ✓", b.name))
        } else if in_other_worktree {
            // 🌳 marks a branch checked out in another worktree (matches the
            // worktree's WIP row marker).
            SharedString::from(format!("🌳 {}", b.name))
        } else {
            SharedString::from(b.name.clone())
        };
        let kind = if is_head_branch {
            BadgeKind::HeadBranch
        } else {
            BadgeKind::Branch
        };
        map.entry(b.target.clone())
            .or_default()
            .push(RefBadge { kind, label });
    }

    // Detached HEAD: add a standalone HEAD badge.
    if let Head::Detached { target } = &snap.head {
        let commit_id = CommitId(target.clone());
        map.entry(commit_id).or_default().insert(
            0,
            RefBadge {
                kind: BadgeKind::HeadBranch,
                label: SharedString::from("HEAD"),
            },
        );
    }

    // Remote-tracking branches.
    for rb in &snap.remote_branches {
        let label = SharedString::from(format!("{}/{}", rb.remote, rb.name));
        map.entry(rb.target.clone()).or_default().push(RefBadge {
            kind: BadgeKind::Remote,
            label,
        });
    }

    // Tags.
    for t in &snap.tags {
        let label = SharedString::from(t.name.clone());
        map.entry(t.target.clone()).or_default().push(RefBadge {
            kind: BadgeKind::Tag,
            label,
        });
    }

    map
}

// ──────────────────────────────────────────────────────────────
// Pre-computed row data
// ──────────────────────────────────────────────────────────────

/// Pre-computed display data for one commit row.
///
/// All strings are [`SharedString`] so the render closure can cheaply clone
/// them without re-allocating.
#[derive(Clone)]
pub struct CommitRow {
    /// Full commit id for row-local features (menus, filtering, focus modes).
    pub id: CommitId,
    /// Short (8-hex) commit id. Retained for Detail Panel / oplog (T021: not rendered in row).
    #[allow(dead_code)]
    pub short_id: SharedString,
    /// First line of the commit message (truncated to 72 chars at build time).
    pub summary: SharedString,
    /// Author name (display only).
    pub author: SharedString,
    /// Author email — used by the avatar helper to derive a stable colour.
    pub author_email: String,
    /// Relative date string, e.g. `"3d ago"`, `"2y ago"`.
    pub date: SharedString,
    /// Ref badges for this commit, if any.
    pub badges: Vec<RefBadge>,
    // ── Graph layout fields (T009) ────────────────────────────
    /// Lane index for the commit node (●) in this row.
    pub lane: usize,
    /// Stable colour index for this node's lane (carried with the branch).
    pub node_color: usize,
    /// All edges passing through this row (Pass / IntoNode / OutOfNode).
    pub edges: Vec<GraphEdge>,
    /// Total lane count across the entire graph (needed to compute graph width).
    pub lane_count: usize,
    /// Parent commit ids, preserving Git's first-parent ordering.
    pub parents: Vec<CommitId>,
    // ── Visual flags (W2-GRAPH) ───────────────────────────────
    /// Whether this commit is the current HEAD.
    pub is_head: bool,
    /// Whether this commit is a merge commit (two or more parents).
    pub is_merge: bool,
}

/// Build the full list of [`CommitRow`]s from a snapshot, pre-computing all
/// display strings.  This is called once when the snapshot is ingested; the
/// render closure only clones SharedStrings.
///
/// Also runs [`layout`] once to compute graph lane / edge data (T009).
pub fn build_commit_rows(snap: &RepoSnapshot) -> Vec<CommitRow> {
    let badge_map = build_badge_map(snap);
    let now_secs = now_unix_secs();

    // Resolve HEAD commit id (W2-GRAPH).
    let head_sha: Option<&str> = head_target(&snap.head);

    // Compute commit graph layout once up-front (T009). The lane-assignment
    // mode (gitk-stable vs Gitru swimlane compaction) is a user setting; it
    // defaults to Stable, so the compaction path is fully opt-in / removable.
    let mode = if theme::graph_lane_compact() {
        GraphLayoutMode::Compact
    } else {
        GraphLayoutMode::Stable
    };
    let graph = layout_with(&snap.commits, mode);
    let lane_count = graph.lane_count;

    snap.commits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let graph_row = graph.rows.get(i);
            let lane = graph_row.map(|r| r.lane).unwrap_or(0);
            let node_color = graph_row.map(|r| r.color).unwrap_or(0);
            let edges = graph_row.map(|r| r.edges.clone()).unwrap_or_default();
            // W2-GRAPH: determine HEAD / merge flags.
            let is_head = head_sha.map(|sha| c.id.0 == sha).unwrap_or(false);
            let is_merge = c.parents.len() >= 2;
            commit_to_row(
                c, &badge_map, now_secs, lane, node_color, edges, lane_count, is_head, is_merge,
            )
        })
        .collect()
}

/// Render data for one stash node drawn in the graph (ADR-0088). Stash rows are
/// shown as a fixed block directly below the WIP row; each connects down to its
/// base commit via injected graph edges on a dedicated lane.
#[derive(Debug, Clone)]
pub struct StashRow {
    pub index: usize,
    /// `"stash@{N}: message"`.
    pub label: SharedString,
    /// Dedicated lane assigned to this stash node.
    pub lane: usize,
    /// All lanes (incl. stash lanes) — drives graph column width.
    pub lane_count: usize,
    /// True when the base commit is in view and a branch line was drawn down
    /// to it; false when the base is out of the loaded window (node only).
    pub connected: bool,
}

/// Build commit rows *and* stash graph rows (ADR-0088).
///
/// The main commit graph layout is computed exactly as before (so the mainline
/// is undisturbed). Each stash is then given its **own extra lane** to the right
/// of the mainline, and `Pass`/`IntoNode` edges are injected into the commit
/// rows from the top down to the stash's base commit — so a branch line runs
/// from the stash node (rendered above the list) down to where it sprouted.
///
/// Returns `(commit_rows, stash_rows, stash_lanes)`. `stash_lanes` is the set of
/// lanes used by stashes, passed to the graph painter so those nodes/edges are
/// drawn in the stash colour.
pub fn build_commit_rows_with_stashes(
    snap: &RepoSnapshot,
) -> (Vec<CommitRow>, Vec<StashRow>, Vec<usize>) {
    let mut rows = build_commit_rows(snap);
    let base_lane_count = rows.first().map(|r| r.lane_count).unwrap_or(0);

    // Map commit SHA → row index for base-commit lookup.
    let mut index_of: HashMap<&str, usize> = HashMap::with_capacity(snap.commits.len());
    for (i, c) in snap.commits.iter().enumerate() {
        index_of.insert(c.id.0.as_str(), i);
    }

    let mut stash_rows: Vec<StashRow> = Vec::new();
    let mut stash_lanes: Vec<usize> = Vec::new();
    // ADR-0088: place stash lanes just to the right of the lanes actually in use
    // near the TOP of history (the stash rows + the visible viewport), NOT past
    // the global max lane count. On wide repos the global max occurs deep in
    // history (many concurrent branches), so `base_lane_count` would push the
    // stash nodes and their connection lines off the right edge of the graph
    // column — the connection looked broken (user report, remote SSH repo with
    // 24 lanes / 11 stashes). The top of history is usually narrow, so packing
    // from there keeps the stash node and the visible part of its line on-screen.
    // Deep-base lines still run downward off the viewport (= "connects below").
    // For small repos the window covers the whole graph, so this equals the old
    // `base_lane_count` (no change).
    const STASH_TOP_WINDOW: usize = 64;
    let top_lane_count = rows
        .iter()
        .take(STASH_TOP_WINDOW)
        .flat_map(|r| {
            std::iter::once(r.lane).chain(r.edges.iter().flat_map(|e| [e.from_lane, e.to_lane]))
        })
        .max()
        .map(|m| m + 1)
        .unwrap_or(base_lane_count)
        .min(base_lane_count);
    let mut next_lane = top_lane_count;

    for s in &snap.stashes {
        let lane = next_lane;
        next_lane += 1;
        stash_lanes.push(lane);
        let label = SharedString::from(format!("stash@{{{}}}: {}", s.index, s.message));

        // Resolve the base commit's row (if it's in the loaded window).
        let base_idx = s
            .base
            .as_ref()
            .and_then(|b| index_of.get(b.0.as_str()).copied());

        let connected = if let Some(b) = base_idx {
            let base_lane = rows[b].lane;
            // Pass the stash lane straight down through every row above the base.
            for r in rows.iter_mut().take(b) {
                r.edges.push(GraphEdge {
                    from_lane: lane,
                    to_lane: lane,
                    kind: EdgeKind::Pass,
                    // Stash lanes are painted in the stash colour (see renderer);
                    // `color` is unused for them but the field is required.
                    color: lane,
                });
            }
            // Curve into the base commit node.
            rows[b].edges.push(GraphEdge {
                from_lane: lane,
                to_lane: base_lane,
                kind: EdgeKind::IntoNode,
                color: lane,
            });
            true
        } else {
            false
        };

        stash_rows.push(StashRow {
            index: s.index,
            label,
            lane,
            lane_count: 0, // patched below once the total is known
            connected,
        });
    }

    let total_lanes = next_lane.max(base_lane_count);
    if total_lanes != base_lane_count {
        for r in rows.iter_mut() {
            r.lane_count = total_lanes;
        }
    }
    for sr in stash_rows.iter_mut() {
        sr.lane_count = total_lanes;
    }

    (rows, stash_rows, stash_lanes)
}

#[allow(clippy::too_many_arguments)]
fn commit_to_row(
    c: &Commit,
    badge_map: &HashMap<CommitId, Vec<RefBadge>>,
    now_secs: i64,
    lane: usize,
    node_color: usize,
    edges: Vec<GraphEdge>,
    lane_count: usize,
    is_head: bool,
    is_merge: bool,
) -> CommitRow {
    let short_id = SharedString::from(c.id.short().to_string());

    // Truncate summary at 72 chars to keep rows manageable.
    // Count chars (not bytes): byte slicing would panic on multi-byte
    // summaries (e.g. Japanese commit messages).
    let summary = if c.summary.chars().count() > 72 {
        let truncated: String = c.summary.chars().take(71).collect();
        SharedString::from(format!("{truncated}…"))
    } else {
        SharedString::from(c.summary.clone())
    };

    let author = SharedString::from(c.author.name.clone());
    let author_email = c.author.email.clone();
    let date = SharedString::from(relative_time(c.author.time, now_secs));
    let badges = badge_map.get(&c.id).cloned().unwrap_or_default();

    CommitRow {
        id: c.id.clone(),
        short_id,
        summary,
        author,
        author_email,
        date,
        badges,
        lane,
        node_color,
        edges,
        lane_count,
        parents: c.parents.clone(),
        is_head,
        is_merge,
    }
}

// ──────────────────────────────────────────────────────────────
// Relative time helper (no external crates)
// ──────────────────────────────────────────────────────────────

/// Return the current time as seconds since Unix epoch.
/// Falls back to 0 if SystemTime is unavailable (should never happen).
pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Format a Unix-epoch timestamp as a human-readable relative string.
///
/// | Range          | Output example |
/// |----------------|----------------|
/// | < 60 s         | `"just now"`   |
/// | < 60 min       | `"42m ago"`    |
/// | < 24 h         | `"5h ago"`     |
/// | < 30 days      | `"3d ago"`     |
/// | < 12 months    | `"4mo ago"`    |
/// | ≥ 12 months    | `"2y ago"`     |
pub fn relative_time(epoch_secs: i64, now_secs: i64) -> String {
    let diff = now_secs.saturating_sub(epoch_secs).max(0);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3_600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86_400 {
        format!("{}h ago", diff / 3_600)
    } else if diff < 86_400 * 30 {
        format!("{}d ago", diff / 86_400)
    } else if diff < 86_400 * 365 {
        format!("{}mo ago", diff / (86_400 * 30))
    } else {
        format!("{}y ago", diff / (86_400 * 365))
    }
}
