# ADR-0174: WIP anchor points and dashed HEAD connectors

- Status: Accepted
- Date: 2026-09-06
- Touches: `crates/kagi-domain/src/{refs,graph}.rs`, `crates/kagi-git/src/snapshot.rs`,
  `src/ui/{graph_wip,graph_squash,graph_view,tab_view,render_body,render_wip}.rs`
- Issue: #472; amended by #476 (target identity) and #767 (hollow anchors)
- Builds on: ADR-0139 (squash ghost connectors), ADR-0088 (stash lanes),
  Model A+ (one WIP row per dirty worktree)

## Context

The WIP rows sit above the commit graph. When `origin` is ahead, HEAD is buried
several rows down and nothing on screen says which commit the next commit will
land on. With several dirty worktrees there are k WIP rows and k different
HEADs, and no way to pair them up.

## Decision

Draw a hollow virtual commit point for each resolved WIP row, joined to its
badge and HEAD by dashed connectors in HEAD's stable lane colour. A dedicated
connector column retains the existing solid landing curve into HEAD.

1. **Data.** `kagi_domain::refs::Worktree` gains `head: Option<CommitId>`, read
   straight off that worktree's `HEAD` in `collect_worktrees` — never resolved
   from the branch name, so a detached worktree still reports a target. The
   field is plain data; `kagi-domain` stays pure.

2. **Injection and pure anchoring.** `graph_wip::inject_wip_edges` is a
   synchronous post-pass over the built `CommitRow`s: `Pass` through every row
   above HEAD, then `IntoNode` at HEAD. `kagi_domain::graph::wip_anchor` selects
   the lane and colour from borrowed `RowOccupancy` projections, without an
   intermediate row allocation or any I/O. It shares `lane_top_busy` /
   `lane_bottom_busy` with squash routing.

   Prefer HEAD's column when it is free all the way up; otherwise use an unused
   dedicated column so a WIP connector cannot overdraw HEAD's descendants.
   Distinct HEADs reserve distinct paths. Multiple WIPs on the **same HEAD**
   share one `WipAnchor { lane, color }` and one injected trace; their hollow
   nodes remain separate rows, aligned vertically.

   `TabViewState::wip_lanes` maps each `WipTarget` to an optional anchor. The
   target-keyed join survives a clean worktree's row disappearing before a
   replacement snapshot; it is never a positional zip with rendered rows.

3. **Colour + dash: a sentinel range.** `GHOST_COLOR` (`usize::MAX`) is a single
   fixed grey and cannot say *which* worktree a line belongs to. So
   `GraphEdge::color` carries `WIP_GHOST_BASE + <HEAD lane colour index>`
   (`WIP_GHOST_BASE = usize::MAX - 64`), which the painter reads as "dashed, in
   `lane_color(idx)`". Rejected alternative: a `dashed: bool` field on
   `GraphEdge` — cleaner in the abstract, but 15 literal sites to touch, and it
   still would not carry the colour. The sentinel preserves ADR-0139's
   precedent. PR swimlanes continue to exclude WIP and squash sentinels before
   remapping columns: local working-tree annotations are not PR ancestry.

4. **The WIP row.** The existing `graph_view::graph_canvas` gains a stroke-only
   WIP node variant, not a second renderer. Its ring uses the lane colour
   unmodified and a zoom-scaled **2px stroke**, with no avatar or background
   fill, including on hover and selection. Its radius matches the visible HEAD
   ring in classic mode and the commit avatar disc in swimlane mode.

   A dashed horizontal connector crosses the badge column, divider and graph
   canvas to the ring's edge. One `OutOfNode` at the topmost WIP starts a
   connector; subsequent rows carry one `Pass` per distinct anchor. Shared-HEAD
   passes leave the hollow ring's interior empty rather than painting a fill
   over the line. Stash rows carry the same deduplicated passing paths.

   WIP and commit rows share the left inset, lane geometry, zoom and horizontal
   scrolling. Their columns stay aligned when the row is selected or zoomed.

## Consequences

- Detached HEAD uses its actual OID, never an inferred branch name.
- A loaded HEAD outside the viewport retains its anchor and clipped connector.
  An unloaded or unborn HEAD has no anchor: no invented lane-0 node or stub.
- WIP badge/ring/connector colour follows HEAD, not worktree enumeration.
- Pure domain tests cover occupancy at both row halves and unresolved endpoints.
  Tier A `commit_row_layout_wip` measures actual node/dash paints for three
  distinct HEADs, shared HEAD, detached/unborn state, zoom, hover and selection.
  Existing connector and linked-worktree interaction scenarios remain in force.

## Out of scope

Making the WIP rows first-class rows *inside* the graph canvas (design A in
#472). This ships the div + small-canvas version; migrate if the visuals fall
short.
