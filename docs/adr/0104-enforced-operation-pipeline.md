# ADR-0104: Enforced operation pipeline in `Backend::run`

- Status: Accepted (Phase 1 — scaffold only; caller migration deferred)
- Date: 2026-06-20
- Implements: `/docs/git-safety-checklist.md` global invariant

> **Honest status note (added after cross-review):** This ADR describes the
> *target*. As of this sprint, `Backend::run(op, plan)` exists and enforces
> preflight, **but has zero callers in the binary** — every mutation path
> (UI `start_*`, headless `KAGI_*`, tests) still calls `execute_*` directly.
> Routing those callers through `run` is the Phase 2 migration (blocked on
> ADR-0107 RepoSession, which is the natural owner of a `run` call). Until
> that migration lands, the safety invariant is still a UI-layer convention,
> NOT a backend guarantee. The concrete safety wins from this sprint are
> ADR-0105 (dirty-merge block), ADR-0106 (atomic conflict save), T-REARCH-014
> (two-stage Discard), and T-REARCH-015 (stash-pop preflight) — those are
> wired into real call paths.

## Context

The product's central thesis is that every write operation flows through
`plan → confirm → preflight → execute → verify → oplog`. The codebase
documentation (`AGENTS.md`, `docs/rearch/architecture.md` §2.2) presents this
as an enforced guarantee.

The 2026-06-20 codebase review (`/docs/codebase-review.md` §4 Finding 2,
verified by reading `src/git/backend.rs:304-427`) found that **this is a UI
convention, not a backend guarantee.** `Backend::execute(op)` dispatches
Checkout, CherryPick, Merge, Revert, Pull, Push, Undo, Amend, StashApply,
StashPop, and StashPush straight to `execute_*` with **no `preflight_check`,
no `verify`, and no `append_oplog`.** Only `DeleteBranch` and `Discard`
re-plan inline.

Consequence: any non-UI caller — the headless `KAGI_*` env-var harness
(48 hooks, 26 raw `git2::Repository::open` in `src/headless.rs`), tests, or
future automation — silently bypasses preflight, verify, and the oplog. The
oplog (the advertised recovery mechanism) will not contain entries for the
majority of writes done via this path. This directly falsifies the safety
thesis.

## Decision (target)

Introduce a single enforced entry point for every mutating operation:

```rust
impl Backend {
    /// The only path that mutates the repository. Enforces the full pipeline.
    pub fn run(
        &mut self,
        op: &Operation,
        plan: &OperationPlan,
    ) -> Result<OperationOutcome, GitError> {
        preflight(op, plan)?;          // HEAD/stash-count unchanged since plan
        let outcome = dispatch_execute(op, plan)?;  // existing execute_* fns
        verify(op, plan, &outcome)?;   // per-op post-condition
        append_oplog(op, plan, &outcome)?;          // recovery handle recorded
        Ok(outcome)
    }
}
```

The target is: every mutating caller routes through `run`, the `execute_*`
methods become `pub(crate)`, and the `verify` + `append_oplog` steps move
into `run` (today the UI's `record_op` does oplog; that responsibility stays
with the UI until the worker-thread consolidation, because it also drives
toasts/footer which are UI concerns).

### What this sprint delivered (scaffold)

- `Backend::run(op, plan)` exists and runs `preflight_check` (or
  `preflight_check_stash` for stash apply/pop) before dispatch.
- The legacy `execute(op)` is `#[deprecated]` and forwards to `run` via a
  synthesized plan (preflight still runs).
- `run` is the future single entry point for ADR-0107 `RepoSession`.

### What this sprint did NOT deliver (deferred to Phase 2)

- **No caller migrated to `run`.** UI/headless/tests still call `execute_*`
  directly. The safety guarantee is therefore NOT yet a backend guarantee.
- **`execute_*` methods remain `pub`.** They are not yet forced through `run`.
- **`verify` and `append_oplog` are not in `run`** — oplog stays a UI
  responsibility (`record_op`) because it also drives toasts/footer.

### Per-op pipeline table (target — with delivery status)

| Op | preflight in run() | wired to real callers? |
|---|---|---|
| checkout (branch/commit) | implemented in `run` | no (callers bypass) |
| cherry-pick / revert | implemented in `run` | no |
| merge (all kinds) | implemented in `run` + dirty-tree block via ADR-0105 (wired in plan) | no |
| discard | implemented in `run` + existing verify/oplog | partial (B3: armed path needs preflight — fixed in this follow-up) |
| stash apply/pop/drop | `preflight_check_stash` in `run` | **yes** (T-REARCH-015) |
| pull / push | implemented in `run` | no |
| undo / amend | implemented in `run` | no |

Legend: "implemented in `run`" = the code exists but no caller uses `run()`
yet; the last column reflects whether real call paths actually go through
preflight.

## Consequences

- **`run()` is scaffolding.** It enforces preflight *if called*, but is not
  yet the single enforced entry point. The honest safety wins from this sprint
  are the four call-path changes (ADR-0105/0106, T-REARCH-014/015).
- **`#[deprecated]` on `execute(op)`** ensures any NEW code uses `run`, not
  the shortcut — a forward-looking guard.
- The worker-thread consolidation (ADR-0073 long-term; deferred) will make
  `run()` the channel message the worker thread receives.

## Rollout (target)

See `/docs/refactor-plan.md` Phase 1. The caller migration (UI `start_*` →
`run`, headless `KAGI_*` → `run`) is a Phase 2 task blocked on ADR-0107
RepoSession (which is the natural owner). Each `execute_*` should become
`pub(crate)` only after all callers route through `run`.

## What this does NOT change

- The `Operation` enum and `OperationPlan` struct (still built by UI/tests).
- The git2-free UI invariant (CI gate).
- The forbidden-op policy (no `reset --hard`, `push --force`, `git clean`,
  `--force-with-lease`, `unsafe`).
- The `[kagi] …` log contract wording (existing lines untouched).
