# Kagi architecture recovery result

## 1. Delivery and revision identity

- Worktree: `/Users/tomixrm/Dev/sandbox/kagi-astra-recovery`; branch: `dev`.
- Start: `5f7a4c80618d270010a1360a4c4a062c75147be5` on the actual default branch `main` (`master` does not exist).
- The requested delay completed in1800.01s. The original checkout, untracked request.md and concurrent Claude work were preserved.
- 114 refs were reviewed by ancestry, patch equivalence and squash/history evidence. The user explicitly deferred `origin/claude/kagi-brew-cargo-mise-nohgv1`; it introduces distribution decisions/conflicts and was not blindly merged.
- Three independent behavior batches were selected, plus a separately verified mechanical prerequisite and this docs-only closeout: five local commits. P1 needs the recorder extraction; P2/P3 do not depend on its behavior.
- The final delivery commit is the commit **adding this file**, not a fictional self-embedded SHA. Its exact immutable identity is recoverable with `git log --diff-filter=A -1 --format=%H -- docs/agent-loop/2026-09-astra-recovery/FINAL.md`; the terminal handoff records that full ending SHA. STATE.md records the final source parent.

## 2. Completed changes

1. **Backend recorder extraction:** move existing result mapping/append code unchanged into `crates/kagi-git/src/backend/recording.rs`. This separates mechanical relocation from changed behavior; no crate, schema or compatibility path is introduced.
2. **Durable operation completion:** stash-drop, history undo/redo and cleanup record at the mutation boundary, before guarded UI delivery. Remote stash-drop records at the existing transport boundary. Full recovery OIDs survive tab switches; cleanup reuses `finish_op_on_main` and releases the confirmed modal before dispatch. Typed preflight errors preserve the existing localized phase label. Opening failure is recorded in the background caller because no Backend exists yet.
3. **Bounded split projection:** replace the collision-prone title/length moved cache and per-frame pairing with one weak-Arc-keyed projection owner in `src/ui/diff_split/cache.rs`. Migrate the existing PR-conflict producer to retain its loaded preview and refresh locale/theme through copy-on-write, preserving outstanding snapshots.
4. **Narrow history Table:** allow its existing truncated author/stat columns to shrink with `min_w(0)` in `crates/kagi-ui-core/src/commit_row.rs`. Card production is unchanged. Keep704-cell native bounds regressions and actual Editor History coverage; correctly drain macOS native window teardown without disabling GPUI leak detection.
5. **Evidence:** full baseline/current audit,27-cache inventory, measured before/after results and narrow corrections to architecture/migration guidance. No new product feature or target-crate scaffold.

## 3. Commits and independent review

| Commit | Purpose |
|---|---|
| `47678c04f0821412fd0bc706a63c22cbacdc22d0` | Behavior-identical backend recorder extraction |
| `91097e8632249af5d9612fa88cb6b94e8db0804b` | Mutation-owned durable completion |
| `d2def467968a106b2332ec83e2cbface2de7d30d` | Correct bounded split projection and PR preview ownership |
| `1291cc903cb8e29eb26ed9f37a47db8e081ed2d5` | Narrow Table metadata and native layout regressions |
| This file's addition commit (see §1) | Final evidence and architecture guidance |

The docs-only ending commit is `docs: record architecture recovery evidence and remaining risks`. The full five-commit list and exact ending SHA are also in the terminal handoff. Every commit carries named agent Co-Authored-By trailers.

Independent read-only reviews: RecordingCutReviewer, DurableFinalReview (with Fable ownership review), ConflictCacheFinalReview, HistoryLayoutFinalReview and NativeLifecycleReview. Main reproduced decisive reports, rejected speculative Card changes, fixed localized preflight/open-failure findings, and exercised the assembled GUI. RecoveryEvidenceReview caught two measurement-label mistakes: qualified-prefix expressions are not distinct Git calls, and cold trials reuse one cache-cleared input. Final EvidenceDeliveryReview accepted numerical/native claims. ArchitectureCloseoutReview found one inventory error: RepoSession is retained in the active-local-tab slot, not for every open tab; Main verified tabs.rs:149,177 and corrected AUDIT.md.

