# ADR-0117: FileHistory → `Entity<FileHistoryView>` (Phase 5.1, first Backend-driven panel)

- Status: Accepted
- Date: 2026-06-22
- Follows: ADR-0110 (Toast/OpLog entity decomposition), `docs/refactor-plan.md` Step 5.1
- Tickets: `T-FHENTITY-001`

## Context

`KagiApp` (`src/ui/mod.rs`, ~5977 LOC) is a god-object. `refactor-plan.md` Step 5.1 decomposes
its UI panels into child `Entity<T>`, each with its own `cx.notify()` scope, to kill the whole-app
repaint on every small interaction. ADR-0110 did this for the two **pure-UI** panels
(`ToastStack`, `OpLogPanel`) — they touch no `Backend`, so the parent simply drives them via
`entity.update(cx, …)` and there is no child→parent path.

The four remaining panels (`CommitPanel`, `FileHistoryView`, `Sidebar`, `ConflictEditor`) all
**drive the `Backend`** (diff load, stage, checkout, resolve). Their Entity move therefore needs a
**child→parent callback path**, not just a field hoist (refactor-plan.md:434-437). No such path
exists yet. This ADR establishes that precedent on the **lowest-risk** of the four —
**FileHistory** (~1,014 LOC, 3 owning fields, read-only `Backend` work except navigation) — so the
pattern is proven before it is applied to the Git-mutating panels.

A three-axis analysis plus an independent cross-model plan review (Codex) both selected FileHistory
as the correct first pick and confirmed the mechanism/shape below.

## Decision

Promote `KagiApp.file_history` from `Option<file_history::FileHistoryState>` to
`Option<Entity<file_history::FileHistoryView>>`, where `FileHistoryView` owns the state data plus
its own `impl Render`.

1. **Child→parent mechanism: `WeakEntity<KagiApp>`** held by the entity. This matches existing
   prior art (`src/ui/watcher.rs` upgrades a `WeakEntity<KagiApp>` to call `reload()`); the repo has
   no `EventEmitter` idiom (`cx.subscribe` appears once, `cx.emit` never). **Guardrail:** the
   back-ref is used ONLY inside event/listener/timer closures, **never** in a `Render` read path —
   honouring the invariant that render must not re-read the checked-out `KagiApp` via `cx`
   (`render_body.rs:402`, `file_history_render.rs:44`).

2. **Entity shape: "fat" (D2) — revised during implementation.** The cross-plan recommended a
   "thin" D1 (logic stays on `KagiApp`, mutates the entity via `entity.update`). Implementation
   surfaced a hard GPUI constraint: a child→parent callback fired from the FileHistoryView's *own*
   listener leases that entity; if the parent method then calls `self.file_history.read/update`,
   it re-leases the same entity and **panics ("already borrowed")**. This kills D1 for every
   *entity-initiated* Backend action (row-click select, refresh, retry, follow-toggle). So the
   load/select/diff logic moves **into** `FileHistoryView` (it holds `repo_path`): the entity
   updates *itself* (direct field writes for sync work, its own `cx.spawn` for async), and the only
   parent callbacks are `close` (`self.file_history = None`) and `jump_to_commit` (graph nav) —
   neither of which touches the entity's lease. `KagiApp` keeps thin entry points
   (`open_file_history`, `step_file_history_selection`, `close_file_history`).

3. **Async stale-result guard stays atomic with the mutation.** Each load runs on the entity's own
   context and marshals back via the entity's own weak handle; a per-entity `generation` counter is
   bumped on every (re)load and checked *inside* the same self-update that writes history/error —
   so rapid refresh/follow on the same entity discards the older result, and a load whose entity was
   dropped (close/reopen) simply no-ops.

4. **Notify-scope discipline.** Row selection, context-menu toggle, and split-drag notify the
   **FileHistory child only**. Close-overlay and jump-to-commit notify **`KagiApp`** (they change
   parent-owned state — the active body view / graph selection).

5. **`file_history_menu` moves into the entity** (pure FH overlay state). `file_history_geom`
   (`Rc<Cell>`, shared with layout) stays on `KagiApp`; the divider-drag handler mutates
   `fh.data.split` via `fh.update` + child-notify.

6. **klog fix (in-scope).** The contract line at `mod.rs:3376` is emitted via a raw multiline
   `eprintln!("[kagi] file-history: loaded …")` — a latent ADR-0096 violation that slips past the
   single-line CI gate. Since this exact block is rewritten, it becomes
   `klog!("file-history: loaded {} entries", …)` with **byte-identical** emitted text.

This is **behaviour-preserving**: same element tree, styles, handlers, async semantics, `[kagi]`
contract lines (order + text), and i18n. Validated by `cargo test --workspace` + the headless
`KAGI_*` harness, but — because subagents cannot exercise the GUI (CLAUDE.md) — it carries a
**human UI-verification-pending** flag until eyeballed.

## Consequences

- A reusable child→parent precedent (`WeakEntity<KagiApp>` back-ref + thin entity + atomic
  generation guard) now exists for the three Git-mutating panels (Step 5.1 remainder).
- FileHistory interactions that are FH-internal (row highlight, menu, split) re-render only the FH
  subtree instead of the whole app.
- One latent klog single-channel violation is closed.
- Out of scope (deferred): the other three panel extractions; moving FH load logic into the entity
  (a possible Step 5.2+ follow-up once `RepoSession` worker lands); `file_history_geom` relocation.

## Rollout

Single PR on `refactor/fh-entity`, green on `cargo build` + `cargo test --workspace` +
`cargo fmt --check` + no new clippy warnings, cross-reviewed by `codex exec review --base main`.
Flagged for human in-app verification (incl. close/refresh/follow race paths) before release.
