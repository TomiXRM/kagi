# ADR-0107: RepoSession — one Backend owner per repo tab

- Status: **Accepted (Phase 3 foundation delivered; read-path migration ongoing)**
- Date: 2026-06-20
- Implements: `/docs/refactor-plan.md` Phase 2 Step 2.1
- Prerequisite for: ADR-0073 worker thread; Phase 3 perf caching

> **Status (Phase 3 start):** `RepoSession` (`Rc<Backend>` wrapper) is
> introduced and wired into `KagiApp` (same lifecycle as `repo_path`). 11
> read-path `Backend::open` sites in `ui/mod.rs` (diff, wip, compare,
> file-history) now use `session.backend()`. ~85 sites remain (32 read-path
> in `operations/*` + 53 write/blocking paths that correctly re-open because
> `run()`/`snapshot()` need `&mut self` and the session is `Rc`, not
> `Arc+Mutex`). The migration is incremental — each read-path site is a
> one-line `Backend::open` → `session.backend()` swap with no behavior change.

## Context

The 2026-06-20 review (`/docs/codebase-review.md` §3; `/docs/performance-review.md`
§3.1, verified by grep) found **132 `Backend::open` call sites in `src/`**
(94 in `src/ui/` alone). Each `Backend::open` (`src/git/backend.rs:22-47`)
re-reads config, walks `.git` resolution, and allocates pools. Trivial read
paths re-open the repo on every interaction: `wip_diffstat`
(`src/ui/mod.rs:3907`), avatar coord resolution (`avatar_fetch.rs:148`),
file-history diff (`mod.rs:3242`), per-file diff open (`mod.rs:3523`).

There is no `RepoSession`. The architecture doc
(`docs/rearch/architecture.md` §2.3) describes a `RepoSession` per tab that
owns the backend handle, but it was never built — the migration README's S5
step is still pending.

This is the foundational blocker for: diff/snapshot/graph-layout caching
(no owner to cache on), the worker-thread consolidation (S3b, ADR-0073), and
the entity decomposition (no clean entity boundary for the repo).

## Decision

Introduce `RepoSession` as the single owner of a `Backend` for a repository
tab:

```rust
// src/git/session.rs (new)
pub struct RepoSession {
    backend: Backend,           // opened once, held for the tab lifetime
    path: PathBuf,
    // Phase 3 will add: snapshot cache, diff cache, watcher handle, worker tx
}

impl RepoSession {
    pub fn open(path: &Path) -> Result<Self, GitError>;
    pub fn path(&self) -> &Path;
    pub fn backend(&self) -> &Backend;          // the only accessor
    pub fn run(&mut self, op: &Operation, plan: &OperationPlan)
        -> Result<OperationOutcome, GitError>;  // delegates to Backend::run (ADR-0104)
}
```

`TabViewState` gains `session: Rc<RepoSession>` (single-thread for now; `Arc`
once the worker thread lands). The 94 `Backend::open` sites in `src/ui/`
migrate file-by-file to read `self.active_view.session.backend()`.

### Why `Rc`, not `Arc`, today

`git2::Repository` is `Send` but `!Sync`. A worker thread will need `Arc` +
an mpsc channel (ADR-0073). For this sprint, the session is foreground-only
and `Rc` suffices — it keeps the migration mechanical and avoids the worker
thread's complexity. The `Rc → Arc` swap is a one-line change when the worker
lands, and the session is the only place it happens.

### Why not delete `Backend` entirely

ADR-0104 makes `Backend::run` the enforced pipeline owner — `Backend` now
earns its existence (it was previously a 112-method 1:1 delegator, per
codebase-review §7 Finding 2). `RepoSession` owns a `Backend`; the two are
complementary: session = lifecycle + cache + (future) worker, backend =
pipeline + git2.

## Consequences

- **132 `Backend::open` sites collapse to one per tab.** Each interaction no
  longer pays 1–5 ms of repo-open overhead. Verified target: `grep -rn
  'Backend::open' src/ui/` → 0 after migration (tests/headless retain their
  own opens until they migrate to sessions too).
- **The cache boundary is now obvious.** Phase 3 perf tickets (diff cache,
  snapshot cache, graph layout cache) hang off `RepoSession` fields.
- **Tab switch becomes a session swap.** No re-open on tab change.
- Migration is mechanical but wide (94 sites). Done file-by-file in the same
  commit shape as the existing `operations/` split — each commit must keep
  `cargo test --workspace` green.
- The `Backend::repo()` escape hatch (already dead, 0 callers — codebase-review
  §7 Finding 1) stays deleted; `RepoSession` exposes only `backend()` which
  exposes only the typed methods, never the raw `git2::Repository`.

## Rollout

See `/docs/refactor-plan.md` Step 2.1. Commit series:
1. Add `src/git/session.rs` + `RepoSession` (additive, no callers yet).
2. Add `session: Rc<RepoSession>` to `TabViewState` + `build_tab_view`.
3. Migrate `src/ui/` files one at a time: `operations/*.rs`, then `mod.rs`
   read paths, then `avatar_fetch.rs`. Each commit: `grep Backend::open
   src/ui/<file>` → 0.

## What this does NOT change (this sprint)

- No worker thread (ADR-0073 deferred — needs careful `Send`/channel design).
- No child `Entity<T>` decomposition of `KagiApp` (Phase 5, deferred).
- `Backend`'s method count (the delegators shrink later, not in this sprint).
