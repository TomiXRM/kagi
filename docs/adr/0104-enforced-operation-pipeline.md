# ADR-0104: Enforced operation pipeline in `Backend::run`

- Status: **Accepted (Phase 2 delivered — safety is now a backend guarantee)**
- Date: 2026-06-20
- Implements: `/docs/git-safety-checklist.md` global invariant

> **Status (updated after Phase 2):** `Backend::run(op, plan)` is now the
> enforced entry point for every mutating operation. Every UI `*_blocking` fn
> and every `operations/*` handler routes through `run`. The deprecated
> `execute(op)` bypass is **deleted**. The safety invariant (preflight runs
> before any mutation) is now a backend guarantee, not a UI convention.
>
> Remaining gap: `execute_*` methods are still `pub` (tests call some directly
> via `execute_undo`/`execute_redo` for operation-history replay, which is
> structurally outside `Operation`). Making them `pub(crate)` is deferred until
> those test paths are refactored or `Operation` grows an undo/redo variant.

## Context

The product's central thesis is that every write operation flows through
`plan → confirm → preflight → execute → verify → oplog`. The codebase
documentation (`AGENTS.md`, `docs/rearch/architecture.md` §2.2) presents this
as an enforced guarantee.

The 2026-06-20 codebase review (`/docs/codebase-review.md` §4 Finding 2,
verified by reading `src/git/backend.rs:304-427`) found that **this was a UI
convention, not a backend guarantee.** `Backend::execute(op)` dispatched
Checkout, CherryPick, Merge, Revert, Pull, Push, Undo, Amend, StashApply,
StashPop, and StashPush straight to `execute_*` with **no `preflight_check`,
no `verify`, and no `append_oplog`.** Only `DeleteBranch` and `Discard`
re-planned inline.

Consequence: any non-UI caller — the headless `KAGI_*` env-var harness,
tests, or future automation — silently bypassed preflight, verify, and the
oplog. The oplog (the advertised recovery mechanism) would not contain
entries for the majority of writes done via this path. This directly
falsified the safety thesis.

## Decision (delivered)

A single enforced entry point for every mutating operation:

```rust
impl Backend {
    /// The enforced path that mutates the repository. Runs preflight, then
    /// dispatches. The oplog/toast/footer recording stays with the UI's
    /// `record_op`; `run`'s job is the safety gate + dispatch.
    pub fn run(
        &mut self,
        op: &Operation,
        plan: &OperationPlan,
    ) -> Result<OperationOutcome, GitError> {
        // Preflight: refuse if HEAD/stash-count changed since the plan.
        match op {
            Operation::StashApply { .. } | Operation::StashPop { .. } => {
                self.preflight_check_stash(plan, plan.stash_count_at_plan())?;
            }
            _ => self.preflight_check(plan)?,
        }
        // Dispatch (behaviour-identical to the former execute_* calls).
        match op { /* per-op dispatch to execute_* */ }
    }
}
```

### What Phase 1 delivered (scaffold)

- `Backend::run(op, plan)` exists and runs preflight before dispatch.
- `Operation` enum gained `MergeCommit` (conflict-resolution finalize).

### What Phase 2 delivered (the guarantee)

- **Every mutating UI path routes through `run`:** all `*_blocking` fns in
  `ui/mod.rs` (checkout, merge, cherry-pick, revert, commit, pull, push,
  amend, undo, stash push/pop, discard, branch create/delete/rename/set-
  upstream, worktree create/open, switch-to-latest, checkout-tracking,
  branch-plan pull-ff/push) and every `operations/*` handler
  (branch, stash, checkout, history, commit).
- **The deprecated `execute(op)` bypass is deleted.** No caller can skip
  preflight via the old shortcut.
- Each site's separate `preflight_check` call is removed where `run()` now
  does it (dedup).

### Per-op pipeline status

| Op | preflight in run() | wired to real callers? |
|---|---|---|
| checkout (branch/commit) | yes | **yes** |
| cherry-pick / revert | yes | **yes** |
| merge (all kinds) | yes + dirty-tree block via ADR-0105 | **yes** |
| discard | yes + existing verify/oplog + stale-plan guard (B3) | **yes** |
| stash apply/pop | yes (`preflight_check_stash`) | **yes** |
| stash drop | `preflight_check_stash` inline (drop not in `Operation` yet) | **yes** |
| pull / push | yes | **yes** |
| undo / amend | yes | **yes** |
| merge-commit | yes (plan synthesized via `plan_merge_commit`) | **yes** |
| create/rename/delete branch, set-upstream, worktree | yes | **yes** |
| conflict save/continue/abort/skip | n/a (not `Operation` variants; resolution save is atomic per ADR-0106) | n/a |
| undo/redo replay (operation history) | n/a (`execute_undo`/`execute_redo`, not `Operation`) | test-only |

## Consequences

- **Safety is a backend guarantee.** No UI/headless caller can skip preflight:
  `run()` is the only path, and the old `execute(op)` shortcut is gone.
- **`execute_*` methods remain `pub`** for now: tests use
  `execute_undo`/`execute_redo` for operation-history replay (structurally
  outside `Operation`). When those migrate to an `Operation` variant (or the
  test harness is refactored), `execute_*` becomes `pub(crate)`.
- **The oplog/toast/footer recording stays with the UI's `record_op`**, not
  `run`, because those are UI concerns (the worker-thread consolidation,
  ADR-0073, may fold them in later).
- The worker-thread consolidation (ADR-0073 long-term) will make `run()` the
  channel message the worker thread receives.

## What this does NOT change

- The `Operation` enum and `OperationPlan` struct (still built by UI/tests).
- The git2-free UI invariant (CI gate).
- The forbidden-op policy (no `reset --hard`, `push --force`, `git clean`,
  `--force-with-lease`, `unsafe`).
- The `[kagi] …` log contract wording (existing lines untouched).
