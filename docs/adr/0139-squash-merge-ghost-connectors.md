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

- **Column**: the connector first tries the tip's *own* lane, which is dead
  above the tip and is exactly the empty channel it wants. Only if something
  reclaimed that column does it allocate a new lane. Without this, a repo with
  24 stale branches would grow 24 columns.
- **Ghost marking**: a `GHOST_COLOR` sentinel on `GraphEdge::color`, not a lane
  set. Because the connector deliberately shares a column with a real branch
  line, "is this lane a ghost" is not answerable the way `stash_lanes` is.

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
