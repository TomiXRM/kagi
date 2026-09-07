# Kagi v1.0 Migration Notes

Strangler migration from the v0.2.0 single-crate app to the v1.0 workspace
(`docs/rearch/architecture.md` §7). Each step keeps `cargo test --workspace`
green. This file is the running log: check off steps, record deviations, and
note why any destructive change was made.

## Current interpretation (2026-09-07)

The numbered steps below retain dated history, including their historical
LOC/test counts. ADR-0121's entity/feature-pane route is implemented; it does
not imply `kagi-app`, `kagi-ui`, a universal worker dispatch path or zero-copy
active-tab swapping exists. Do not resume S5 by mechanically creating crates.
Settings, i18n and klog now live in `kagi-ui-core` with root re-exports.
Current workspace/DAG/counts and explicit deferred ownership work are recorded
in `docs/agent-loop/2026-09-astra-recovery/{AUDIT,METRICS}.md`.
The recovery extends ADR-0149's mutation-owned logging for stash drop,
history undo/redo and cleanup; guarded UI completion is presentation-only.
The measured app-layer slices below now live in `src/app` and existing `Sessions`;
they do not establish the original `kagi-app` crate, universal worker dispatch, or
zero-copy tab ownership target.

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
- [x] **S3 — Extract `kagi-git`.** The backend now lives in `crates/kagi-git/`
  (completed in **ADR-0115**, 2026-06-22; the old `src/git/` tree no longer exists).
  Sub-steps below record how it got there; paths written `src/git/…` in the dated
  sub-step entries describe the pre-extraction layout.
  - [x] S3a (Codex) — moved pure plan/outcome/`Head` types (`StateSummary`, the validation enums, `AmendMode`, `MergeKind`, all `*Outcome`, `DiscardBackup`, `Head`) to `kagi-domain` (`plan.rs`/`head.rs`); `OperationPlan` kept in `ops.rs` (its `pub(crate)` plan-time fields move with the OperationController in S5). All green.
  - [~] S3b — `ops.rs` is split per-op (now `crates/kagi-git/src/ops/<feature>.rs`); the `Backend` façade owns the git2 path; the `Operation` enum (`kagi-domain::operation`) and a per-session worker thread (`crates/kagi-git/src/worker.rs`, ADR-0073) exist. A formal `GitBackend` trait + wiring the worker as the UI's only call path remain gated with S5 (the OperationController).
- [x] **S4 — De-leak the UI** (ADR-0078). Done in 2 batches:
  - S4a (Codex): added `src/git/backend.rs` — a `Backend` handle owning git2::Repository with 98 delegating methods (git2-clean public API). Additive, green.
  - S4b (Codex): rewrote all ~82 `Repository::open` sites + every `plan_/execute_/git::` call across `src/ui/{mod,avatar_fetch,tabs,conflict_view,commit_panel,commands}.rs` onto `Backend`. **`grep -rE 'git2::|Repository::open' src/ui` = 0.** 635 tests green. CI grep gate added in `ci.yml`. (Crate-level enforcement — moving src/ui into a git2-free `kagi-ui` crate — lands with S6.)
