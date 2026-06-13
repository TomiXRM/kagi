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

- [ ] **S1 — Workspace skeleton.** Create `crates/{kagi-domain,kagi-git,kagi-app,kagi-ui,kagi}` + `kagi-test-fixtures`. Move the existing bin into `crates/kagi`. Keep code shape; use `pub use` bridges. Compiles + tests green.
- [ ] **S2 — Extract `kagi-domain`.** Move already-pure modules (graph, diff/diffstat models, resolution, templates, checklist, message-gen rules, status/refs models, plan types, settings/i18n). Re-point tests. Unlocks the test pyramid base.
- [ ] **S3 — Extract `kagi-git`.** `GitBackend` trait + `Operation` enum; split `ops.rs` per-op; stand up the per-session worker thread; git2 adapter behavior-identical (move verbatim, refactor second). CLI adapter for network.
- [ ] **S4 — De-leak the UI.** Route every `Repository::open`/`git2::` site in the view through `OperationController`/`GitBackend`. Add CI grep gate. *(Candidate for Codex GPT-5.5 high/xhigh, batched per feature area.)*
- [ ] **S5 — Introduce `kagi-app`.** `AppState`/`RepoSession`/`OperationController`; collapse active-vs-cache; `Selection` enum; `RepoMode`.
- [ ] **S6 — Split the view.** Carve `ui/mod.rs` into per-feature components + view-models; collapse modals into `ActiveModal`.
- [ ] **S7 — Retire `KAGI_*`, add `ci.yml`, update README for v1.0.**

## Deviations / decisions log
- (none yet)

## Tooling note
Heavy mechanical-but-intricate extraction (S4, parts of S3/S6) may be delegated to
Codex (GPT-5.5 high/xhigh) via the codex CLI / plugin, in well-scoped per-area
batches, mindful of its Plus-plan rate limits. Claude (Max) owns orchestration,
integration, and keeping the workspace green.
