# 03 — Git Operation Layer (re-architecture research)

> NOTE (2026-06-22): `src/git` was extracted to `crates/kagi-git` in ADR-0115; paths below describe the pre-extraction layout.

> Research sub-agent #3 / Kagi v1.0 re-architecture (PM-led).
> Domain: **Git operation layer** — libgit2 usage, dry-run/preflight/execute/verify
> pipeline, operation log. **This is the most important domain: it carries Kagi's
> safety thesis** (ADR-0004: "the most important value is never to break the local repo").
> Scope: RESEARCH ONLY. Builds on `docs/rearch/inventory.md`, `docs/research/{gitbutler,jj,zed-gpui}-reuse-research.md`.

---

## 1. Kagi 現状 (current state)

### 1.1 The triad pattern — every write op is three flat functions

`src/git/ops.rs` is **6557 LOC of flat free functions**. Every write operation is a
hand-written triad (or pair) over `&git2::Repository`:

```
plan_X(repo, args)        -> Result<OperationPlan, GitError>   // analyse: current/predicted state, warnings, blockers, recovery
preflight_check(repo,plan)-> Result<(), GitError>              // HEAD unchanged since plan?  (shared) 
execute_X(repo, args)     -> Result<Outcome, GitError>         // the single libgit2 mutation
```

~30 operations follow this shape: `checkout`, `checkout_commit`, `create_branch`,
`create_branch_with_checkout`, `create_worktree`, `open_worktree_for_branch`,
`stash_push/apply/pop`, `cherry_pick`, `merge_branch`, `merge_into_conflict`, `revert`,
`pull`, `push`, `pull_branch_ff`, `push_branch`, `set_upstream`, `rename_branch`,
`undo_commit`, `amend`, `delete_branch`, `discard` (+ `plan_commit`/`execute_commit` in
`staging.rs`). The pub-use surface in `src/git/mod.rs:55-116` re-exports the lot.

**Shared types** (`ops.rs:86-147`):
- `StateSummary { head: String, dirty: String }` — one-line, *already stringified* ("branch: main" / "1 staged, 2 modified").
- `OperationPlan { title, current, predicted, warnings: Vec<String>, blockers: Vec<String>, recovery: String, head_at_plan: Head (priv), stash_count_at_plan: usize (priv), preview_files: Vec<FileStatus>, preview_commits: Vec<String>, destructive: bool }`.

`preflight_check` (`ops.rs:464`) is the **one** genuinely shared step: it re-`resolve_head`
and compares to `plan.head_at_plan`, erroring if the repo moved (optimistic-concurrency
guard). `preflight_check_stash` (`ops.rs:1922`) additionally compares `stash_count_at_plan`.

### 1.2 What's good — the safety thesis is *actually implemented*

This is the crown jewel and **must be preserved**:

- **Dirty-WT blockers are real.** `plan_checkout` (`ops.rs:392`) pushes a blocker when
  staged/unstaged changes exist; the UI cannot offer Execute when `blockers` is non-empty
  (ADR-0004 Guarded class). Conflict-state, already-HEAD, missing-branch all blocked too.
- **In-memory dry-run is the differentiator** (ADR-0002 §dry-run, ADR-0005). `plan_cherry_pick`
  / `plan_revert` / `plan_merge_branch` use `cherrypick_commit` / `merge_commit` / `merge_trees`
  **in-memory** to predict conflicts and populate `preview_files` *without touching the working
  tree or leaving CHERRY_PICK/MERGE state* (`ops.rs:1-28` doc comment is explicit). This is the
  single feature that justifies libgit2 over the CLI.
- **Forbidden APIs are structurally absent.** No `reset --hard`, no `git clean`, no force push,
  no `stash_drop` from public API (`stash_drop_internal` is private, called only after a clean
  apply). `CheckoutBuilder::safe()` is the only checkout strategy except the one audited
  `force()` in `execute_discard`.
- **`execute_discard` is the canonical full pipeline** (`ops.rs:6469`, ADR-0046):
  **backup → execute → verify**. (1) write each target's current WT bytes into the ODB via
  `repo.blob()` collecting `path→blob-SHA`; abort the whole op if *any* backup fails; (2)
  `checkout_index(force, update_index(false))` for the named paths only; (3) re-read status
  and verify each target left the unstaged set. The backup blob SHAs are the oplog recovery handle.