- [~] **S5 — Measured application-layer migration.** `src/app` now owns selected
  plan/approval/lease/completion flows and session identity, while the original
  `kagi-app` crate, `OperationController`, zero-copy active/cache ownership,
  `Selection`, and `RepoMode` remain unimplemented. ADR-0121 still rejects a
  wholesale crate move without measured ownership and worker-setting parity.
  - [x] **#484 — remove boundary and writer admission (#526/#530).** Remove owns receipts/Unknown handling;
    competing GUI writers use the same Sessions leases ([ADR-0175](../../adr/0175-app-remove-boundary.md)).
  - [x] **Stash family complete — #541/#550/#557/#572.** Local push/apply/pop/drop and remote drop use Sessions plans/leases;
    conflict follow-up waits for reload and stays bound to its owner ([ADR-0176](../../adr/0176-app-stash-local-boundary.md)).
  - [x] **#482 stage 1 — #574.** `SessionId`/`WorktreeId`, attach/detach lifetime,
    owner-bound plans, and sibling invalidation are implemented ([ADR-0182](../../adr/0182-session-identity-and-lifetime.md)).
  - [x] **Plan failure state — #570.** Modal plan/replan failure replaces the old
    plan and prevents confirmation until a new Ready plan exists ([ADR-0180](../../adr/0180-modal-plan-failure-state.md)).
  - [x] **Transport recording — #558.** PR merge and SSH remote pull record at their
    execution boundary; ambiguous transport outcomes remain `Unknown` ([ADR-0177](../../adr/0177-transport-recording-boundary.md)).
  - [x] **Backend policy — #563/#575.** Central actor/auto-snapshot policy enforces mandatory trust/preflight/verify;
    raw-executor callers migrated, retaining documented family exceptions ([ADR-0178](../../adr/0178-backend-execution-policy.md)).
  - [x] **Recovery roots — #568.** Discard/remove file backups survive GC through ref-backed blob roots;
    default retention matches the oplog entry lifetime ([ADR-0179](../../adr/0179-ref-backed-discard-remove-backups.md)).
  - [x] **CLI/MCP contract — #571.** Both frontends use the shared agent contract and
    report the receipt from their own run, never the global oplog tail ([ADR-0181](../../adr/0181-agent-contract-cli-mcp.md)).
  - [x] **Test oplog isolation — #578.** Git fixtures use per-test children with temporary
    `KAGI_LOG_DIR`; test runs refuse the HOME fallback with `tests must set KAGI_LOG_DIR` ([ADR-0149](../../adr/0149-oplog-in-backend-run-and-schema.md)).
  - [x] **Conflict design r1 — #577.** [FAMILY-conflict](../app-layer/FAMILY-conflict.md) defines revision-bound jobs and settlement;
    the PR's PM ruling requires implementation in C0 → C1 → C2 → C3 order below.
  - [ ] **#482 stage 2 — #489.** Make snapshot/read ownership single-owner and
    replace the read generation guard with `RequestSlot`.
  - [ ] **#482 stage 3.** Move selection, scroll, pane, and menu state into `TabView`,
    then remove `reset_per_repo_ui`.
  - [x] **Conflict C0 — #582 / part of #569.** Preserve `TerminationUnknown` as `Unknown`,
    reserve the owner lease before Continue/Skip/Abort, and retain it when stop is unconfirmed.
  - [x] **Conflict C1.** Move Save and directory/file resolution through the app
    boundary while retaining the legacy busy bridge for the other conflict actions;
    exact conflict/buffer revisions and the Backend-owned receipt accompany the
    finite completion evidence.
  - [ ] **Conflict C2.** Migrate typed Abort flows with progress-aware outcomes,
    recovery, and Backend-owned recording.
  - [ ] **Conflict C3.** Migrate Continue/Skip and reconcile, preserve typed process
    evidence, and remove the remaining direct conflict writers/UI recording.
  - [ ] **#314.** Establish a separate worker proof before any universal dispatch
    migration; the current worker remains outside the production GUI path.
- [~] **S6 — Split the view.** Carve `ui/mod.rs` into per-feature modules; later collapse modals into `ActiveModal` + add view-models. Progress (`ui/mod.rs` 16,775 → 14,331 LOC):
  - [x] S6a (Codex): modal subsystem → `src/ui/modals.rs` (3,504 LOC; ~22 modal structs + ~24 render_*_modal fns).
  - [x] S6c (Claude): diff view-models + tree-sitter highlighter → `src/ui/diff_view.rs` (287 LOC).
  - [x] Later slices: commit-panel rendering and KagiApp render methods moved into focused siblings; `ActiveModal` is structural (ADR-0093); UI-core and editor/file-history/ecosystem panes became crates (ADR-0121).
  - [ ] Full `src/ui` → git2-free `kagi-ui` remains a target, not completed or the automatic next step. Root UI's direct-git prohibition remains enforced by the uv invariant gates.
- [~] **S7 — Retire `KAGI_*`, add `ci.yml`, update README for v1.0.**
  - [x] `ci.yml` added: blocking `cargo test --workspace` (macOS) + UI-git2-free grep gate; advisory fmt/clippy + Linux test leg (pre-existing v0.2.0 lint debt).
  - [ ] retire `KAGI_*` headless harness (after S5/S6 make view-models testable); README v1.0 update.

## Deviations / decisions log
- **2026-06-14:** S1 scoped down — instead of creating all 5 crates up front (a big
  churn with no immediate payoff), only `kagi-domain` was carved now. The remaining
  crate boundaries (kagi-git/app/ui/bin) are introduced when the code actually
  separates at S3b/S4/S5, to avoid a long red window. The git2-confinement invariant
  is still the end goal; it becomes enforceable once the UI is de-leaked (S4).
- **2026-06-14:** S2c/S2d/S3a/S4a/S4b/S3b/S6a delegated to Codex (gpt-5.5, high
  reasoning) per the Codex-for-complex-implementation directive. Each batch reviewed +
  re-tested by Claude before commit. Pattern: precise per-area brief, hard
  green/no-API-change constraints, Codex iterates cargo itself, Claude verifies
  invariants (grep gate, exhaustiveness, domain-purity) + commits.
- **2026-06-14:** Codex Plus quota exhausted mid-S6b (resets ~07:07). S6c (diff_view)
  was then done by Claude directly as a self-contained low-risk slice. Larger coupled
  moves (commit_panel, KagiApp methods, S5 state model) wait for Codex to resume.

## Completion-criteria status (goal)

