# ADR-0073: Repository worker thread

- Status: **Accepted (Phase 1 — infrastructure delivered; write-path migration ongoing)**
- Date: 2026-06-21
- Builds on: ADR-0107 (RepoSession), ADR-0104 (Backend::run)
- Implements: `/docs/performance-review.md` §9.2, `/docs/refactor-plan.md` Phase 5 Step 5.2

## Context

`RepoSession` (ADR-0107) holds a `Rc<Backend>` for read paths — opened once
per tab. But **write paths still re-open the repo on every operation** (32
`Backend::open` sites in `*_blocking` fns), because `run()` needs `&mut self`
and `Rc` is not `Arc+Mutex`.

Each `Backend::open` re-reads config, re-walks `.git` resolution, and allocates
git2 pools (~1-5 ms per call). On a busy session (frequent commits/checkouts)
this is constant overhead on the UI thread's background tasks.

The codebase review (`/docs/performance-review.md` §3.1) identified this as the
foundational blocker: a single `RepoSession` should own one `Backend` for its
entire lifetime, and all operations (read and write) should go through it.

## Decision

### Phase 1 (this PR): Worker thread infrastructure

Introduce a dedicated worker thread per `RepoSession`. The thread owns the
`Backend` exclusively (no `Arc+Mutex` — the thread is the single owner). The
UI sends operations via an `mpsc` channel; the worker executes them and
returns results via a `oneshot` channel.

```rust
// src/git/worker.rs (new)
pub struct RepoWorker {
    tx: mpsc::Sender<WorkRequest>,
    handle: Option<std::thread::JoinHandle<()>>,
}

enum WorkRequest {
    Run { op: Operation, plan: OperationPlan, reply: oneshot::Sender<Result<OperationOutcome, GitError>> },
    Read { f: Box<dyn FnOnce(&Backend) + Send>, },  // arbitrary read closure
    Shutdown,
}

impl RepoWorker {
    pub fn spawn(path: &Path) -> Result<Self, GitError>;
    pub fn run_async(&self, op: Operation, plan: OperationPlan)
        -> impl Future<Output = Result<OperationOutcome, GitError>>;
    pub fn shutdown(&self);  // sends Shutdown, joins thread
}
```

The worker thread:
1. Opens the `Backend` once (`Backend::open(path)`).
2. Loops on `rx.recv()`, executing each `WorkRequest`.
3. For `Run`, calls `backend.run(&op, &plan)` (ADR-0104 enforced pipeline).
4. On `Shutdown`, exits cleanly.

### What Phase 1 delivers

- `RepoWorker` type + `worker.rs` module.
- `RepoSession` gains an `Option<RepoWorker>` field (lazily spawned).
- `RepoSession::run_async(op, plan)` returns a future that sends to the worker
  and awaits the reply.
- The existing synchronous `*_blocking` paths are **unchanged** — they still
  re-open. Migration is incremental (Phase 2).

### What Phase 1 does NOT deliver

- **No caller migrated to `run_async`.** The `*_blocking` fns still call
  `Backend::open` directly. Each migration is a one-site swap from
  `Backend::open → cx.background_spawn → run` to `session.run_async(op, plan)`.
- **`Arc<Mutex<Backend>>` not used.** The worker owns the Backend; the UI never
  touches it directly. This avoids lock contention entirely.

### Why a dedicated thread (not `cx.background_spawn` per op)

`cx.background_spawn` creates a fresh task on GPUI's background executor for
each operation. Each task opens its own `Backend` (the current re-open pattern).
A dedicated worker thread:

1. **Opens once.** The `Backend` (and its `git2::Repository`) lives for the tab
   lifetime — no per-op open.
2. **Serializes mutations.** Git operations are not thread-safe on the same
   repository; the worker's single-threaded receive loop guarantees no two ops
   run concurrently on the same repo.
3. **Simplifies `Send` boundaries.** The `Backend` never crosses threads; only
   `Operation` + `OperationPlan` (both `Send`) cross via the channel.

### Phase 2 (follow-up): migrate callers

Each `*_blocking` fn migrates from:
```rust
let mut repo = Backend::open(&repo_path)?;
repo.run(&op, &plan)?
```
to:
```rust
let outcome = session.run_async(op, plan).await?;
```

The `*_blocking` fns become thin wrappers that spawn the future on the
background executor and block on it (for headless/test paths) or return it
(for UI `cx.spawn` paths).

## Consequences

- **One `Backend::open` per tab** (down from ~85 total open sites). The worker
  owns it; reads use `RepoSession::backend()` (unchanged); writes use
  `RepoWorker::run_async`.
- **No lock contention.** The worker is single-threaded; no `Mutex`.
- **Clean shutdown.** `RepoWorker::shutdown` sends `Shutdown` and joins; the
  thread exits. `Drop` impl does this automatically if forgotten.
- **The worker thread is `Send`-safe.** Only `Operation`/`OperationPlan`
  (both `Send`) cross the channel boundary.
- **Headless path.** Headless constructs `RepoSession` + `RepoWorker` the same
  way; `run_async` works identically (the future can be polled synchronously
  in the headless event loop).

## What this does NOT change

- The git2-free UI invariant (CI gate) — the worker is in `src/git/`, not `src/ui/`.
- The `Backend::run` pipeline (ADR-0104) — the worker calls it.
- The `[kagi] …` log contract — existing lines untouched.
- The forbidden-op policy.
