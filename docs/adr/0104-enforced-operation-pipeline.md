# ADR-0104: Enforced operation pipeline in `Backend::run`

- Status: Accepted
- Date: 2026-06-20
- Supersedes: ADR-0073 (GitBackend trait — partial; this ADR makes the
  pipeline a backend guarantee rather than a UI convention)
- Implements: `/docs/git-safety-checklist.md` global invariant

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

## Decision

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

Every `execute_*` function changes signature to take `&OperationPlan` and
calls `preflight_*` (HEAD-moved / stash-count) as its **first line**. The
`Backend::execute(op)` entry point is removed; all callers (UI `start_*`
handlers, headless `KAGI_*` hooks, tests) migrate to `run(op, plan)`.

### Per-op pipeline table

| Op | preflight | verify | oplog |
|---|---|---|---|
| checkout (branch/commit) | `preflight_check` | re-read HEAD == predicted | yes |
| create branch | `preflight_check` | branch ref exists | yes |
| create worktree | `preflight_check` | worktree dir + branch ref exist | yes |
| delete branch | `preflight_check` | ref gone | yes (tip SHA) |
| discard | `preflight_check` | status re-read | yes (backup blobs) |
| stash push | `preflight_check` | stash list grew by 1 | yes |
| stash apply | `preflight_check_stash` | index/WT changed | yes |
| stash pop | `preflight_check_stash` | stash entry gone on success | yes |
| stash drop | `preflight_check_stash` | ref gone | yes (stash reflog id) |
| cherry-pick | `preflight_check` | new HEAD == predicted commit | yes |
| revert | `preflight_check` | new HEAD == predicted commit | yes |
| merge (ff) | `preflight_check` | HEAD == upstream tip | yes |
| merge (real) | `preflight_check` | MERGE_HEAD or new merge commit | yes |
| merge (into-conflict) | `preflight_check` + dirty-tree block | conflict session present | yes |
| pull | `preflight_check` | HEAD advanced or merge commit | yes |
| push | `preflight_check` | remote tip advanced (CLI result) | yes |
| undo commit | `preflight_check` + pushed re-check | HEAD moved back, WT/index untouched | yes (old HEAD SHA) |
| amend | `preflight_check` + pushed re-check | HEAD SHA changed | yes (old HEAD SHA) |
| conflict continue | `preflight_check` | MERGE_HEAD gone, new commit | yes |

## Consequences

- **Safety becomes a backend guarantee.** No caller can skip preflight, verify,
  or oplog. This closes codebase-review §4 Findings 1, 2, 3, 6, 12–16, 27 in
  one change.
- **Oplog coverage becomes complete.** Every mutating op is recoverable.
- **Signature ripple.** ~12 `execute_*` signatures change to take
  `&OperationPlan`. Callers in `src/ui/operations/*` (each `start_*` already
  builds a plan and shows a confirm modal — the change is the execute call),
  `src/headless.rs` (each `KAGI_*` hook must `plan_*` first and refuse if
  `!plan.blockers.is_empty()`), and `tests/` must migrate.
- **`Operation` enum stays.** The UI continues to build `Operation` values to
  *describe* work; `run()` enforces *doing* it. The UI/git2 separation
  invariant is unchanged.
- **`Backend` begins to earn its existence** as a facade — it is no longer a
  112-method 1:1 delegator but the single owner of the pipeline.
- The worker-thread consolidation (ADR-0073 long-term; deferred) will make
  `run()` the channel message the worker thread receives.

## Rollout

See `/docs/refactor-plan.md` Phase 1. Each `execute_*` migrates in its own
commit so the blast radius stays bounded. Tests are added per op: build a
plan, mutate the repo (e.g. create a commit), call `run()` → must return a
preflight error, not mutate.

## What this does NOT change

- The `Operation` enum and `OperationPlan` struct (still built by UI/tests).
- The git2-free UI invariant (CI gate).
- The forbidden-op policy (no `reset --hard`, `push --force`, `git clean`,
  `--force-with-lease`, `unsafe`).
- The `[kagi] …` log contract wording (existing lines untouched).
