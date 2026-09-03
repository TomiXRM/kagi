//! Commit list row data and badge helpers — T008 / T009
//!
//! All display strings are pre-computed at snapshot time; the render closure
//! only clones SharedString values, never calling format! per frame.

use std::collections::HashMap;

use gpui::SharedString;

use kagi::graph::{layout, EdgeKind, GraphEdge};
use kagi_git::{Commit, CommitId, Head, RepoSnapshot};

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
    /// Semantic label, e.g. `"main ✓"`, `"origin/main"`, `"v0.1.0"`. Drag /
    /// double-click / context menus key on it (Remote = the full ref).
    pub label: SharedString,
    /// Remote-tracking refs at the same commit that this LOCAL badge absorbed
    /// (`origin/main` for `main`, or the tracked upstream when the names
    /// differ). Rendered as a ☁ mark + tooltip instead of a second chip —
    /// `[main][origin/main]` pairs used to eat the whole badge column.
    pub remotes: Vec<SharedString>,
}

impl RefBadge {
    pub fn new(kind: BadgeKind, label: impl Into<SharedString>) -> Self {
        Self {
            kind,
            label: label.into(),
            remotes: Vec::new(),
        }
    }
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

    // Branches checked out in a *linked* worktree. Tips of these (other than
    // the current HEAD branch) get the 🌲 glyph so the graph shows, at a
    // glance, which branches are live in a worktree (Model A+ multi-HEAD
    // markers). The MAIN worktree's branch is deliberately excluded: seen from
    // inside a linked worktree it is "checked out elsewhere" too, but tagging
    // it 🌲 read as "master is a worktree branch" in every worktree tab
    // (user report). Checkout protection for it is unaffected — that comes
    // from `checked_out_worktree_path`, not this glyph.
    let worktree_branches: std::collections::HashSet<&str> = snap
        .worktrees
        .iter()
        .filter(|w| !w.is_main)
        .filter_map(|w| w.branch.as_deref())
        .collect();

    // Local branches.
    for b in &snap.branches {
        let is_head_branch = head_branch_name == Some(b.name.as_str());
        let in_other_worktree = !is_head_branch && worktree_branches.contains(b.name.as_str());
        let label = if is_head_branch {
            SharedString::from(format!("{} ✓", b.name))
        } else if in_other_worktree {
            // 🌲 marks a branch checked out in another worktree (matches the
            // worktree's WIP row marker).
            SharedString::from(format!("🌲 {}", b.name))
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
            .push(RefBadge::new(kind, label));
    }

    // Detached HEAD: add a standalone HEAD badge.
    if let Head::Detached { target } = &snap.head {
        let commit_id = CommitId(target.clone());
        map.entry(commit_id)
            .or_default()
            .insert(0, RefBadge::new(BadgeKind::HeadBranch, "HEAD"));
    }

    // Remote-tracking branches. A remote ref at the SAME commit as a local
    // branch of the same name — or as the local branch that tracks it — is
    // folded into that local badge (`remotes`) instead of getting its own
    // chip; the local name stays the label. Everything else is a Remote chip.
    for rb in &snap.remote_branches {
        let full = format!("{}/{}", rb.remote, rb.name);
        let locals_here = map.get_mut(&rb.target);
        let absorbed = locals_here.and_then(|badges| {
            badges
                .iter_mut()
                .filter(|b| matches!(b.kind, BadgeKind::Branch | BadgeKind::HeadBranch))
                .find(|b| {
                    let local_name = local_badge_name(&b.label);
                    local_name == rb.name
                        || snap
                            .branches
                            .iter()
                            .find(|lb| lb.name == local_name)
                            .and_then(|lb| lb.upstream.as_ref())
                            .is_some_and(|u| u.remote_branch == full)
                })
                .map(|b| b.remotes.push(SharedString::from(full.clone())))
        });
        if absorbed.is_none() {
            map.entry(rb.target.clone())
                .or_default()
                .push(RefBadge::new(BadgeKind::Remote, full));
        }
    }

    // Tags.
    for t in &snap.tags {
        map.entry(t.target.clone())
            .or_default()
            .push(RefBadge::new(BadgeKind::Tag, t.name.clone()));
    }

    map
}

/// The bare branch name inside a local badge label (`"🌲 name"`, `"name ✓"`).
pub fn local_badge_name(label: &str) -> &str {
    label
        .trim_start_matches("\u{1f332} ")
        .trim_end_matches(" \u{2713}")
}

/// Where a ref lives, drawn as leading icons on the chip: laptop = exists as
/// a local branch, cloud = exists on a remote. Both = local+remote at this
/// commit. One rule, no colour-as-meaning (user request).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadgeWhere {
    pub local: bool,
    pub remote: bool,
}