## 4. Structural measurements

| Metric | Before | After |
|---|---:|---:|
| Workspace packages |11|11|
| KagiApp fields |113|113|
| Inherent KagiApp impls / files |64 /51|64 /51|
| cx.notify static sites |443|442|
| Qualified-Git-prefix expressions / imports in root UI |176 /59|177 /60|
| uniform_list / variable list / ListState constructors |10 /2 /5|10 /2 /5|
| Rust files over800 LOC / source-only subset |50 /41|50 /41|
| Workspace test passes / failures / existing ignores after baseline artifact repair |1848 /0 /30|1864 /0 /30|
| Warm split pairings per101 unchanged renders |101|0|
| Incorrect moved marks in equal-title/size reproduction |4|0|

The full large-file list, impl distribution, notify owners, dependency sites and UI conventions are in AUDIT.md. Counts include source-local cfg(test), exclude integration-test directories for AST metrics, and are not dynamic repaint rates. All11 manifests, Cargo.lock and ci/loc-baseline.txt are byte-identical to the start; no new dependency/crate or raised ceiling. Root ownership is intentionally not claimed solved.

## 5. Validation and limitations

Exact commands, elapsed times and stage results are preserved in METRICS.md. Baseline fmt/build/clippy/invariants passed. The initial workspace run failed two xtask tests because copied artifacts embedded a deleted build path; targeted xtask artifact rebuild passed5/5 and the unchanged baseline rerun passed. This was not classified as a production regression or hidden by removing tests.

The extraction passed1848 tests; P1 passed1855; P2 passed1864, with0 failures and30 pre-existing ignores. Focused split13/13 and PR6/6 passed. Primary assembled native execution passed all existing/P1/layout scenarios; temporary timing and state instruments were then removed. The704 geometry cells are custom native scenarios, not704 extra Rust unit tests.

No existing test was deleted, ignored or weakened. Platform teardown initially failed leak detection despite passing geometry assertions; draining MacPlatform's close queue and autorelease pool while the GPUI context remains alive fixed process exit. LEAK_BACKTRACE is a separate expensive diagnostic toggle, not leak detection; disabling its backtrace capture does not suppress the leak assertion.

Final source verification after removing all temporary hooks:

| Command | Exit | Seconds |
|---|---:|---:|
| `cargo fmt --all -- --check` (after `cargo fmt --all`) |0|0.753|
| `cargo build --workspace` |0|7.859|
| `cargo test --workspace` |0|643.929|
| `uv run --project ci check-all` |0|0.376|
| `cargo clippy --workspace --all-targets` |0|57.989|

Tests ran with `KAGI_GUI_E2E=1 KAGI_NO_RESTORE=1
KAGI_NO_SINGLE_INSTANCE=1 LEAK_BACKTRACE=""`:1864 passed/0 failed/30 ignored,
95 Rust result summaries, plus every native scenario and successful runner exit.
Clippy has the same12 diagnostic/future-warning classes and13 located
diagnostics; no new/increased message/file pair. These elapsed times include
build/native teardown, not application latency.

## 6. Current workspace dependency diagram

Arrows list workspace dependencies only (external dependencies omitted):

```text
gpui-terminal -> none
kagi -> gpui-terminal, kagi-domain, kagi-git, kagi-ui-core,
        kagi-ui-editor, kagi-ui-file-history, kagi-ui-ecosystem
kagi-domain -> none (manifest dependency-free)
kagi-git -> kagi-domain
kagi-mcp -> kagi-domain, kagi-git
kagi-ui-core -> kagi-domain
kagi-ui-editor -> kagi-domain, kagi-ui-core
kagi-ui-file-history -> kagi-domain, kagi-ui-core
kagi-ui-ecosystem -> kagi-domain, kagi-ui-core
kagi-web -> kagi-domain
xtask -> none
```

