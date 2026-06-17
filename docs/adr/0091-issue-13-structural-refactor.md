# ADR-0091 — Issue #13 structural refactor: AGENTS.md + physical splits

- **Status:** Accepted
- **Date:** 2026-06-17
- **Context:** GitHub issue #13 (GLM5.2 Max codebase-structure review). The review
  identified 10 structural problems (P1–P10) and a phased, behavior-preserving
  roadmap aligned with the existing `docs/rearch/` migration plan.

## Decision

Execute the **low-risk, high-value** prefix of the review's roadmap — the items it
rated High-1/2/3 and Medium-1 — as a set of purely mechanical, behavior-preserving
changes, each verified by `cargo build` + `cargo test --workspace` (739 tests green).
No `Operation` enum, no `ActiveModal` enum, no ViewModel layer yet — those are larger
behavioral changes deferred to follow-ups.

### What was done

1. **P10 / High-1 — `AGENTS.md` (new, repo root).** Single entry point for agents:
   the four hard invariants (no `git2::` in `src/ui`, `kagi-domain` purity, no
   destructive commands, the plan→preflight→execute→verify→oplog pipeline), the
   layering table, file-size targets, and the settings/logging/state-update/modal
   conventions. Consolidates rules that were spread across 90+ ADRs.

2. **P1 / High-2 — `src/ui/mod.rs` physical split (18,510 → 12,368 LOC).**
   - `src/ui/types.rs` (199 LOC): auxiliary presentational types — `BottomTab`,
     `DividerKind`/`DividerDrag`/`DividerGhost`, `BranchDrag`/`BranchDragGhost`,
     `FooterStatus`, `Toast`/`ToastKind`.
   - `src/ui/render.rs` (5,983 LOC): `impl Render for KagiApp` plus the `render_*`
     presentation methods and free render functions.
   - Both are **child modules** of `crate::ui`, so they keep access to `KagiApp`'s
     private fields with no visibility widening (Rust: descendants see ancestor
     privates). `impl KagiApp` is legally spread across files. The
     `KagiApp` struct and operation-orchestration methods stay in `mod.rs` (a later
     phase). The GPUI re-entrancy rule (no `cx.entity().read(cx)` inside render) was
     preserved — moves were verbatim.

3. **P4 / High-3 — `src/ui/settings.rs` (new, 174 LOC).** Extracted the
   settings-persistence service (`read_setting`/`write_setting`/`settings_path`/
   `parse_string_value`/`SETTINGS_KEYS`) out of `theme.rs`. `theme.rs` now holds only
   theme tokens/logic and calls `settings::` for its own persistence. Callers in
   `i18n.rs`/`tabs.rs`/`smart_commit.rs`/`mod.rs` updated. JSON format, key set, and
   parse behavior unchanged.

4. **P3 / Medium-1 — `src/git/ops.rs` → `src/git/ops/` (7,071 LOC → 9 modules +
   `mod.rs`).** Per-operation physical split: `checkout`, `branch`, `worktree`,
   `stash`, `cherry_revert`, `merge`, `pull_push`, `history`, `discard`. Shared types
   and helpers (`OperationPlan`, preflight helpers, signature/oid/tree utilities) live
   in `ops/mod.rs`, which re-exports every submodule so the public surface — and the
   `pub use ops::{…}` list in `src/git/mod.rs` — is unchanged. Functions moved
   verbatim; the `plan_`/`execute_` of each operation are now co-located.

## Consequences

- A "change checkout behavior" task now reads a ~430-LOC `ops/checkout.rs` instead of
  scanning a 7k-LOC file; UI rendering is separated from operation control.
- Visibility was widened only to `pub(crate)` for a handful of shared `ops/` helpers
  (never narrowed); `src/git/mod.rs` is byte-for-byte unchanged.
- The git2-confinement CI grep gate still passes (`src/ui` has zero `git2::`).

## Phase 4 / P1 cont. — `src/ui/operations/` split (done)

The operation-orchestration methods were moved verbatim out of `mod.rs`
(**12,370 → 6,380 LOC**) into ten per-family submodules under
`src/ui/operations/`, each an additional `impl KagiApp` block:

- `conflict` (866), `branch` (1,279), `commit` (951), `history` (736),
  `stash` (730), `checkout` (524), `pull_push` (416), `cherry_revert` (293),
  `worktree` (235), `discard` (215).

`src/ui/operations/mod.rs` only declares the submodules. As grandchildren of
`crate::ui` the modules use `use crate::ui::*;` and keep access to `KagiApp`'s
private fields/methods. Visibility was widened only to `pub(crate)` for ten
helper methods called across module boundaries (`commit_title_for`,
`replan_*`, `default_worktree_path`, `seed_history_from_reflog`,
`set_template_inputs`, `effective_commit_message`); none narrowed. Behaviour,
signatures, and the GPUI re-entrancy rule are unchanged. `cargo test
--workspace` = 739 passed / 0 failed; git2 grep gate clean; fmt clean.

## Not done (deferred follow-ups, per the review)

- **Medium-2 / P7 / ADR-0076** — `ActiveModal` enum replacing ~25 `Option<XModal>`.
- **Medium-3 / P5** — ViewModel layer so UI is unit-testable without the `KAGI_*`
  headless harness; then the log-protocol split (Low-1).
- **P2 / ADR-0075** — collapse active-vs-`tab_cache` dual state into `RepoSession`.
- **serde-backed `Settings`** (P4 second half).
