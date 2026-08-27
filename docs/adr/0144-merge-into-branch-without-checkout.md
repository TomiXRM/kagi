# ADR-0144: Merging into a branch without checking it out

- Status: Accepted
- Date: 2026-08-27
- Closes: ADR-0079's deferred "drop onto an arbitrary branch label"

## Context

ADR-0079 shipped drag-and-drop merge with the drop target restricted to the
**current** branch, and deferred branch→branch drops with one line: it "would
require checking out B first or a detached merge".

Checking B out is the wrong trade. It moves the user's working tree twice for
an operation that has nothing to do with their working tree, and it fails
outright when the tree is dirty — the state people are usually in when they
reach for this.

The second option turns out to need almost no new machinery.
`execute_merge_branch` already computes the merge **in memory**
(`merge_commits` → `write_tree_to` → `commit`); the only calls that touch the
working tree are the `checkout_tree` and `set_head` at the end, and those exist
solely because the destination *is* the checked-out branch.

## Decision

`ops/merge_into.rs` merges `source` into `target` by computing the merge in
memory and moving `refs/heads/<target>`. HEAD, the index and the files on disk
are not touched.

- **Its own module, not an arm inside `ops/merge.rs`.** The two differ in what
  they may touch, and that difference is the whole safety argument; a shared
  function with a "don't check out" flag would put both behaviours one boolean
  apart.
- **A conflicting merge is a blocker, not an entry into Conflict Mode.**
  Conflicts are resolved in the working tree, and the working tree belongs to
  the current branch. The blocker says so and names the fix (check `target`
  out). This is the one thing the off-branch path cannot do, and it is better
  to refuse than to half-apply.
- **A target checked out in a linked worktree is a blocker.** Moving the ref
  would leave that worktree's index and files describing a commit its HEAD no
  longer points at — a corruption the user would meet later, somewhere else,
  with no connection to what caused it. Detected with the existing
  `worktree_checkout_of`.
- **A remote-tracking chip is a drop target too**, resolving to the local
  branch of that name — created at the remote tip, tracking it, if it does not
  exist yet. Refusing the drop was the first implementation and it was wrong:
  the graph is where the user works, and half its branch chips being inert is
  not a safety property, just a gap. Nothing is ever pushed; the remote ref is
  read, never written.
  - When the local branch **already exists it wins**, even if it has moved past
    the remote. Starting from the remote tip instead would leave the local
    commits out of the merge, which is a way to lose work, so the plan warns
    that the two differ rather than silently picking one.
- **Dropping onto the current branch still routes to `plan_merge_branch`.**
  That path can enter Conflict Mode; sending it here would silently downgrade
  a resolvable conflict to a refusal.
- **Recovery names the branch, not the reflog.** HEAD never moved, so
  `git reflog` does not show this operation. The plan gives
  `git branch -f <target> <previous-sha>` and points at `git reflog <target>`.
- Execution goes through `Backend::run` like every other write
  (`Operation::MergeIntoBranch`), so preflight is enforced and the oplog
  records it.

## Consequences

- ADR-0079's "future work: branch→branch DnD (with target checkout)" is
  delivered **without** the target checkout it assumed.
- The gesture is now asymmetric in a way worth knowing: dropping onto the
  current branch can start a conflict resolution, dropping onto any other
  branch cannot. The blocker text carries that difference.
- `MergePlanModal` gains an `off_branch` flag rather than a second modal — its
  confirm label already read `Merge <source> into <destination>`, which is
  correct for both.
- The tests assert what did **not** move (HEAD, the index, the working tree,
  the source branch, files on disk) as well as what did. A merge that quietly
  checked the target out would satisfy "the target advanced" just as well; the
  mutation that adds `checkout_tree` back fails two of them.