`kagi-app`/`kagi-ui` remain targets, not implemented crates. Existing RepoSession and feature entities are acknowledged; the current session is retained in one active-local-tab slot, not a per-open-tab session cache. Creating more crates is not the next automatic migration step.

## 7. Duplication decisions

Removed: cleanup's custom async completion in favor of the existing panic/stale guard; UI-side durable-result responsibility for the selected operations; repeated split pairing and separate moved-mark derivation ownership; PR's per-render reconstruction of loaded conflict rows.

Retained deliberately: tiny three-line ListState constructors across feature boundaries; read-only diff-derived data versus editable buffer state; root orchestration while measured ownership remains unresolved. A new helper/dependency for three lines or a common mutable/read-only cache would blur policy, not improve ownership. The existing shared modal/theme grammar was reused, not replaced with another design system.

## 8. Cache and runtime results

Split medians (nanoseconds):31 cold trials using one allocation and clearing between trials;101 warm trials. Initial after and final integrated repeat are both disclosed.

| Rows | Cold before | Cold initial after | Cold final repeat | Warm before | Warm initial after | Warm final repeat |
|---:|---:|---:|---:|---:|---:|---:|
|256|428417|391292|285167|10417|83|83|
|4096|4479959|4332209|4558041|111375|83|83|
|32768|35500292|34795833|36737667|885042|42|83|

Cold always performs31 pairings and31 move derivations. Warm changes101 pairings/0 move derivations to0/0. The robust benefit is removal of O(rows) pairing/allocation on unchanged renders, **not** a speedup ratio derived from nanosecond cache hits. Cold speedup is not established. Retention is bounded by8 FIFO entries and8MiB conservative derived-storage accounting; it excludes active renderers, holds no strong source rows and bypasses oversized entries.

Actual app medians, milliseconds,7 trials on a three-commit/4096-line Rust fixture:

| Path | Before | Contemporary baseline control | Matched recovery |
|---|---:|---:|---:|
| Startup snapshot + first window |208.079|198.540|195.128|
| Uncached repo open |1609.350|1591.883|1679.582|
| First-return uncached tab presentation |1191.600|1169.617|1268.336|
| Commit selection / changed files |10.565|10.446|10.213|
| Cold diff open |243.273|245.372|244.474|
| Warm diff open |224.116|226.605|225.104|
| External reload excluding watcher debounce |67.589|68.940|69.585|

No overall app speedup or statistical equivalence is claimed: repo/tab samples remain slower. Startup excludes process/font initialization; diff completion excludes later syntax work; OS caches are uncontrolled. The raw tab key misleadingly says cached, but logs report cached=no. Initial after data inherited LEAK_BACKTRACE=1 and is excluded from the comparable table; a clean matched rerun used an empty value (even `0` enables capture). Other caches have a source-based inventory, not invented hit/miss/RSS measurements.

## 9. GUI coverage and objective evidence

- 704 Card/Table cells:320×240,640×480,1024×768,1440×900 ×80/100/125/150% zoom ×EN/JA ×11 inputs ×2 row styles. Fresh mounts and selected/unselected transitions.
- Inputs: empty, one character,512 continuous ASCII,512 unspaced Japanese, long branch/path/email, multiline commit message, emoji+combining marks, very long URL, plus a spaced-author case. Raw model strings stay unchanged.
- Actual GPUI natural-height measurement versus fixed allocation; finite/nonnegative immediate-child bounds; row N+1 top ≥ row N bottom. Baseline Table overflow at320px width/125%/EN was x320..400 for the final column; corrected matrix passes.
- Actual KagiApp/Editor History with all11 inputs, first-entry reorder and unchanged scroll extent at fresh1440/1024/1440 windows; fixture fingerprints unchanged.
- 160 throwaway real-KagiApp paints cover empty/loading/error/selected/disabled across all sizes/zooms/locales, asserting immediate Root bounds. No deep descendant paint proof is inferred.
- Existing oplog interaction still proves expansion22→69px, following row738→785px and copied content; existing theme, graph-copy, snapshot and WIP scenarios remain active.
- No successful screenshot, native in-place resize, deeply nested Card metadata-width or gap-free adjacency claim. CoreGraphics found no on-screen window for the exact owned app PID; pinned GPUI reports render_to_image unavailable. Fresh exact-size native mounts and bounds are the objective fallback.

