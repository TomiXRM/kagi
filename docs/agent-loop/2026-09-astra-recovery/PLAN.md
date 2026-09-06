# Recovery plan

## Scope and sequencing

1. Preserve current work; resolve actual outstanding integrations before creating `dev` from the default branch (`main`; no `master` exists).
2. Record toolchain, OS, dependencies, fmt/build/test/clippy/CI/GUI baselines and current-code metrics before production edits.
3. Audit architecture/ownership, performance/cache, GPUI/layout, safety/tests independently, read-only. Main rechecks important claims.
4. Select 2–5 independently reviewable batches, priority P0 safety/state corruption, P1 ownership, P2 measured performance, P3 reproduced layout faults. No new product features.
5. For each selected batch record symptom, root cause, touched/non-touched files, before metrics, acceptance, regression scenario, rollback and dependencies; then characterize, implement minimally, verify, independently review and locally commit with agent co-author attribution.
6. Re-measure, run final validation and objective layout coverage, update stale architectural guidance only where evidence requires, remove temporary implementation aids, and deliver a clean worktree without push/release/tags.

## Global rollback conditions

Stop and split any batch exceeding 15 production files or roughly 1,500 changed lines (pure moves excepted), mixing structural and visual changes, or lacking a single-sentence reason. After three failures of one approach, preserve evidence and back out only this task's changes safely. Never weaken tests, ratchets or safety checks to obtain green.

## Selected batches

### Prerequisite — Extract existing backend recorder unchanged

- Purpose: isolate the already-existing durable result mapping and append owner before adding the missing callers; no dependency, schema, API-path or behavior change.
- Files: `crates/kagi-git/src/backend.rs`, new focused `backend/recording.rs`. Existing function bodies move byte-for-byte; the public mapping remains exported from `backend`.
- Acceptance/rollback: every existing call resolves and produces identical entries; otherwise revert the mechanical extraction before any behavior change.
- Verified: fmt, workspace build, 1848 passing tests / 0 failures / 30 existing ignores, all invariants; independent reviewer found no changes to runtime semantics. Local commit `47678c0`.
- Four implementation commits plus a separate final evidence/document-correction commit make five total, within the requested 2–5. Keeping the full audit and measurement inventories separate avoids inflating the focused P1 commit beyond the approximate 1500-line boundary. No LOC baseline increase is accepted.

### Batch 1 — Durable non-run operation completion (P1)

- Symptom: stash-drop and history undo/redo omit persistent operation records; cleanup has a custom completion task that can retain the busy latch after panic or apply results to another tab.
- Root cause: non-`Backend::run` mutations leave durable recording and async completion ownership in inconsistent UI paths. Persisting only inside a guarded callback is insufficient: a tab switch discards that callback.
- Scope: `src/ui/operations/{stash,history,mod}.rs`, `src/ui/{blocking_ops,branch_cleanup}.rs`, and existing backend/oplog boundaries only if required to preserve mutation-owned recording. Regression scenarios in the main-thread GUI lane; no #476 commit/worktree feature edits.
- Baseline: 2 non-run families omit successful/failed persistent entries; cleanup has 1 custom plain `task.await` completion instead of the shared panic/stale guard.
- Acceptance: each attempted non-run write produces exactly one persistent record with correct repository and recoverable OIDs, including stale UI completion; cleanup always releases busy state and never replaces another tab's modal. Existing log wording/order and Git preflight/trust behavior remain unchanged.
- Regression: real fixture stash-drop and history undo/redo; stale completion into another fixture tab; controlled panic/refusal completion, checking durable JSONL, repository fingerprints and active UI state.
- Rollback: revert this batch if recording duplicates, stale UI updates, recovery OID loss or pipeline behavior changes. No schema migration; old oplog remains readable.
- Dependencies: the mechanical recorder-extraction prerequisite, because this commit extends its new child module. P2 and P3 are independent behavior batches. Fable reviewed ownership before implementation; Main validated.
- Complete: `91097e86`;1855 passing workspace tests, native success/refusal/stale-tab/open-failure/EN/JA coverage and independent review. Existing assertions and log contracts retained.

### Batch 2 — Correct, bounded split-diff derived cache (P2)

