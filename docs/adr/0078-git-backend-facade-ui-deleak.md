# ADR-0078: Path/Handle `Backend` Façade as the UI De-leak Mechanism

- Status: Accepted / Date: 2026-06-14
- Context: v1.0 re-architecture, migration step S4. Implements the invariant of
  ADR-0072 (UI must not call git2) as a staged strangler step ahead of the full
  worker-thread `GitBackend` of ADR-0073.

## Decision

`kagi-git` (currently `src/git/`) gains a **`Backend` struct** that owns a
`git2::Repository` internally and exposes one thin method per git operation the UI
performs. The UI obtains a `Backend` via `Backend::open(path)` and calls
`backend.plan_checkout(..)`, `backend.snapshot()`, etc. — it never names
`git2::Repository` again.

- Each method **delegates** to the existing free functions in `ops.rs` /
  `snapshot.rs` / `status.rs` / `diff*.rs` / `conflicts.rs` / `staging.rs`
  (`fn snapshot(&self) -> ... { snapshot::snapshot(&self.repo) }`, etc.).
  Behavior is byte-identical; this is a wrapping layer, not a rewrite.
- The few inline git2 reads in the UI (`repo.head()`, `repo.workdir()`,
  `repo.stash_foreach()`) get dedicated `Backend` methods too.
- After the UI is migrated, `git2`/`git2::Repository` no longer appears in the UI
  layer; a CI grep gate enforces it (ADR-0072 Consequences).

## なぜ

- The headline invariant (UI ≠ git2) is the product thesis as a type boundary. The
  full end-state (ADR-0073: `GitBackend` trait + per-session worker thread + async
  `Operation` enum) is a large change to the async model; doing it big-bang against
  a 16.7k-LOC UI with ~82 `Repository::open` sites + ~50 `plan_/execute_` calls is
  risky. A method-bearing façade removes git2 from the UI **now**, with
  behavior-identical delegation, and is the natural seam the worker-thread upgrade
  later slots behind (the `Backend` API is already `GitBackend`-shaped).
- It keeps every step green: the façade is purely additive until call sites move,
  and call sites move mechanically (`Repository::open(p)?` → `Backend::open(p)?`;
  `plan_x(&repo, a)` → `backend.plan_x(a)`).

## 代替案

1. ~70 path-based free functions (`plan_checkout_at(path, ..)`), re-opening per call.
2. **本決定**: a `Backend` handle that opens once and offers methods.
3. Jump straight to the ADR-0073 worker-thread async `GitBackend` trait.

## 捨てた案

- 案1: re-opens the repo on every call (the current wasteful pattern) and sprays
  `_at` twins across the API. The handle form is cleaner and opens once per use.
- 案3: too big a leap to do safely in one move; the async/worker change is
  orthogonal to de-leaking and is best layered on *after* the UI talks to a handle.
  Deferred to S3b, not abandoned.

## 将来の負債 / リスク

- The façade opens the repo on the foreground when `Backend::open` is called from a
  UI closure; today the UI already does this inside `background_spawn`, so behavior
  is unchanged. The worker-thread move (ADR-0073, S3b) replaces this with a single
  long-lived `Repository` per session.
- The `Backend` method list must track the ops surface; new operations add a method.
  Acceptable — it is the one intended choke point.
- `OperationPlan` still carries `pub(crate)` plan-time fields; when the
  OperationController (ADR-0075, S5) lands, plan construction + preflight move behind
  the controller and the façade's `plan_/preflight_/execute_` methods fold into
  `request(Operation)`.

## Consequences

- `kagi-ui` becomes git2-free → it can later be split into its own crate with no
  git2 dependency (ADR-0072), making the invariant a compile error.
- CI gains `! grep -rE 'git2::' src/ui` (and later `crates/kagi-ui/src`).
