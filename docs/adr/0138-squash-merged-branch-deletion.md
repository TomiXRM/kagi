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

A hit downgrades the blocker to a `DeleteSquashMerged` **warning** naming the
squash commit. The warning is not decoration: the graph shows the branch as a
dead end, so "safe to delete" looks wrong until you know which commit already
carries the change.

Bounds, all in the direction of refusing to unblock:

- `SQUASH_SCAN_LIMIT = 500` commits, and commits older than the branch tip are
  skipped — a squash lands *after* the work it replays.
- Merge commits are skipped: they have no single "change they introduce".
- Every error is swallowed into `None`. An inconclusive answer must read as
  "not merged".

## Consequences

- This is **not** a force delete. The delete only unblocks when the identical
  change is provably already in HEAD, so nothing can be lost. ADR-0014 stands:
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