| Criterion | State |
|-----------|-------|
| `cargo test --workspace` passes | ✅ green at every commit (635 tests) |
| Main Git ops tested on fixtures/tempdirs | ✅ existing 306 suites preserved; domain unit-tested |
| UI ↔ Git-logic responsibilities separated | ✅ `src/ui` is git2-free (CI grep gate); git logic in `kagi-domain` + the `kagi-git` Backend (`crates/kagi-git/`, ADR-0115) |
| Dangerous ops via plan→confirm→preflight→execute→verify | ✅ preserved (Backend delegates to intact triads; no destructive cmds added) |
| v0.2.0 features migrated to v1.0 structure | 🟡 git logic fully migrated to domain/backend; views organised into modules + modals/diff_view extracted; remaining god-file decomposition (S5/S6) in progress |
| docs/rearch/research + architecture.md + ADR + migration notes | ✅ inventory, 10 research docs, architecture.md, ADRs 0072–0078, this log |
| Destructive-change rationale explainable | ✅ ADRs + per-commit messages |
| README v1.0 description updatable | ✅ v1.0 section added to README |

**Foundation = delivered** (layering, git2-confinement invariant, safety pipeline,
testability, extensibility). Remaining = incremental growth on the foundation (S5
app-state model, finish S6 view decomposition, S3b worker thread, retire `KAGI_*`).

## Issue #13 structural refactor (2026-06-17) — see ADR-0091
GLM5.2 Max codebase-structure review (issue #13). Low-risk, behavior-preserving
prefix of its roadmap done as verbatim physical splits, each `cargo test --workspace`
green (739 tests):
- **High-1 (P10):** `AGENTS.md` added at repo root (invariants + layering + conventions).
- **High-2 (P1):** `src/ui/mod.rs` 18,510 → 12,368 LOC. Extracted `src/ui/types.rs`
  (199) and `src/ui/render.rs` (5,983) as child modules — no visibility widening.
- **High-3 (P4):** `src/ui/settings.rs` (174) carved out of `theme.rs`; callers in
  i18n/tabs/smart_commit/mod switched to `settings::`.
- **Medium-1 (P3):** `src/git/ops.rs` 7,071 LOC → `src/git/ops/` (9 per-op modules +
  shared `mod.rs`); `src/git/mod.rs` re-exports unchanged.

- **Phase 4 (P1 cont.):** `src/ui/mod.rs` 12,370 → 6,380 LOC. Operation-orchestration
  methods moved verbatim into `src/ui/operations/` (10 per-family submodules:
  conflict/branch/commit/history/stash/checkout/pull_push/cherry_revert/worktree/
  discard), each an `impl KagiApp` block using `use crate::ui::*;`. Only `pub(crate)`
  widening for 10 cross-module helpers.

- **serde `Settings` (P4 second half) — see ADR-0092:** `settings.json` now parses via
  `serde_json` into a typed `Settings` struct. On-disk flat string format unchanged;
  `write_setting` preserves unknown keys (the `SETTINGS_KEYS`-drop foot-gun and the
  hand-written `parse_string_value` are gone). `theme.rs` reads via typed accessors.

- **P7 / `ActiveModal` enum — see ADR-0093:** the ~22 `Option<XModal>` fields on
  `KagiApp` collapsed into one `active_modal: Option<ActiveModal>` with per-modal
  accessors in `src/ui/operations/modal_state.rs`. Implements the ActiveModal half of
  ADR-0076.

- **P5 / ViewModel layer foundation — see ADR-0094:** new `src/ui/view_models/`
  module with `StatusBarVM` (pure, window-free unit tests); `render_status_bar`
  consumes it. First slice of ADR-0076's VM layer; further VMs + the log-protocol
  split (Low-1) continue from here.

- **P2 / per-tab dual-state collapse — see ADR-0095:** the ~16 duplicated top-level
  `KagiApp` view fields replaced by a single `active_view: TabViewState`;
  `apply_tab_view` is now a one-line move (no field-by-field sync foot-gun). The
  behaviour-preserving slice of ADR-0075's RepoSession model.

- **P5 / log-protocol seam (Low-1) — see ADR-0096:** all ~347 `[kagi]` contract
  lines now route through the `klog!` macro (single greppable emission point),
  separable from ad-hoc human logging. Byte-identical output; behaviour unchanged.

Deferred (bigger / behavioral): remaining VMs + moving headless assertions onto VM
unit tests (P5); the full `RepoSession`/`OperationController` zero-copy swap
(ADR-0075, beyond the P2 slice).

## Tooling note
Heavy mechanical-but-intricate extraction (S4, parts of S3/S6) may be delegated to
Codex (GPT-5.5 high/xhigh) via the codex CLI / plugin, in well-scoped per-area
batches, mindful of its Plus-plan rate limits. Claude (Max) owns orchestration,
integration, and keeping the workspace green.
