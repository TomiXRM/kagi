# ADR-0174: WIP → HEAD dashed connector

- Status: Accepted
- Date: 2026-09-06
- Touches: `crates/kagi-domain/src/refs.rs`, `crates/kagi-git/src/snapshot.rs`,
  `src/ui/graph_wip.rs` (new), `src/ui/graph_squash.rs`, `src/ui/graph_view.rs`,
  `src/ui/tab_view.rs`, `src/ui/render_body.rs`, `src/ui/render_wip.rs`
- Issue: #472
- Builds on: ADR-0139 (squash ghost connectors), ADR-0088 (stash lanes),
  Model A+ (one WIP row per dirty worktree)

## Context

The WIP rows sit above the commit graph. When `origin` is ahead, HEAD is buried
several rows down and nothing on screen says which commit the next commit will
land on. With several dirty worktrees there are k WIP rows and k different
HEADs, and no way to pair them up.

## Decision

Draw one dashed connector per WIP row, down to that worktree's HEAD, in the
worktree's own lane colour, ending in a solid curve into HEAD's node.

1. **Data.** `kagi_domain::refs::Worktree` gains `head: Option<CommitId>`, read
   straight off that worktree's `HEAD` in `collect_worktrees` — never resolved
   from the branch name, so a detached worktree still reports a target. The
   field is plain data; `kagi-domain` stays pure.

2. **Injection.** `graph_wip::inject_wip_edges` is a post-pass over the built
   `CommitRow`s, structurally the squash ghost connector (ADR-0139) with a
   different pair of endpoints: `Pass` through every row above HEAD, `IntoNode`
   at HEAD. It reuses `graph_squash`'s `top_busy` / `bottom_busy` rather than
   copying them. Lane choice follows the same rule — reuse HEAD's own column
   when free, otherwise a fresh lane; with `origin` ahead that column is
   occupied by HEAD's descendants, so a fresh lane is the normal (and correct)
   outcome. Two connectors can never collide because the first one's `Pass`
   edges mark its lane busy for the next.

   It runs in `build_tab_view`, not in a background scan as the squash pass
   does: every input is already in the snapshot, so there is nothing to wait
   for. The lanes it returns are stored as `TabViewState::wip_lanes`,
   positionally aligned with the WIP rows.

3. **Colour + dash: a sentinel range.** `GHOST_COLOR` (`usize::MAX`) is a single
   fixed grey and cannot say *which* worktree a line belongs to. So
   `GraphEdge::color` carries `WIP_GHOST_BASE + <lane colour index>`
   (`WIP_GHOST_BASE = usize::MAX - 64`), which the painter reads as "dashed, in
   `lane_color(idx)`". Rejected alternative: a `dashed: bool` field on
   `GraphEdge` — cleaner in the abstract, but 15 literal sites to touch, and it
   still would not carry the colour. Both are reversible; the sentinel matches
   the ADR-0139 precedent and keeps the whole feature inside the UI layer.

4. **The WIP row itself.** Its graph column is now an ordinary
   `graph_view::graph_canvas` rather than a hand-placed `div` circle: the node
   is the dot, an `OutOfNode` edge on its own lane is the dashed stub down to
   the row's bottom edge, and one `Pass` per connector belonging to a WIP row
   above it keeps those lines running through. Reusing the painter is what makes
   the lane geometry, zoom scaling and horizontal scroll line up with the rows
   below — a bespoke canvas would have to re-derive all three.

## Consequences

- A WIP row's dot is now filled (the standard node) rather than a hollow ring.
- `render_wip_row` takes a lane **colour index** instead of an `Hsla`, since the
  graph painter is indexed by colour, not by colour value.
- `wip_targets` (which WIP rows exist, in order) and `render_body`'s row builder
  must stay row-for-row identical; the lanes are matched by position. Both sides
  carry a comment saying so.
- HEAD outside the loaded window draws nothing rather than half a line — the
  same rule stashes use for `connected == false`.

## Out of scope

Making the WIP rows first-class rows *inside* the graph canvas (design A in
#472). This ships the div + small-canvas version; migrate if the visuals fall
short.