- Symptom: diffs with equal title/row count can reuse another diff's moved-row marks; split pairing is rebuilt and allocated every rendered frame.
- Root cause: the existing process cache uses selection-surface identity instead of immutable content ownership and caches only half of the derived split layout.
- Scope: `src/ui/diff_split.rs`, `src/ui/render_helpers.rs`, focused cache child module, and the existing PR-conflict producer in `pr_mode.rs`/`pr_conflicts.rs`. Independent review proved that caller rebuilt its source Arc every render, so it must retain its loaded rows in the same cutover. Do not change graph-row DiffCaches, selection semantics, Git or PR navigation behavior.
- Baseline: MOVED_CACHE has 8 entries but no byte budget; pairing allocates O(rows) on every split frame. Record cold/warm medians before replacing the implementation.
- Decision: extend the existing derived-layout owner, key by the existing immutable `Arc<Vec<DiffRow>>` allocation with weak ownership (no retained heavy source rows and no address reuse), cache paired rows and moved marks together, retain the 8-entry cap and add a documented derived-payload budget. No new dependency or global singleton.
- Acceptance: distinct same-size diffs never share wrong marks; repeated rendering of unchanged rows reuses pairing/detection; source replacement and dropped source invalidate correctly; cache respects both bounds, including oversized entries.
- Regression: same-title/same-length different contents; changed Arc payload; entry and weight eviction; repeated warm retrieval versus cold construction measured across multiple sizes.
- Rollback: revert if weak identity is not reliable under current row mutation, if warm paths allocate proportional to rows, or if retained derived storage is unbounded.
- Dependencies: none. Isolated writer worktree `recovery/split-cache`; Main owns all validation and commits.
- Complete: `d2def46`;1864 passing workspace tests,13 focused split and6 PR tests, primary native runner and independent review. Final integrated repeat eliminates101 warm pairings; same-title/size wrong marks4→0. No end-to-end speedup claim.

### Batch 3 — Fixed-height history metadata contract (P3)

- Symptom: at 320×240 / 125% / EN, the Table's fixed author/stat columns force the final column from x320 to x400 outside its 320px parent. Card metadata remains a guard target, not a claimed reproduced defect.
- Root cause: Table author/stat columns opt out of shrinking; the subject alone cannot absorb a narrow viewport's deficit. Let the existing truncated columns shrink with `min_w(0)`; no new renderer or clip-based workaround.
- Scope: `crates/kagi-ui-core/src/commit_row.rs`; objective native GUI scenarios in `tests/recovery/` with Main integrating the runner route. No new theme system, product strings or full-window redesign.
- Baseline: reproduced Table overflow with actual GPUI text measurement; two existing consumer shapes use the shared renderer. Speculative Card changes were explicitly removed.
- Acceptance: empty/one-character/512-ASCII/512-Japanese/long branch/path/email/multiline/emoji+combining/URL values do not overlap subsequent rows or illegally exceed containers. The raw model/copy values stay unmodified.
- Verification matrix: 320×240, 640×480, 1024×768, 1440×900; zoom 80/100/125/150%; EN/JA; empty/loading/error/selected/disabled where those states exist. Check finite nonnegative bounds, row ordering, resize behavior and fixture non-mutation; screenshots supplement, never replace assertions.
- Rollback: revert if normal short metadata becomes inaccessible, fixed list heights vary by content, or the layout fix merely clips an incorrect height.
- Dependencies: none. Isolated writer worktree `recovery/history-layout`; Main owns GUI execution and final integration.
- Complete: `1291cc903cb8e29eb26ed9f37a47db8e081ed2d5`; final GUI-enabled workspace1864 passed/0 failed/30 ignored, fmt/build/clippy/CI passed, with no new warnings.704 Card/Table cells and actual loaded Editor History pass;160 real-App state paints passed separately. Fresh exact-size bounds are the documented fallback; native in-place resize/screenshots and deep Card width remain human checks, not claimed acceptance evidence.

### Closeout — Evidence and current architecture correction

- Purpose: preserve the full current-code audit, measured before/after data,
  final commit/verification record and honest remaining runtime limitations.
- Scope: the five explicitly requested recovery documents and narrow corrections
  to architecture/migration guidance. No production code or new test abstraction.
- Acceptance: numbers match executed evidence; old target topology is clearly
  distinguished from implemented entity/feature-crate boundaries; no incomplete
  result, screenshot, native resize, process-startup or remote-auth claim.
- Dependency: all four implementation commits verified. Rollback any unsupported
  statement rather than manufacture evidence. This is the fifth local commit.
- Complete evidence: AUDIT.md, METRICS.md and FINAL.md preserve the full inventories, matched medians, final source checks, deferred findings and runtime limitations. Closeout changes only those requested documents/STATE/PLAN and architecture/migration guidance.

## Explicit integration exception

User chose to defer `origin/claude/kagi-brew-cargo-mise-nohgv1` after review showed real new distribution functionality, 3 merge conflicts, an ADR collision, CI incompatibility and a formula-versus-cask product choice. All original refs remain intact. `dev` was created from `main` at `5f7a4c8`; the original worktree and concurrent #476 work are untouched.