- **Outcome types carry recovery handles**: `DiscardOutcome.backups`, `UndoOutcome`, `AmendOutcome`
  (old SHA), `PullOutcome`, `PushOutcome`, `FetchOutcome`.
- **`destructive: bool`** drives ADR-0023 two-stage confirmation (only `discard`/`amend` set it today).

### 1.3 The leakage — UI bypasses ops.rs (Invariant #1 violation)

This is the structural debt the re-architecture exists to repay:

- **`src/ui/mod.rs` opens `git2::Repository::open` 80×** and references **`git2::` 81×**
  *inline*. Other UI files leak too: `tabs.rs`, `commit_panel.rs`, `conflict_view.rs`,
  `commands.rs`, `avatar_fetch.rs` (1 each). The git2 dependency is woven through the view layer.
- Not just opens — **direct libgit2 logic in the UI**: e.g. `ui/mod.rs:3441`
  `repo2.find_branch(&modal.input, git2::BranchType::Local)` (branch-existence checks for
  per-keystroke modal validation); inline `repo.checkout_tree`/`set_head` paths exist alongside
  the `execute_*` functions.
- **Per-keystroke synchronous re-planning** (`ui/mod.rs:1499` comment, `modal_replan_gen`): each
  keystroke in a modal does `Repository::open` + rebuild the plan **synchronously on the UI thread**.
  On a large repo this blocks the frame (snapshot reload is "当面同期", ADR-0019 §reload).
- Operations run inside **GPUI `cx.spawn(async move …)` tasks** (20+ sites: `ui/mod.rs:3156,
  3728, …`). Each task re-opens the repo and calls `plan_*`/`preflight`/`execute_*` itself. There
  is **no dedicated repo-owning worker thread and no OperationController** — the pipeline
  ordering (plan→confirm→preflight→execute→verify→log) is *re-implemented ad hoc at every call
  site* in the god-object. `append_oplog` is called once (`ui/mod.rs:4859`) inside one such task;
  nothing structurally guarantees every op logs.

### 1.4 Network via CLI

`src/git/cli.rs` (`run_git`, 109 LOC) shells out to the system `git` for fetch/pull/push
(ADR-0002 §認証, ADR-0009): shell-bypass (`&[&str]`, never interpolated), `GIT_TERMINAL_PROMPT=0`
+ `LC_ALL=C`, 60s timeout via background-thread + `recv_timeout`. `execute_pull`/`execute_push`
do `run_git(fetch/push)` then finish the merge/FF **in-memory via git2** (no MERGING state).
The git2-vs-CLI split is correct and proven; keep it.

### 1.5 Oplog format

`src/git/oplog.rs` (715 LOC) appends JSON Lines to `$KAGI_LOG_DIR|$HOME/.kagi/operations.jsonl`.
`OpLogEntry { timestamp, op: String, repo: String, before: StateSummary, outcome: OpOutcome }`;
`OpOutcome = Success{after} | Failed{error} | Refused{blockers}`. Hand-written JSON
serialise **and** a hand-rolled tolerant parser (no serde dependency, by choice). Write failures
go to stderr only — never abort the app. **Gaps**: only `before`/`after` *string summaries* are
stored (no SHAs, no recovery handle, no op args, no `destructive`/override note); logging is
opt-in per call site (not enforced); append-only (no undo-by-replay).

---

## 2. 参考プロジェクトの実装方針 (reference projects)

### 2.1 Zed `crates/git` — the closest architectural fit

- **`GitRepository: Send + Sync` trait** abstracts all git access; every method returns
  `BoxFuture<'_, Result<…>>` (async, never blocks). `RealGitRepository` is the concrete impl.
- Zed **shells out to the git CLI** (no libgit2) and runs every command on a
  `BackgroundExecutor` via `executor.spawn(async move { git.build_command(&[…]).output().await })`.
  Synchronisation is at the executor level — no `Arc<Mutex<Repository>>` smeared through callers.
- Note: *"Do not spawn this command on the background thread, it might pop open the credential
  helper which we want to block on"* — i.e. credential-sensitive ops are deliberately handled apart.
- **Lesson for Kagi**: the *trait + async-method + executor-owned-handle* shape is exactly the
  `GitBackend` we want. Kagi keeps libgit2 (for the in-memory dry-run Zed gives up), so the
  Repository handle must be owned by **one worker thread** (git2::Repository is `Send` but **not
  `Sync`**) rather than Zed's stateless CLI-per-call model.

### 2.2 GitButler — oplog / snapshot atomicity (concept only; FSL bars code reuse)

