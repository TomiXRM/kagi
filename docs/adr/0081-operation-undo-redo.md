# ADR-0081: Operation Undo / Redo (GitKraken-style, ref-move based)

- Status: Accepted / Date: 2026-06-14
- Context: New feature for release. Generalizes the existing single-shot
  `undo_commit` (ADR-0041, soft) and the oplog (ADR-0074) + ORIG_HEAD/reflog
  machinery (used by conflict abort) into an Undo/Redo history.

## Decision

Provide **Undo and Redo of ref-moving operations** (commit, merge, and the other
HEAD-moving ops) like GitKraken — undo after a commit/merge, then redo — while keeping
Kagi's safety thesis. **No commit is ever destroyed**: undo/redo only move a branch ref
between SHAs that remain reachable via the reflog/ODB, and every undo/redo goes through
the `plan → confirm → preflight → execute → verify → log` pipeline with a preview.

### Model
- An in-session **`OperationHistory`**: a `Vec<HistoryEntry>` + a cursor.
  `HistoryEntry { kind, branch, before: CommitId, after: CommitId, summary }` is pushed
  after each successful ref-moving `execute_*` (commit, merge, cherry-pick, revert,
  amend, undo-commit…). The `before`/`after` come from the snapshot the pipeline
  already captures (the oplog records them — ADR-0074).
- **Undo** moves `branch` from `after` back to `before`; **Redo** moves it from
  `before` forward to `after`. Cursor tracks position; a NEW operation truncates the
  redo tail (standard undo-stack semantics).
- The ref move is a **safe, soft-style operation**: update the branch ref + reset the
  index to the target, **without discarding working-tree changes** (mixed/soft, never
  `--hard`; ADR-0023 forbids hard reset). The dropped commits stay in the reflog.

### Layering (reuse the pipeline; no Git in the view)
1. **domain**: `HistoryEntry`, `OperationKind`, the undo/redo stack logic (pure;
   push/undo/redo/cursor) — unit-testable without a repo.
2. **git-backend** (`Backend`): `plan_undo(entry)` / `plan_redo(entry)` build an
   `OperationPlan` (current HEAD → target, what is preserved, blockers); `execute_undo`
   / `execute_redo` perform the ref move via libgit2 `reference.set_target` +
   index/worktree reconcile (soft), then verify HEAD == target.
3. **app/ui**: the history lives in app state; toolbar **Undo** + **Redo** buttons
   (enabled per cursor) open the plan modal; on confirm the controller runs the op and
   updates the cursor. The view never calls git directly (ADR-0078).

### MVP scope
- Undo/Redo of **commit and merge** (the user's examples) — plus any other recorded
  ref-moving op that reduces to a branch-ref move. Soft (working tree preserved).
- Out of MVP: undo of stash/discard/checkout (non-ref-move or destructive-restore),
  cross-session history persistence, partial/selective undo, undo across branch switch.

## なぜ
- Direct, reversible history is a core GitKraken affordance users expect. Implemented as
  reflog-backed ref moves through the existing safety pipeline, it adds the capability
  **without** any destructive command (no data loss; everything recoverable) — fully
  consistent with Kagi's "predict before acting / nothing silently lost" thesis.
- Reuses what exists: ORIG_HEAD/reflog handling, the oplog before/after states, and the
  soft `undo_commit` are exactly the primitives; this generalizes them.

## 代替案 / 捨てた案
- **`git reset --hard` based undo** — rejected (data loss; violates ADR-0023).
- **Re-running inverse operations** (e.g. undo merge = `git revert -m1`) — rejected for
  MVP: revert creates new commits (not a true undo) and changes history semantics; the
  ref-move model is the GitKraken behavior and is reversible by redo.
- **Persistent cross-session history** — deferred (reflog is the durable backstop;
  in-session stack covers the asked-for flow).

## 将来の負債 / リスク
- A NEW op after some undos truncates redo (intended). Undoing a commit/merge with
  unrelated uncommitted changes: keep them (soft) and surface a warning; block only if
  the move would require overwriting tracked changes.
- Branch switches / external git operations invalidate stack entries → entries are
  validated in preflight (target SHA still reachable, branch still at `after`); stale
  entries are skipped with a clear message.
- In-session only for MVP (lost on quit); reflog still allows manual recovery.

## Consequences
- New domain `history` module + `Backend::{plan,execute}_{undo,redo}` + app history
  state + toolbar Redo button (Undo button generalized from undo-commit). oplog records
  undo/redo as operations too.
- ticket: T-UNDOREDO-001.
