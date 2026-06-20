# ADR-0105: Block merge on dirty working tree

- Status: Accepted
- Date: 2026-06-20
- Implements: `/docs/git-safety-checklist.md` §5

## Context

`plan_cherry_pick` and `plan_revert` (in `src/git/ops/cherry_revert.rs:84-104`)
emit a **blocker** when the working tree has staged or unstaged tracked changes
— the operation cannot proceed until the user commits, stashes, or discards.

`plan_merge_branch` (`src/git/ops/merge.rs:26`) only emits a **warning** via
`merge_dirty_warnings`. `execute_merge_into_conflict` then runs `repo.merge()`
with `checkout_opts.safe()` on a possibly-dirty tree, writing conflict markers
*into the user's uncommitted files*. The `ORIG_HEAD` rollback (`merge.rs:393-400`)
restores the **ref**, not the working tree.

This is an inconsistency and the single most dangerous git path in the
codebase (`/docs/codebase-review.md` §4 Finding 11):

> User has unstaged edits to `auth.rs`. Clicks Merge on a branch whose changes
> also touch `auth.rs`. Plan shows a yellow warning. On confirm, conflict
> markers are written *into the user's unsaved `auth.rs` edits*. `git merge
> --abort` would discard both the merge AND the user's pre-merge edits.

Merge is strictly more dangerous than cherry-pick here because it writes
conflict markers to disk; cherry-pick uses the in-memory variant and only
syncs on success.

## Decision

`plan_merge_branch` and `plan_merge_into_conflict` emit a **blocker** (red)
when `!status.staged.is_empty() || !status.unstaged.is_empty()` — mirroring
the cherry-pick rule. Untracked-only changes remain a warning (they don't
participate in the merge).

Specifically: the real-merge and into-conflict paths are **blocked** on a
dirty tracked tree. The fast-forward path is unaffected (FF never touches the
WT beyond a safe checkout).

The user must commit, stash, or discard before merging. The plan's recovery
text is updated to point at stash as the cleanest rollback point.

## Consequences

- Merge safety now matches cherry-pick/revert safety.
- Users on the existing "merge with dirty tree" workflow (previously only
  warned) will see a blocker. This is intentional — the prior behavior risked
  interleaving conflict markers with unsaved edits.
- Recovery text gains a `git merge --abort` mention for the real-merge path
  (the only safe rollback once a merge has started).
- This rule composes cleanly with ADR-0104's `preflight_check` (the dirty-tree
  state is re-verified at execute time).

## Rollout

One commit in `src/git/ops/merge.rs`. New test: dirty WT + merge plan →
blocker present, Execute hidden.