/// What a badge shows: `(name, where)`. Remote-only chips drop a leading
/// `origin/` (the cloud icon says "remote"; the tooltip keeps the full ref);
/// other remotes keep their prefix so two remotes never collide. Tags and
/// the detached-HEAD marker carry no icons.
pub fn badge_display(b: &RefBadge) -> (String, BadgeWhere) {
    match b.kind {
        BadgeKind::Remote => {
            let l: &str = b.label.as_ref();
            (
                l.strip_prefix("origin/").unwrap_or(l).to_string(),
                BadgeWhere {
                    local: false,
                    remote: true,
                },
            )
        }
        BadgeKind::Branch | BadgeKind::HeadBranch => (
            b.label.to_string(),
            BadgeWhere {
                local: b.label.as_ref() != "HEAD",
                remote: !b.remotes.is_empty(),
            },
        ),
        BadgeKind::Tag => (
            b.label.to_string(),
            BadgeWhere {
                local: false,
                remote: false,
            },
        ),
    }
}

/// Tooltip text: the full label plus every absorbed remote ref.
pub fn badge_tooltip(b: &RefBadge) -> String {
    let mut t = b.label.to_string();
    for r in &b.remotes {
        t.push('\n');
        t.push_str(r);
    }
    t
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
    /// AI-agent provenance verdict (issue #337). `None` = show no agent badge
    /// (unclassifiable commits deliberately render nothing).
    pub provenance: Option<kagi_domain::provenance::Provenance>,
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

impl CommitRow {
    /// A blank row carrying only an id — for tests that exercise graph-edge
    /// post-passes (`graph_squash`) and care about lanes, not display strings.
    #[cfg(test)]
    pub fn empty_for_test(id: CommitId) -> Self {
        Self {
            id,
            short_id: SharedString::default(),
            summary: SharedString::default(),
            author: SharedString::default(),
            author_email: String::new(),
            date: SharedString::default(),
            badges: Vec::new(),
            provenance: None,
            lane: 0,
            node_color: 0,
            edges: Vec::new(),
            lane_count: 1,
            parents: Vec::new(),
            is_head: false,
            is_merge: false,
        }
    }
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

    // Compute commit graph layout once up-front (T009). Lanes use the gitk
    // layout (ADR-0122): a branch keeps its column until it ends and freed
    // columns are reused by *new* lanes (never by shifting existing ones), so
    // long branch lines run straight and bend only where they fork or join —
    // the Fork/GitKraken look. The "Avatar commit nodes" setting
    // (`graph_lane_compact`) only changes how nodes/rows are drawn, not the
    // lane layout.
    let graph = layout(&snap.commits);
    let lane_count = graph.lane_count;

    // Issue #337: user-extensible agent-provenance patterns, layered on the
    // built-in defaults. Loaded once per build (not per row).
    let agent_patterns = kagi_ui_core::settings::Settings::load().agent_patterns();

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
                c,
                &badge_map,
                now_secs,
                lane,
                node_color,
                edges,
                lane_count,
                is_head,
                is_merge,
                &agent_patterns,
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
    agent_patterns: &[kagi_domain::provenance::AgentPattern],
) -> CommitRow {
    let short_id = SharedString::from(c.id.short().to_string());

    // issue #414: commit summary + author are remote-origin text shown on every
    // graph row (the list the user looks at continuously). Neutralize terminal
    // control bytes before truncation/display, matching the detail panel.
    let safe_summary = kagi_domain::text_safety::sanitize_control_bytes(&c.summary);
    // Truncate summary at 72 chars to keep rows manageable.
    // Count chars (not bytes): byte slicing would panic on multi-byte
    // summaries (e.g. Japanese commit messages).
    let summary = if safe_summary.chars().count() > 72 {
        let truncated: String = safe_summary.chars().take(71).collect();
        SharedString::from(format!("{truncated}…"))
    } else {
        SharedString::from(safe_summary)
    };

    let author = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(
        &c.author.name,
    ));
    let author_email = c.author.email.clone();
    let date = SharedString::from(relative_time(c.author.time, now_secs));
    let badges = badge_map.get(&c.id).cloned().unwrap_or_default();

    // Issue #337: classify agent provenance from trailers + author/committer +
    // any local branch badge on this commit. `None` renders no badge.
    let trailers = kagi_domain::trailers::parse_trailers(&c.message);
    let branch_label = badges
        .iter()
        .find(|b| matches!(b.kind, BadgeKind::HeadBranch | BadgeKind::Branch))
        .map(|b| b.label.as_ref());
    let provenance = kagi_domain::provenance::classify_provenance(
        &trailers,
        &c.author,
        &c.committer,
        branch_label,
        agent_patterns,
    );

    CommitRow {
        id: c.id.clone(),
        short_id,
        summary,
        author,
        author_email,
        date,
        badges,
        provenance,
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

// ADR-0121 C3: `now_unix_secs` / `relative_time` moved to `kagi_ui_core::time`
// so pane crates can reuse them; re-exported here to keep existing
// `commit_list::…` call sites working.
pub use kagi_ui_core::time::{now_unix_secs, relative_time};

#[cfg(test)]
mod worktree_badge_tests {
    use super::*;
    use kagi_git::{Branch, Head, RepoSnapshot, Worktree};

    fn snap(head_branch: &str) -> RepoSnapshot {
        let tip = CommitId("a".repeat(40));
        let branch = |name: &str| Branch {
            name: name.into(),
            target: tip.clone(),
            upstream: None,
        };
        let wt = |branch: &str, is_main: bool, is_current: bool| Worktree {
            name: if is_main {
                "main".into()
            } else {
                branch.into()
            },
            path: std::path::PathBuf::from(format!("/r/{}", branch)),
            branch: Some(branch.into()),
            is_current,
            is_main,
            wip: None,
            locked: false,
            lock_reason: None,
        };
        RepoSnapshot {
            head: Head::Attached {
                branch: head_branch.into(),
                target: "a".repeat(40),
            },
            commits: Vec::new(),
            branches: vec![branch("master"), branch("feat"), branch("other")],
            remote_branches: Vec::new(),
            tags: Vec::new(),
            status: Default::default(),
            stashes: Vec::new(),
            // master lives in the MAIN worktree, feat in a linked one.
            worktrees: vec![
                wt("master", true, head_branch == "master"),
                wt("feat", false, head_branch == "feat"),
            ],
            cleanup_rows: Vec::new(),
            last_fetch_secs: None,
        }
    }

    fn labels(head: &str) -> Vec<String> {
        let map = build_badge_map(&snap(head));
        let mut v: Vec<String> = map
            .values()
            .flatten()
            .map(|b| b.label.to_string())
            .collect();
        v.sort();
        v
    }

    /// From the main tab, the linked worktree's branch is 🌲; from inside
    /// the linked worktree, the MAIN branch is plain — it is checked out
    /// "elsewhere", but 🌲 read as "master is a worktree branch" in every
    /// worktree tab (user report).
    #[test]
    fn only_linked_worktree_branches_get_the_tree_glyph() {
        // (sorted; the multibyte 🌲 sorts last)
        assert_eq!(labels("master"), vec!["master ✓", "other", "🌲 feat"]);
        assert_eq!(labels("feat"), vec!["feat ✓", "master", "other"]);
    }
}

#[cfg(test)]
mod remote_fold_tests {
    use super::*;
    use kagi_git::{Branch, Head, RemoteBranch, RepoSnapshot, UpstreamInfo};

    fn tip(c: char) -> CommitId {
        CommitId(c.to_string().repeat(40))
    }

    fn base_snap() -> RepoSnapshot {
        RepoSnapshot {
            head: Head::Attached {
                branch: "main".into(),
                target: "a".repeat(40),
            },
            commits: Vec::new(),
            branches: Vec::new(),
            remote_branches: Vec::new(),
            tags: Vec::new(),
            status: Default::default(),
            stashes: Vec::new(),
            worktrees: Vec::new(),
            cleanup_rows: Vec::new(),
            last_fetch_secs: None,
        }
    }

    fn all_badges(snap: &RepoSnapshot) -> Vec<RefBadge> {
        let mut v: Vec<RefBadge> = build_badge_map(snap).into_values().flatten().collect();
        v.sort_by(|a, b| a.label.cmp(&b.label));
        v
    }

    /// `[main][origin/main]` at one commit becomes ONE chip: the local badge
    /// with the remote folded into `remotes` (rendered as ☁ + tooltip).
    #[test]
    fn same_name_remote_at_same_commit_is_folded_into_the_local_badge() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "main".into(),
            target: tip('a'),
            upstream: None,
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "main".into(),
            target: tip('a'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 1, "one chip, not two: {:?}", b);
        assert_eq!(b[0].label.as_ref(), "main ✓");
        assert_eq!(b[0].remotes, vec![SharedString::from("origin/main")]);
        assert_eq!(
            badge_display(&b[0]),
            (
                "main ✓".to_string(),
                BadgeWhere {
                    local: true,
                    remote: true
                }
            )
        );
        assert_eq!(badge_tooltip(&b[0]), "main ✓\norigin/main");
    }

    /// Different names but tracking (local `foo` → origin/bar), same commit:
    /// folded too — the LOCAL name stays the label, the remote is on hover.
    #[test]
    fn tracked_upstream_with_a_different_name_is_folded_too() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "foo".into(),
            target: tip('b'),
            upstream: Some(UpstreamInfo {
                remote_branch: "origin/bar".into(),
                ahead: 0,
                behind: 0,
            }),
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "bar".into(),
            target: tip('b'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 1, "{:?}", b);
        assert_eq!(b[0].label.as_ref(), "foo");
        assert_eq!(b[0].remotes, vec![SharedString::from("origin/bar")]);
    }

    /// Diverged (remote ahead): the remote is at ANOTHER commit → its own
    /// chip, shown as `☁ main` (prefix in the tooltip only).
    #[test]
    fn remote_at_a_different_commit_keeps_its_own_chip() {
        let mut s = base_snap();
        s.branches.push(Branch {
            name: "main".into(),
            target: tip('a'),
            upstream: None,
        });
        s.remote_branches.push(RemoteBranch {
            remote: "origin".into(),
            name: "main".into(),
            target: tip('c'),
        });
        let b = all_badges(&s);
        assert_eq!(b.len(), 2);
        let remote = b.iter().find(|x| x.kind == BadgeKind::Remote).unwrap();
        assert_eq!(
            remote.label.as_ref(),
            "origin/main",
            "label stays the full ref"
        );
        assert_eq!(
            badge_display(remote),
            (
                "main".to_string(),
                BadgeWhere {
                    local: false,
                    remote: true
                }
            )
        );
        // A non-origin remote keeps its prefix so two remotes can't collide.
        let up = RefBadge::new(BadgeKind::Remote, "upstream/main");
        assert_eq!(badge_display(&up).0, "upstream/main");
    }
}
