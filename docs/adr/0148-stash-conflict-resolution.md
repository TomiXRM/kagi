# ADR-0148 — Stash-apply/pop conflict resolution (Conflict Mode)

Status: Accepted
Issue: #309 (follow-up to #280 `ConflictedStashKept`)

## Context

Since #280, a conflicted `stash pop`/`apply` is allowed: the stash is **kept**
(`StashPopOutcome::ConflictedStashKept`) and libgit2 writes the conflict entries
into the index. But unlike merge/rebase/cherry-pick/revert, a stash-apply
conflict leaves `repo.state() == RepositoryState::Clean` — there is **no**
`MERGE_HEAD`/`REBASE_*`/`CHERRY_PICK_HEAD` state file. So
`classify_op` returns `None`, `detect_conflict_session` never fires, and the
user cannot reach the 3-pane Conflict Mode. The conflicted paths show in the
commit panel but there is no GUI action to complete the resolution — the only
path is: resolve markers in the editor → `git add` in the built-in terminal →
manually drop the stash.

## Decision

Route a conflicted stash-apply through the existing Conflict Mode via a new
op kind, with stash-specific `continue`/`abort` semantics (there is no
`ORIG_HEAD`/`MERGE_HEAD` to lean on).

### Detection
`classify_op`'s fallthrough gains: when `state == Clean` **and** the index has
conflict entries (`index.has_conflicts()` / `collect_conflict_files` non-empty),
return `Some(ConflictOp::StashConflict)`. Ordering: the named-state arms
(Merge/Rebase/…) still win; StashConflict is only the `Clean + unmerged index`
case, which the named states never produce.

### `ConflictOp::StashConflict`
- `slug() == "stash"`.
- **No skip** (not a sequencer). The skip predicate becomes an explicit
  allow-list (`Rebase | CherryPick | Revert`) instead of "anything but Merge",
  so StashConflict (and Merge) correctly disallow skip.

### Complete (the "continue" action)
A stash apply is **not** a commit — its result is ordinary working-tree changes.
So completion does **not** create a merge commit and does **not** shell out to
any `<op> --continue`. It:
1. Stages the resolved paths at stage 0 (`stage_conflict_resolution`, already
   used by merge continue), which removes the unmerged entries → the paths
   become normal staged changes and the repo is commit-able / clean of
   conflicts.
2. Then surfaces a **"drop the kept stash?"** prompt (the stash is still there
   from #280). Dropping is opt-in and uses the existing stash-drop op; declining
   leaves the stash intact. This closes the #280 manual-drop gap.

`execute_conflict_continue` gets a `StashConflict` branch that performs (1) and
returns an outcome that routes the UI to the drop-stash prompt rather than the
commit panel.

### Abort
The pre-apply working tree was clean (stash apply's plan requires a clean tree,
ADR-0046 family), so the pre-apply state is exactly `HEAD`. Abort therefore:
- Checks out `HEAD` for the **conflicted paths only** (pathspec-bounded, mirrors
  the #278 abort discipline — never a repo-wide reset), and clears the unmerged
  index entries for those paths.
- Leaves the **stash intact** (never drops on abort).
- Does **not** touch `ORIG_HEAD` (there is none). The existing
  `execute_conflict_abort` ORIG_HEAD path is not reused; a dedicated
  `execute_stash_conflict_abort` handles this.

### Out of scope
- Partial (per-hunk) stash application beyond what Conflict Mode already offers.
- Applying a stash onto a dirty tree (still blocked at plan time).

## Consequences
- Conflict Mode becomes reachable for the one conflict source that has no git
  state file, using the same 3-pane UI and resolution buffer (incl. the #297
  binary/symlink raw path).
- New surface: `ConflictOp::StashConflict`, a `StashConflict` branch in the
  continue/abort executors, a stash-drop prompt in the UI, and EN+JA strings.
- The `plan_/preflight_/execute_` discipline is preserved: abort/complete are
  execute-side; detection is read-only.

## Acceptance
- Conflicted `stash pop` enters Conflict Mode.
- Completing resolution clears the conflict entries (commit-able) and offers to
  drop the stash.
- Abort restores the clean (pre-apply == HEAD) tree and keeps the stash.
- Mutation-verified tests for detection, complete, and abort.