## 10. Safety evidence

The `plan → confirm → preflight → execute → verify → oplog` path is retained; recording moves to the selected mutation owner, not an unguarded UI callback. Seven permanent backend/offline-transport regressions exercise stash-list drift, changed HEAD, trust refusal, index/worktree preservation, full-OID stash/ref recovery and real local-shell transport success/failure. Native active/stale-tab/open-failure and EN/JA cases verify one persistent entry for each executed/failed attempt and no wrong-tab UI update. Full OIDs are used for fixture recovery, not merely asserted nonempty.

All write verification used disposable repositories/settings/logs. No authenticated SSH/GitHub path is represented as tested; offline transport exercises the command boundary, not real authentication. Existing klog contracts, domain purity and UI git2 prohibition remain intact; CI invariants pass. Existing append failure policy is unchanged: this is not crash/disk-full exactly-once delivery. No real user repository was used for write testing.

## 11. Top five intentionally deferred findings

1. **Cleanup partial-result loss:** successful remote-half deletion can be hidden if later local deletion returns an error (`ops/branch_cleanup.rs:481–486` at baseline). Existing primitive/API limitation, not claimed repaired by recording relocation.
2. **Other non-run mutation owners:** conflict/terminal/PR-merge families need separate recording/partial-outcome review. The selected three families do not prove universal backend dispatch.
3. **Application-state ownership:**113 root fields and64 impls remain; active-view/tab-cache copying, async freshness/cancellation and worker setting parity need one measured responsibility boundary, not a wholesale crate move.
4. **Remaining cache lifetime/key debt:** row-index DiffCaches still clear on renumber; ecosystem data survives tab close, other memory/disk caches are unbounded, and some values are duplicated. This batch fixes only the measured split projection/PR producer.
5. **Remaining UI-thread and layout evidence debt:** binary-conflict reads/highlighting and large-file cold rebuilds remain; deeper Card descendants, real resize/focus and a real tab-cache-hit timing series need their own scenario.

Separately, the packaging branch is deferred by explicit user decision; standalone stash-drop Enter remains absent from the root router (the existing confirmation handler is exercised instead).

## 12. Next highest-priority batch

Characterize **cleanup remote-success/local-failure** with an offline real-shell fixture, then return and persist the partial outcome plus both recovery identities even when the second phase fails. Preserve the existing confirm/trust/preflight gates and localized presentation. Do not start by moving KagiApp or wiring every operation through the worker.

## 13. Human checks still needed

On a real visible macOS window: resize continuously between the four sizes, inspect Table/Card/Editor History at125/150% in EN/JA, deep metadata truncation, focus rings and accessible copy values; inspect dark/light PR conflict previews after theme/language changes. Objective offscreen bounds are not a claim that those visual checks happened.

## 14. Delivery safety

No project push, release, tag creation, force push, destructive worktree cleanup or main commit was performed. The original checkout and explicitly deferred branch remain intact. The final terminal handoff records clean `dev` status and its exact ending SHA; only verified production/test paths and the requested documentation are committed. Temporary cache counters, runtime hooks and state probes are not shipping source. Tests may create refs or push between their own disposable fixtures; these are not delivery operations on the user's repository.