- `but-oplog`: **snapshot = a Git tree** serialising HEAD + all ref positions + meta (no WT/untracked
  content). oplog = a commit chain over snapshot trees; meta in `operations-log.toml`.
  `UnmaterializedOplogSnapshot` = **commit the snapshot only when the op succeeds → all-or-nothing**.
- This "snapshot before, confirm only on success" maps **perfectly onto Kagi's verify-then-log
  pipeline** and is the recommended hardening for `oplog.rs` (concept-adopt, per gitbutler-reuse-research).
- Backend is **gix-based** (git2 only as legacy fallback), DB-coupled, FSL-1.1-MIT (Competing Use ⇒
  **code reuse forbidden**). Adopt the snapshot-atomicity *idea*, write our own git2 code.

### 2.3 jj / Jujutsu — operation log as a content-addressed DAG (study only)

- Operations **and** their `View` snapshots are stored in **content-addressed storage like Git
  commits → safe to write without locking**. A `View` records where every bookmark/tag/git-ref
  points + the head set + the working-copy commit per workspace. `OperationMetadata` =
  description / hostname / time-range / is_snapshot.
- `jj undo` walks the op DAG one step; `jj op restore`/`op revert` jump to / revert any past op.
- Backend is **gix**; op-store is protobuf + a custom object store ⇒ too heavy for Kagi's MVP and
  non-interoperable with git2. **Adopt the metadata schema and the "undo = step back over recorded
  ops" model**; do not adopt the storage layer (Kagi keeps JSONL).
- `Merge<T>` (N-way conflict) is the only near-pure jj type; relevant to conflicts, not this domain.

