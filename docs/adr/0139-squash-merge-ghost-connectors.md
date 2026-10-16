# ADR-0139: Squash-merged branches get a dashed ghost connector

- Status: Accepted
- Date: 2026-08-21

## Context

A squash merge replays a branch as one *new* commit on the target. The
branch's own commits are never rewritten and never referenced, so no parent
pointer links the two. Git's graph is drawn from parent pointers, so the
branch stays a dead-end leaf **forever**.

ADR-0138 made those branches deletable. It did not make them legible: the user
reported that after a squash merge the branch "残り続けている" — still sitting
there, unconnected to where it actually went. The graph is telling the truth
and it still reads as a bug.

## Decision

Draw the missing link as a **dashed connector** from the squash commit down to
the tip it replayed. Dashed, and in a muted foreground tint, because it is not
a parent link — it is patch-id equivalence, a different claim, and it must not
be mistaken for history.

### Detection: invert the loop

`squash_merged_as` (ADR-0138) walks the target's history per branch. Asking it
for every branch costs `branches × SQUASH_SCAN_LIMIT` tree diffs — **measured
at >15s** on this repo (1109 commits, 24 stale branches). Unusable anywhere
near a refresh.

`collect_squash_links` indexes the window's patch-ids **once** into a
`patch-id → commit` map, then does one combined diff per branch:
`SQUASH_SCAN_LIMIT + branches`. **Measured at 604ms** on the same repo, finding
all 24 links. Both entry points share `combined_patch_id` / `commit_patch_id`,
so the equivalence test cannot drift between them.

It runs on a background thread with a `squash_gen` supersede token, exactly
like Branch Cleanup (ADR-0128) — for the same reason: the snapshot path runs
after every git operation and must not grow a 600ms step.

### Rendering: reuse the stash lane trick

Stashes (ADR-0088) already draw a line that the layout algorithm knows nothing
about: allocate a lane, push `Pass` edges through the rows in between, curve
into the target commit — all as a post-pass over built rows. The connector is
the same shape, mirrored (the squash commit is *newer*, so it is above).
`kagi-domain`'s layout is untouched.

Two choices worth recording:

- **Column**: the connector first tries the tip's *own* lane. If that column
  has been reclaimed, it reuses a nearby safe lane before allocating a new
  one (see the local-routing correction below). Historical graph width must
  not force a short connector to the far right.
- **Ghost marking**: a `GHOST_COLOR` sentinel on `GraphEdge::color`, not a lane
  set. Because the connector deliberately shares a column with a real branch
  line, "is this lane a ghost" is not answerable the way `stash_lanes` is.

## Correction (2026-08-21)

The detection this depends on was tightened — see ADR-0138's correction. Two
knock-on changes here:

- The connector uses the same byte-exact confirmation as the delete gate. It
  could have kept the loose patch-id test (a line is not an irreversible
  action), but a graph that says "merged here" while the delete says
  "unmerged" is worse than either answer alone.
- Dropping the timestamp prune costs time: `collect_squash_links` now always
  scans up to `SQUASH_SCAN_LIMIT` commits instead of stopping early when every
  candidate branch is recent. Measured 604ms → 1.57s on this repo. It is a
  background job with a supersede token, so the trade is worth it.

`inject_squash_edges` also dedupes by `(tip, squash)`: links are per branch, so
two branches at the same tip drew the identical line twice and the second one
took a whole new column.

## Local-routing correction (2026-10-17, #1627)

The original fallback allocated at the graph-wide high-water mark. An old,
wide merge could therefore turn a short squash relationship into a large
U-shaped detour even when nearby columns were empty throughout its interval.

`kagi-domain::graph::squash::select_lane` now applies this policy:

1. Keep the tip's own column if its connecting interval is free.
2. Otherwise choose an existing free column by `(maximum endpoint distance,
   sum of endpoint distances, column index)`, minimizing the longer detour
   first and resolving ties deterministically.
3. Allocate one new high-water column only when no existing column is safe.

Candidates are visited from the endpoint midpoint outward without allocating
or sorting; the first safe candidate wins. Occupancy checks include the squash
row's lower half, the tip row's upper half, and every intervening node and
both edge halves, regardless of color. Already-injected ghosts reserve their
paths: overlapping connectors separate, while disjoint intervals can reuse
the same column. Only a newly allocated column expands the shared width.

The UI still injects complete `OutOfNode` / `Pass` / `IntoNode` paths after the
Stable parent layout. Real nodes, parent edges, colors, WIP anchors and stash
ownership do not move. Alias deduplication, unloaded/reversed endpoint skips,
scan ownership and generation guards, and muted/dashed rendering are unchanged.
WIP-to-HEAD routing retains its separate same-lane policy (ADR-0174).

Regression coverage includes domain collision/ranking boundaries, UI injection
and identity preservation, and `squash_graph_routing` native canvas coverage.
The latter observes submitted endpoint commands and dashed segments in classic
and avatar modes at 100% and 167%, including scrolling; it does not substitute
a lane-number assertion for complete painted-path continuity.

## Consequences

- The graph gains ~600ms of background work per reload. It is off the UI
  thread and the result is dropped if superseded, so a fast series of
  operations costs nothing but wasted background cycles.
- A link is drawn only when **both** ends are inside the loaded commit window.
  A squash commit outside it draws nothing rather than half a line.
- Rebase-and-merge is still not detected (ADR-0138): each commit is replayed
  separately, so no combined patch-id matches.
- The end arcs of a connector are solid (muted); only its straight runs are
  dashed. Dashing a Bézier needs arc-length parameterisation and the arcs are
  ~9px.
