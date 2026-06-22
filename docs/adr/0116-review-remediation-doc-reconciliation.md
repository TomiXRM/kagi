# ADR-0116: Review remediation — doc reconciliation, render anti-patterns, god-file splits

- Status: Accepted
- Date: 2026-06-22
- Follows: ADR-0115 (`kagi-git` crate extraction), ADR-0110 (entity decomposition),
  `docs/refactor-plan.md` Phase 3 (perf) & Phase 5 (structural moves)
- Tickets: `T-DOC-001`, `T-KLOG-001`, `T-PERF-RENDER-001`, `T-PERF-RENDER-002`,
  `T-SPLIT-RENDER-001`, `T-SPLIT-HELPERS-001`, `T-SPLIT-PULLPUSH-001`

## Context

A three-axis code review (UI/GPUI layer, domain/git layer, cross-cutting
abstraction) surfaced findings that cluster into three themes. The git layer and
the domain purity invariant came out **healthy** (`kagi-domain` is dependency-free;
the same-named files across `kagi-domain`/`kagi-git` are intentional shims/splits,
not duplication). The debt is concentrated elsewhere:

1. **Documentation drift (highest-priority, lowest-risk).** ADR-0115 moved the git
   backend from `src/git/` to `crates/kagi-git/`, but the *live* instruction docs
   were never reconciled. `CLAUDE.md` (the self-described "single entry point for
   AI agents") points the new-feature workflow at `src/git/ops.rs` — a path that
   no longer exists. `docs/refactor-plan.md` describes the `src/git → crates/kagi-git`
   move as a *future* Step 5.5 and references `src/git/snapshot.rs` in Step 3.6,
   though both already happened. An agent following these docs writes to nowhere.
   55 stale `src/git/` references exist across 11 files.

2. **GPUI render anti-patterns.** `render()` is not pure: it runs synchronous
   git/file I/O on the UI thread (`render.rs:341/355` → `mod.rs:2021/2033/2061`:
   `Backend::open`, conflict-session detection, `ResolutionBuffer::load`), mutates
   `self` mid-render (`render.rs:307/321/356`), and rebuilds `sidebar.rows` from
   every ref each frame (`render.rs:496-511`). These are exactly the targets of
   `refactor-plan.md` Steps 3.1 / 3.4 / 3.6, not yet executed.

3. **Oversized god-files.** Four UI files and one op file blow past the repo's
   own ≤800 LOC/file, ≤80 LOC/fn targets: `mod.rs` (5775), `render.rs` (3521,
   with an 882-line `render`), `modal_renderers.rs` (3395), `render_helpers.rs`
   (3153, with an 832-line `render_commit_panel`), `ops/pull_push.rs` (2077).

The KagiApp god-object (≈88 `pub` fields) and full `Entity<T>` decomposition are
real but are **already owned by Phase 5.1** (ADR-0110, in progress) and are
explicitly higher-risk (they need child→parent callback paths). This ADR does
**not** re-scope them; it executes the lower-risk, higher-certainty items first.

## Decision

Execute the remediation in three waves, each landing green (`cargo build` +
`cargo test --workspace` + `cargo fmt --check`) before the next:

### Wave 1 — Truth first (parallel, disjoint files)

- **T-DOC-001:** Correct stale `src/git/` → `crates/kagi-git/` in the *live*
  instruction docs (`CLAUDE.md`, `AGENTS.md`, `docs/refactor-plan.md`,
  `docs/rearch/migration/README.md`, `docs/rearch/architecture.md`). Reconcile
  "future" framing of already-done moves. Point-in-time *research* snapshots
  (`docs/rearch/research/*`, `docs/rearch/inventory.md`) are historical analysis —
  they are **not** rewritten; where they could mislead, a dated "superseded by
  ADR-0115" banner is added rather than editing the body. Also drop the
  "god-file mid-split" claims for files that finished splitting, and the dead
  `take_X` accessor references (the accessors were removed).

- **T-KLOG-001:** Replace the three raw `eprintln!("[kagi] …")` contract-line
  writes (`headless.rs:194`, `ops/stash.rs:405`, `ops/pull_push.rs:51`) with
  `klog!`, byte-for-byte identical output, and add a CI grep gate forbidding
  `eprintln!("[kagi]` outside `klog.rs` — closing the ADR-0096 single-channel
  contract the way the git2 gate closes ADR-0078.

### Wave 2 — Render purity (sequential; central files `render.rs`/`mod.rs`)

- **T-PERF-RENDER-001 (Steps 3.1/3.6):** Move the conflict-detect + reflog-seed
  I/O and `Backend::open` out of the `render()` path into the reload/tab-switch
  commit point via `cx.background_spawn` + marshal-back. Remove the
  `ensure_auto_fetch_ticker` call from `render()`; arm it on app init.
- **T-PERF-RENDER-002 (Step 3.4):** Stop rebuilding `sidebar.rows` every frame —
  recompute only when its inputs (branches/collapsed/filter) change; hoist
  `theme()`/`zoom()` to one local per render and gate `set_rem_size` behind a
  change check; drop the per-row `row.clone()` in `render_rows`.

These are **behaviour-preserving**; they are validated by `cargo test` + the
headless `KAGI_*` harness, but because the GUI cannot be exercised by subagents
(CLAUDE.md), each carries a "human UI verification pending" flag until eyeballed.

### Wave 3 — Split the god-files (mechanical, behaviour-preserving moves)

- **T-SPLIT-RENDER-001:** Split `render.rs` along feature seams (header / body /
  sidebar / bottom-panel / overlay) into sibling `render_*.rs` modules; the
  882-line `render` becomes a thin composition of extracted methods.
- **T-SPLIT-HELPERS-001:** Move the commit-panel and file-history render code out
  of `render_helpers.rs` to sit next to their state (`commit_panel.rs` /
  `file_history.rs`); extract the shared modal "card" chrome in
  `modal_renderers.rs` into a `RenderOnce` component so each modal renderer
  shrinks toward the 80-LOC target.
- **T-SPLIT-PULLPUSH-001:** Split `ops/pull_push.rs` into `ops/pull.rs` /
  `ops/push.rs` / `ops/fetch.rs` (+ shared helpers), keeping each
  `plan_/preflight_/execute_` triple together and the `#[cfg(test)]` tests with
  their op. Rename in-module test fns off the `plan_*` prefix so the triple
  inventory greps clean.

Splits are pure `mod`/visibility moves with re-exports preserving public paths;
no `[kagi]` contract line, no behaviour, changes.

## Consequences

- The docs stop lying: an agent reading `CLAUDE.md` is sent to the file that
  actually exists. This is the cheapest, highest-leverage fix and ships first.
- `render()` stops blocking the UI thread on I/O and stops doing O(all-refs)
  allocation per frame — the Phase 3 perf win, scoped to the two safest steps.
- Each oversized file drops under (or toward) the repo's own size targets,
  shrinking the merge-conflict and review surface that motivated the request.
- Out of scope here (deferred to their existing owners): the `KagiApp` →
  `Entity<T>` panel decomposition (Phase 5.1), `verify_X`/oplog wiring in
  `Backend::run` (Phase 5 controller), and `ops/history.rs` unit-test backfill
  (tracked as a follow-up). This ADR records that these were *considered* and
  intentionally sequenced after the low-risk waves.

## Rollout

Waves land as separate commits/PRs on `claude/cool-shannon-uk8c5s`, each green on
build + `cargo test --workspace` + `cargo fmt --check`. Wave 2 & 3 items that
touch UI render are flagged for human in-app verification before release.
