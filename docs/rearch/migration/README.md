# Kagi v1.0 Migration Notes

Strangler migration from the v0.2.0 single-crate app to the v1.0 workspace
(`docs/rearch/architecture.md` §7). Each step keeps `cargo test --workspace`
green. This file is the running log: check off steps, record deviations, and
note why any destructive change was made.

## Invariants held at every step
- `cargo test --workspace` green.
- Every v0.2.0 feature (`inventory.md` §2) and safety guarantee (`§3`) preserved.
- `plan → confirm → preflight → execute → verify` pipeline intact.
- No new `git2::` usage introduced into the (eventual) `kagi-ui` crate.

## Steps

- [x] **S1 — Workspace skeleton.** Added `crates/kagi-domain` as a workspace member + dep of root `kagi`. (Full crate fan-out — kagi-git/app/ui/bin split — deferred until S3b/S4/S5 when the layers actually separate; root `kagi` crate stays the bin/ui host for now via bridges.)
- [x] **S2 — Extract `kagi-domain`.** Done in 4 batches (S2a–S2d), all green:
  - S2a: `commit`, `graph`. S2b: `refs`, `message_template`, `trailers`.
  - S2c (Codex): split `status`, `diffstat`, `diff`, `checklist` (model→domain, git2→backend).
  - S2d (Codex): split `resolution` (conflict model), `message_gen` (rule-based + parsers).
  - kagi-domain = 3,418 LOC, 11 modules, zero git2/gpui. Old `kagi::git::*` paths preserved via per-module re-export shims in `src/git/`.
- [~] **S3 — Extract `kagi-git`.** Sub-steps:
  - [x] S3a (Codex) — moved pure plan/outcome/`Head` types (`StateSummary`, the validation enums, `AmendMode`, `MergeKind`, all `*Outcome`, `DiscardBackup`, `Head`) to `kagi-domain` (`plan.rs`/`head.rs`); `OperationPlan` kept in `ops.rs` (its `pub(crate)` plan-time fields move with the OperationController in S5). All green.
  - [ ] S3b — `GitBackend` trait + `Operation` enum; split `ops.rs` per-op; per-session worker thread; git2 adapter behavior-identical; CLI adapter for network. *(Gated with S5: needs the OperationController as the UI's call path. The S4a `Backend` façade is the synchronous precursor of this trait.)*
- [x] **S4 — De-leak the UI** (ADR-0078). Done in 2 batches:
  - S4a (Codex): added `src/git/backend.rs` — a `Backend` handle owning git2::Repository with 98 delegating methods (git2-clean public API). Additive, green.
  - S4b (Codex): rewrote all ~82 `Repository::open` sites + every `plan_/execute_/git::` call across `src/ui/{mod,avatar_fetch,tabs,conflict_view,commit_panel,commands}.rs` onto `Backend`. **`grep -rE 'git2::|Repository::open' src/ui` = 0.** 635 tests green. CI grep gate added in `ci.yml`. (Crate-level enforcement — moving src/ui into a git2-free `kagi-ui` crate — lands with S6.)
- [ ] **S5 — Introduce `kagi-app`.** `AppState`/`RepoSession`/`OperationController`; collapse active-vs-cache; `Selection` enum; `RepoMode`.
- [ ] **S6 — Split the view.** Carve `ui/mod.rs` into per-feature components + view-models; collapse modals into `ActiveModal`. (Then move src/ui → `crates/kagi-ui` with no git2 dep — makes the invariant a compile error.)
- [~] **S7 — Retire `KAGI_*`, add `ci.yml`, update README for v1.0.**
  - [x] `ci.yml` added: blocking `cargo test --workspace` (macOS) + UI-git2-free grep gate; advisory fmt/clippy + Linux test leg (pre-existing v0.2.0 lint debt).
  - [ ] retire `KAGI_*` headless harness (after S5/S6 make view-models testable); README v1.0 update.

## Deviations / decisions log
- **2026-06-14:** S1 scoped down — instead of creating all 5 crates up front (a big
  churn with no immediate payoff), only `kagi-domain` was carved now. The remaining
  crate boundaries (kagi-git/app/ui/bin) are introduced when the code actually
  separates at S3b/S4/S5, to avoid a long red window. The git2-confinement invariant
  is still the end goal; it becomes enforceable once the UI is de-leaked (S4).
- **2026-06-14:** S2c/S2d delegated to Codex (gpt-5.5, high reasoning) per the
  Codex-for-complex-implementation directive. Each batch reviewed + re-tested by
  Claude before commit. Pattern: precise per-module brief, hard green/no-API-change
  constraints, Codex iterates cargo itself, Claude verifies invariants + commits.

## Tooling note
Heavy mechanical-but-intricate extraction (S4, parts of S3/S6) may be delegated to
Codex (GPT-5.5 high/xhigh) via the codex CLI / plugin, in well-scoped per-area
batches, mindful of its Plus-plan rate limits. Claude (Max) owns orchestration,
integration, and keeping the workspace green.
