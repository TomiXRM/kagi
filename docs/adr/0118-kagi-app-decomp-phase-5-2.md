# ADR-0118: KagiApp decomposition, Phase 5.2 (data-cluster consolidation + Git-panel entity promotion)

- Status: Accepted
- Date: 2026-06-23
- Follows: ADR-0110 (Toast/OpLog entity decomposition), ADR-0117 (FileHistory → `Entity<FileHistoryView>`), `docs/rearch/migration/README.md` S6
- Tickets: `T-DECOMP-001` (avatar), `T-DECOMP-002` (diff-cache), `T-ENTITY-CONFLICT-001`, `T-ENTITY-COMMITPANEL-001`

## Context

`KagiApp` (`src/ui/mod.rs`, ~5.7k LOC, 110+ fields) is the project's known god-object (CLAUDE.md).
ADR-0110 and ADR-0117 began Phase 5.1: pure-UI panels (`ToastStack`, `OpLogPanel`) and the first
Backend-driven panel (`FileHistoryView`) became child `Entity<T>` with their own `cx.notify()`
scope; `sidebar` and `conflict` were consolidated from ~6 and ~13 flat fields into named
sub-structs (`SidebarState`, `ConflictState`). ADR-0117's consequences explicitly leave a
**"Step 5.1 remainder": the three Git-mutating panels** (`CommitPanel`, `ConflictEditor`,
`Sidebar`) still to promote, now that the `WeakEntity<KagiApp>` child→parent precedent exists.

Two distinct kinds of state still sit flat on `KagiApp`:

1. **Pure-data caches read *during the parent's* render** — the changed-files/diff caches
   (`diff_cache`, `file_diff_cache`, `remote_diff_inflight`, `local_diff_inflight`,
   `diffstat_cache`, `src/ui/mod.rs:850-867`) and the avatar cache (`avatar_images`,
   `avatar_fetch_for`, `:1138-1142`). These are not self-rendering views; the main app render reads
   them to decorate commit rows / the inspector.
2. **Self-rendering Git-mutating panels** — `ConflictState` + the conflict editor/dashboard, and
   `CommitPanelState` + its inputs — which are the ADR-0117 "remainder".

## Decision

Phase 5.2 continues the decomposition with **two mechanisms, matched to the state's kind**, and a
**risk-ordered ticket sequence** (safest first, one PR per ticket, each cross-model reviewed and
flagged human-UI-verify-pending per ADR-0117).

### Mechanism A — sub-struct consolidation (for pure-data caches)

Group a cohesive flat-field cluster into a named `struct` field on `KagiApp` (the ADR-0110 5.1
mechanism), with the shared invalidation centralised as a method. **These clusters deliberately do
NOT become `Entity<T>`:** they are read inside the *parent's* `Render`, so an entity would buy no
notify-scope isolation and would instead add a re-entrancy surface (reading a child entity during
the parent render is exactly the borrow hazard ADR-0117 warns against). Consolidation is a pure,
test-verifiable refactor that shrinks the god-object and makes the eventual `RepoSession`/`AppState`
move (S5) tractable.

- **T-DECOMP-001** — `avatar_images` + `avatar_fetch_for` → `struct AvatarStore` (smallest cluster,
  ~7 owning sites). **Proving run** for the SubAgent→PM pipeline.
- **T-DECOMP-002** — the five changed-files/diff cache fields → `struct DiffCaches` with a
  `clear()` that replaces the 4 hand-maintained invalidation sites (`reload`, `reload_external`,
  `tabs::reset_per_repo_ui`, `tabs::show_welcome`), removing the "forgot one cache" bug class.

### Mechanism B — `Entity<T>` promotion (for the Git-mutating panels, the real "子Entity抽出")

Promote a consolidated panel to a self-rendering `Entity<T>` following the **ADR-0117 template
verbatim**:

1. Child→parent via a **`WeakEntity<KagiApp>` back-ref**, used ONLY in event/listener/timer
   closures — **never** in a `Render` read path.
2. **"Fat" entity**: the panel owns its Backend-driving logic (it holds `repo_path`) and updates
   *itself*; the parent keeps only thin entry points (`open_*`/`close_*`) plus callbacks that touch
   *parent*-owned state (so they don't re-lease the child mid-listener and panic "already
   borrowed").
3. **Atomic stale-result guard**: a per-entity `generation` bumped on each (re)load and checked
   inside the same self-update that writes the result.
4. **Notify-scope discipline**: panel-internal interactions notify the child only; actions changing
   parent-owned state notify `KagiApp`.
5. **Behaviour-preserving**: identical element tree/styles/handlers, async semantics, `[kagi]`
   contract lines (text + order), and i18n.

- **T-ENTITY-CONFLICT-001** — `ConflictState` → `Entity<ConflictView>` (lowest-risk of the three
  remainders: state already consolidated, a self-contained full-screen mode). Safety-critical
  (conflict resolution atomicity, ADR-0106) → own PR, extra adversarial cross-review.
- **T-ENTITY-COMMITPANEL-001** — `CommitPanelState` (+ `commit_input`, template inputs,
  `smart_commit`, draft autosave) → `Entity<CommitPanelView>`. Highest coupling → last.
- `Sidebar` Entity promotion is **deferred** past Phase 5.2: its render consumes large amounts of
  `active_view` ref data, so it needs a data-push channel (no `WeakEntity` reads in render),
  which is better sequenced with the S5 `AppState` work.

### Sequencing / dispatch (PM)

Tickets land **sequentially**, not in parallel: every ticket edits the same `KagiApp` struct +
init + invalidation regions, so concurrent worktrees would collide there. Each ticket is one
SubAgent (`general-purpose`) implementing to the ticket spec on a per-ticket branch; the PM
(Conductor) verifies `cargo build` + `cargo test --workspace` + `cargo fmt --check` + no new
clippy, then a **cross-family (codex) review** before integrating. The safe consolidations
(001/002) precede the entity promotions to prove the pipeline and de-risk the heavy moves.

## Consequences

- `KagiApp` loses ~12 flat fields across 001/002 and two large panels migrate to isolated
  notify-scopes (B), continuing toward the S5/S6 target architecture.
- The `DiffCaches::clear()` method removes the recurring "forgot to clear one of the N cache
  fields" bug class (e.g. the `local_diff_inflight` reset gap caught during ①-1b review).
- Each entity promotion carries an unavoidable **human-UI-verify-pending** flag (subagents cannot
  exercise the GUI, CLAUDE.md); the test suite + headless harness gate correctness but not pixels.
- Out of scope: the Sidebar entity, the S5 `AppState`/`RepoSession`/`OperationController` collapse,
  and the `main_diff`/`pending_diff_highlight`/`compare_view` view cluster (a possible later
  `DiffCaches` extension or its own ticket).

## Rollout

One PR per ticket on `refactor/kagi-app-decomp-5-2` (or per-ticket branches), green on
`cargo build` + `cargo test --workspace` + `cargo fmt --check` + no new clippy, cross-reviewed by
`codex` (different model family). Entity-promotion PRs flagged for human in-app verification
(incl. the conflict resolve/continue/abort race paths) before release.