Sources: [jj operation log](https://github.com/jj-vcs/jj/blob/main/docs/operation-log.md),
[jj concurrency](https://docs.jj-vcs.dev/latest/technical/concurrency/),
[Zed crates/git](https://github.com/zed-industries/zed/tree/main/crates/git).

---

## 3. 採用すべき設計 (recommended design)

Target layering (PM contract):
**domain (pure)** → **git-backend** → **app (OperationController + worker)** → **ui**.

### 3.1 domain — pure types (no git2, no I/O)

Move/keep the pure plan vocabulary into a `domain` module with **zero** git2 dependency:

```rust
pub struct OperationPlan { title, current: StateSummary, predicted: StateSummary,
    warnings: Vec<Warning>, blockers: Vec<Blocker>, recovery: String,
    preview_files: Vec<FileStatus>, preview_commits: Vec<String>, destructive: bool,
    fingerprint: RepoFingerprint }   // replaces priv head_at_plan + stash_count_at_plan
pub enum Warning { … }   // typed, localizable (today they are raw English Strings)
pub enum Blocker { … }   //   "
pub struct RepoFingerprint { head: Head, stash_count: usize, /* + index/WT hash later */ }
```

Make `Warning`/`Blocker` **typed enums** (cf. existing `BranchNameError`/`WorktreePathError`
which already do this) so i18n (ADR-0048) stops string-matching English in the UI
(`localize_plan_blockers` at `ui/mod.rs:1049` exists *because* blockers are raw strings today).

### 3.2 git-backend — a `GitBackend` trait + a unified `Operation`

The single biggest win is **killing the plan/preflight/execute copy-paste**. Replace ~30 free-fn
triads with one trait per stage driven by a unified `Operation` value:

```rust
pub enum Operation {                       // the op *request* (data, not behaviour)
    Checkout { branch: String },
    CheckoutCommit { id: CommitId },
    CherryPick { id: CommitId },
    Discard { paths: Vec<String> },
    Merge { target: String }, Revert{..}, Amend{..}, Pull, Push, …
}

pub trait GitBackend: Send {               // Send (lives on the worker), not Sync
    fn snapshot(&mut self) -> Result<RepoSnapshot>;
    fn plan(&mut self, op: &Operation) -> Result<OperationPlan>;          // analyse + in-memory dry-run
    fn preflight(&mut self, op: &Operation, plan: &OperationPlan) -> Result<()>;  // fingerprint match
    fn execute(&mut self, op: &Operation, plan: &OperationPlan) -> Result<OpOutcome>; // single mutation (+ backup if destructive)
    fn verify(&mut self, op: &Operation, plan: &OperationPlan) -> Result<VerifyReport>; // re-read & compare to predicted
}
```

- **`Git2Backend`** owns the `git2::Repository` and implements all five for local ops via the
  *existing, proven* code (the triad bodies move in nearly verbatim — low risk).
- **`CliBackend`** (or a `network: NetworkOps` collaborator) keeps `run_git` for fetch/pull/push;
  `execute` for those delegates to it then finishes in-memory, exactly as `execute_pull` does today.
- Shared boilerplate (`StateSummary` building, the dirty-parts formatter duplicated in every
  `plan_*`, the standard dirty/conflict/unborn blocker set) collapses into **helpers called by
  `plan`** — eliminating the ~30× repetition that bloats `ops.rs` to 6.5k LOC.
- **Dispatch**: a `match op { … }` inside each trait method, or per-op `impl PlanOp for Checkout`
  structs. Either kills the flat-namespace explosion; the `enum Operation` is preferable because it
  makes the op set enumerable (menu/registry, oplog `op` name, tests) and serialisable.

### 3.3 app — `OperationController` enforces the pipeline once

```rust
impl OperationController {
    pub fn request(&self, op: Operation) -> RequestId   // UI's ONLY entry point
}
```

`request` drives the canonical sequence **in exactly one place** (ADR-0004 §5):
`plan → (UI confirm; 2-stage if plan.destructive, ADR-0023) → preflight → execute → verify → log`.
- **Plan-with-blockers ⇒ never executes** (move the `if !plan.blockers.is_empty()` guard that
  `execute_discard` already has into the controller so *every* op gets it).
- **destructive ⇒ record current HEAD SHA in the oplog before execute** (ADR-0023 line 19) and
  require the two-stage confirm before the controller proceeds.
- **Logging is mandatory and centralised** — the controller writes the oplog entry for *every*
  outcome (Success/Failed/Refused), removing the opt-in-per-call-site gap. Adopt GitButler's
  *snapshot-before / commit-on-success* atomicity: capture the `before` fingerprint at plan time,
  finalise the entry only after verify (or as `Failed` with the recovery handle on error).
- Results return to the UI via GPUI's existing channel/`cx.spawn` notify mechanism (`RequestId`
  correlates the async reply), so the view never needs the repo.

### 3.4 worker-thread / repo ownership (git2::Repository is Send, not Sync)

`git2::Repository: Send + !Sync`. Therefore:
- The `Git2Backend` (and its `Repository`) lives on **one dedicated worker thread** owned by the
  `OperationController`. Requests arrive over an `mpsc`/crossbeam channel; the worker processes them
  serially (matching ADR-0004 §4 "single operation only, no chaining" and giving free
  serialisation of concurrent UI requests). This is the libgit2 analogue of Zed's `BackgroundExecutor`.
- The worker is also the natural home for the **snapshot reload** (ADR-0019 triggers: startup /
  own-op-success / `.git` watcher / manual Refresh) — moving the today-synchronous, UI-thread reload
  off the frame path and fixing the per-keystroke `Repository::open` stall.
- **Network ops** (credential-sensitive) follow Zed's caveat: run them so the credential helper can
  surface (the CLI subprocess already handles its own TTY/`GIT_TERMINAL_PROMPT`); keep them off any
  path that would deadlock a prompt.

### 3.5 Invariant #1 made structural — UI cannot touch git2

- **The `git2` dependency must not be importable from the `ui` crate/module.** Concretely: split
  the workspace so `git2` is a dependency of `git-backend`/`app` only, and `ui` depends on `app` +
  `domain` exclusively (no `git2` in `ui/Cargo.toml`). Then `git2::Repository::open` / `find_branch`
  in the UI is a **compile error**, not a convention — type-level enforcement as the PM requires.
- The UI's only verb is `controller.request(op)`; it receives `RepoSnapshot` + `OperationPlan` +
  outcomes. Per-keystroke modal validation moves to **pure domain validators** (the
  `validate_branch_rename` / `create_branch_name_errors` pattern already proves this works without a repo).

### 3.6 oplog / verify upgrades

- Extend `OpLogEntry`: add `op` **args**, before/after **SHAs** (not just string summaries),
  `recovery_handle` (e.g. discard backup blobs, pre-op HEAD SHA), `destructive`, and any override
  note (ADR-0039 §override). Adopt jj's `OperationMetadata` fields (description/host/time-range) as
  the schema target. Keep JSONL + the serde-free codec (deliberate, per ADR-context) — just widen it.
- **`verify` becomes a first-class trait step** returning a typed `VerifyReport` (predicted vs actual
  divergence) instead of being folded into individual `execute_*` bodies; controller surfaces
  divergence as a warning + recovery (ADR-0004 §5 verify).
- Optional later: GitButler-style snapshot **tree** for richer undo; jj-style **undo-by-stepping**
  over recorded ops. Not MVP.

---

## 4. 採用しない設計 (rejected)

- **gix / gitoxide backend** (jj, GitButler new-gen). Kills the in-memory dry-run path Kagi is built
  on (git2-only today), forces a backend rewrite, and conflicts with ADR-0002 (git2 primary).
- **Dropping libgit2 for CLI-only (Zed's model).** Zed can because it doesn't do in-memory conflict
  prediction. Kagi's core value *is* `cherrypick_commit`/`merge_trees` dry-run — keep git2 for local,
  CLI only for network (status quo split is correct).
- **jj content-addressed op-store (protobuf + custom object DB).** Over-engineered for Kagi's MVP and
  non-interoperable with git2; JSONL oplog stays. Adopt only its metadata/undo *concepts*.
- **GitButler virtual/parallel branches & code reuse.** FSL-1.1-MIT Competing Use bars reuse; HEAD-invasive
  workspace-commit model is the opposite of Kagi's "never mutate WT without consent" thesis.
- **`Arc<Mutex<Repository>>` shared across threads.** `!Sync` makes this fragile; single-owner worker
  thread + channel is cleaner and gives serialisation for free.
- **Per-op `Repository::open` in async tasks (current).** Replaced by the one worker-owned handle.
- **`reset --hard` / `git clean` / force push.** Forbidden remains forbidden (ADR-0004/0023);
  `execute_hard_reset`-isolated `checkout_tree(force)` only if/when ADR-0024 is approved.

## 5. リスク (risks)

1. **Behaviour-preserving migration of 6.5k LOC.** The triad bodies encode subtle, tested safety
   rules (blocker wording, in-memory dry-run, discard backup order). Move them into `GitBackend`
   *verbatim first*, refactor the shared boilerplate *second*, with the existing integration suites
   (29 suites / 306 test fns) as the regression net. Don't rewrite semantics while re-layering.
2. **De-leaking the UI is large.** 80 `Repository::open` + 81 `git2::` sites in `ui/mod.rs` (a 16.7k
   LOC god-object) must each move to `controller.request` or a pure validator. High churn, must be staged.
3. **Per-keystroke re-plan latency.** Moving plan to the worker adds async round-trips; need a
   debounce + pure-validator fast path so modals stay responsive (today they're sync-but-blocking).
4. **Worker serialisation vs UI responsiveness.** A long network op on the single worker could stall
   later requests; may need a separate network lane or cancellation (Zed's credential caveat applies).
5. **Optimistic-concurrency fingerprint.** `preflight` today only compares HEAD (+ stash count). A
   richer `RepoFingerprint` (index/WT hash) is safer but costs a status read each preflight.
6. **Localization regression.** Typing `Warning`/`Blocker` must preserve the exact English strings the
   tests pin (`Display` impls do this for `BranchNameError` today — follow that pattern).

## 6. 未解決事項 (open questions)

1. **Dispatch shape**: single `enum Operation` + `match` in the backend, vs per-op `impl PlanOp`
   trait objects? (enum favoured for enumerability/oplog/tests; confirm with PM.)
2. **Crate split granularity**: separate cargo crates (`kagi-domain`, `kagi-git-backend`, `kagi-app`,
   `kagi-ui`) to make git2 un-importable from ui, vs module-only with a lint? Crates give real
   type-level enforcement (PM Invariant #1) but more build plumbing.
3. **How much oplog widening is MVP** — args + SHAs + recovery handle is clearly in; jj-style
   undo-by-stepping and GitButler snapshot-trees are clearly later. Where's the v1.0 line?
4. **Snapshot reload ownership**: does the worker own the `.git` watcher (ADR-0019) too, or a
   separate observer that nudges the controller? (Reload-on-worker fixes the UI-thread stall.)
5. **Cancellation / timeout** for long worker ops (esp. CLI network) — needed for the single-lane worker?
6. **`RepoFingerprint` contents** — HEAD-only (cheap, current) vs +index/WT hash (safe, costs a status).
7. **Confirm-flow callback shape** — how does the controller pause mid-pipeline for the UI's
   (possibly two-stage) confirm without blocking the worker? (Likely: plan returns to UI; a second
   `confirm(RequestId)` resumes into preflight→execute.)
