# ADR-0138: A squash-merged branch is deletable, proven by patch-id

- Status: Accepted
- Date: 2026-08-20

## Context

`plan_delete_branch` gated deletion on reachability alone: the branch tip had
to be an ancestor of HEAD (`repo.graph_descendant_of(head, tip)`), otherwise
the plan carried a `DeleteUnmerged` **blocker**.

A squash merge replays the whole branch as one *new* commit on the target. The
branch's own commits are never rewritten and never become ancestors, so that
check answers "unmerged" forever. In the graph the branch sits as a dead-end
leaf — which is exactly how the user described it when they reported that kagi
could not delete branches they had already squash-merged on GitHub.

Force delete is deliberately absent (ADR-0014), so there was no escape hatch:
those branches simply accumulated.

## Decision

When reachability says "not merged", try to **prove** the merge instead:
`squash_merged_as()` (`crates/kagi-git/src/ops/squash_merge.rs`) computes the
patch-id of `merge_base(HEAD, tip)..tip` — the branch's combined change — and
walks `base..HEAD` looking for a single-parent commit whose own patch-id is
identical. This is `git cherry`'s equivalence test, via libgit2's
`git_diff_patchid`.

**Correction (2026-08-21): patch-id alone is not sufficient, and this ADR
originally claimed it was.** `git patch-id` strips whitespace, so two diffs
differing only in indentation hash the same — verified: indenting a line by
four spaces and by two tabs both give `62d419e8…`. In Python that is a
behaviour change, and `git branch -d` correctly refuses. Being *looser than
git* on an irreversible delete with no `-D` escape hatch is the opposite of
this product's purpose.

An empty diff is worse: it hashes to one fixed value, so every net-zero branch
matched every `--allow-empty` CI-retrigger commit, and each other.

Patch-id is therefore only a cheap **index**. Every hit is confirmed by
`exact_change_key()` — the same content patch-id keeps (paths, and every
`+`/`-`/context line) with the whitespace stripping removed, and without the
blob OIDs and hunk line numbers that legitimately differ when the target
branch moved. Empty diffs are refused outright. The confirmation runs once per
hit, not once per candidate, so it costs nothing measurable.

`a_whitespace_only_difference_is_not_a_squash_merge` and
`a_net_zero_branch_is_not_squash_merged_by_an_empty_commit` in
`tests/squash_links_test.rs` pin both, each asserting the patch-id collision
as a precondition so they cannot silently stop testing anything.

A hit downgrades the blocker to a `DeleteSquashMerged` **warning** naming the
squash commit. The warning is not decoration: the graph shows the branch as a
dead end, so "safe to delete" looks wrong until you know which commit already
carries the change.

Bounds, all in the direction of refusing to unblock:

- `SQUASH_SCAN_LIMIT = 500` commits reaching a diff, plus `SQUASH_WALK_LIMIT
  = 20_000` on the traversal itself — the first alone bounds only the commits
  that pass the filters, so a filter rejecting most of history let the walk run
  to the root for free.
- **No timestamp prune.** There was one ("a squash lands after the work it
  replays"), and it was wrong: an amend, a rebase, or clock skew between
  machines pushes the tip's committer time past the squash commit and the
  branch silently became undeletable again. `squash_merged_as` gets the same
  guarantee structurally from `walk.hide(base)`; the whole-repo scan, which
  cannot hide a single base, uses `!graph_descendant_of(base, squash)` — the
  reachability fact the timestamp was only approximating.
- Merge commits are skipped: they have no single "change they introduce".
- Every error is swallowed into `None`. An inconclusive answer must read as
  "not merged".

## Consequences

- This is **not** a force delete. The delete only unblocks when a byte-exact
  copy of the change is already in HEAD, so nothing can be lost. ADR-0014 stands:
  a genuinely unmerged branch is still blocked, and `-D` still does not exist.
  `a_genuinely_unmerged_branch_is_still_blocked` in `tests/delete_branch_test.rs`
  is the guard against this becoming a back door.
- Rebase-and-merge is not detected: it replays each commit separately, so no
  combined patch-id matches. Detecting it means matching every commit of
  `base..tip` individually (N×M diffs). Marked `ponytail:` in the source; add
  it if anyone asks.
- `MergedBranchStatus::SquashMergedLikely` in branch cleanup (ADR-0128) still
  uses the `[gone]` upstream heuristic. It could now use this local proof, at a
  cost of N branches × M commits of diffs. Deliberately not done.
