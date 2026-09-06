# Recovery measurements

Start HEAD: `5f7a4c80618d270010a1360a4c4a062c75147be5`. Environment: Apple M5 Pro arm64, Darwin 25.4.0; rustc/cargo 1.96.0, LLVM 22.1.2. Debug builds unless stated. Isolated temporary fixture repositories/settings; no writes to real user repositories. OS filesystem cache is uncontrolled: 'cold' means fresh application/cache input, not an OS-cache flush.

## Reproduction protocol

- Baseline: `cargo metadata --format-version 1`, `cargo tree --workspace`, `cargo tree --workspace -d`, `cargo fmt --all -- --check`, `cargo build --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `uv run --project ci check-all`.
- Native: `KAGI_GUI_E2E=1 KAGI_NO_RESTORE=1 KAGI_NO_SINGLE_INSTANCE=1 cargo test -p kagi --test gui_e2e_runner -- --nocapture`. Runner redirects KAGI_LOG_DIR to a temporary directory.
- Structural AST: `uv run --python 3.12 --with tree-sitter==0.24.0 --with tree-sitter-rust==0.23.2 /tmp/kagi-astra-recovery-evidence/structure_metrics.py <worktree> <before|after> [revision]`. Source/metrics scripts are temporary measurement instruments, not production dependencies.
- Split timing: temporary `recovery_split_cache_measurements` module invoking real `split_rows`/moved detection before, real `split_projection` after; `cargo test -q -p kagi --lib recovery_split_cache_measurements -- --nocapture --test-threads=1`. 31 cold trials reuse the same input allocation, clearing the cache before each retrieval; 101 same-input warm trials per size. No timing assertions or distinct-input variability claim.
- App timing: temporary `runtime_bench.rs` invoked by the real native runner, 7 trials each. Three-commit fixture with a 4096-line added Rust file and a second two-commit repository. Measures fresh app/data caches using Instant and explicit completion predicates. Startup is snapshot plus first offscreen window, **not process/font initialization**. Diff-open ends at visible rows, not later async syntax completion. External reload calls the actual external reload path but excludes watcher debounce. Tab switch measures first-return **uncached** presentation, not a cache hit or background revalidation; the historical raw JSON key `tab_switch_cached_presentation` is misleading and is corrected in the table below.
- Raw evidence: `/tmp/kagi-astra-recovery-evidence/{baseline-*,recording-extraction-*,durable-completion-*,structure-before.json,runtime-before.json,split-cache-before.txt,split-cache-after.txt}`. Temporary instrumentation is removed before commits; tables here preserve results without requiring those files.

## Initial validation

| Check | Exit | Elapsed seconds |
|---|---:|---:|
| fmt | 0 | 0.775 |
| build | 0 | 114.347 |
| test | 101 | 404.117 |
| clippy | 0 | 116.489 |
| invariants | 0 | 0.592 |

The initial workspace run had 1814 passed / 2 failed / 2 ignored before stopping in xtask. Those two failures came from copied build artifacts embedding a deleted `CARGO_MANIFEST_DIR`, not production source. `cargo clean -p xtask && cargo test -p xtask` passed all 5; the full unchanged-baseline rerun passed in 79.78s (artifact://47). The independently verified behavior-identical recorder extraction then passed **1848 / 0 / 30 ignored**, 93 result summaries, 350.114s. The different wall time is build/cache/background-load noise, not an application speed claim.

Baseline native runner: all 8 existing scenarios passed in 15.74s. Isolated real app also launched, opened first commit/file and installed its watcher; CoreGraphics saw no visible window for its owned PID. Pinned GPUI render_to_image and offscreen native resize are unavailable; bounds assertions and fresh exact-size windows are used, not fictitious screenshots/resize success.

## Structural before/after

| Metric | Baseline | Recovery |
|---|---:|---:|
| Workspace packages | 11 | 11 |
| KagiApp fields | 113 | 113 |
| Inherent KagiApp impl blocks / files | 64 / 51 | 64 / 51 |
| cx.notify static sites | 443 | 442 |
| All methods named notify (one unrelated git callback) | 444 | 443 |
| uniform_list constructions | 10 | 10 |
| gpui::list constructions / ListState constructors | 2 / 5 | 2 / 5 |
| UI call expressions with qualified Git-prefix callee text / imports | 176 / 59 | 177 / 60 |
| Tracked Rust files >800 LOC / source-only subset | 50 / 41 | 50 / 41 |

AUDIT.md contains complete per-file inventories and the dependency DAG. Counts are source AST nodes including source-local cfg(test), not dynamic notification/repaint rates or an alias-complete call graph. LOC baseline must remain byte-identical; no numerical increase is accepted.

The 176 expressions include chained `ok`/`map_err`/`and_then` wrappers as well as their inner Git calls. This is a bounded syntactic inventory, not 176 distinct Git operations; the exact collector and label are preserved for before/after comparability.

The inherent impl distribution is unchanged. Lexical KagiApp notify sites234→233;
free-function cx.notify sites remain152. Other entity counts in AUDIT.md remain
unchanged. This is not a dynamic repaint-rate measurement. Large production
files that changed: backend.rs2041→2020, ops/stash.rs958→949,
branch_cleanup.rs1064→1044, pr_mode.rs2349→2343. Other >800-line production
files retain their baseline lengths; no large file is hidden in a new crate.

## Actual application timings

Median milliseconds, 7 trials each. Source definitions and stopping conditions are above; these are representative small-fixture observations, not benchmarks of large repositories or proof of statistical significance.

| Path | Before median ms | Contemporary baseline control ms | Matched recovery median ms |
|---|---:|---:|---:|
| commit_select_changed_files | 10.565375 | 10.446375 | 10.212875 |
| diff_open_cold | 243.272959 | 245.371834 | 244.474125 |
| diff_open_warm | 224.115666 | 226.605417 | 225.103833 |
| external_reload_without_watcher_debounce | 67.589000 | 68.940084 | 69.584500 |
| repo_open_uncached | 1609.350458 | 1591.883167 | 1679.581833 |
| startup_snapshot_and_first_window | 208.079250 | 198.539625 | 195.128125 |
| first_return_tab_switch_uncached | 1191.600000 | 1169.617000 | 1268.335667 |

Raw series: `runtime-before.json`, `runtime-control.json` and
`runtime-after-matched.json` in the evidence directory. The control used baseline
code plus the Table/harness changes, which these timed paths do not exercise.
The matched recovery run exited0 (artifact://176;74.68s including31.2s build).
Seven trials do not establish attribution or statistical equivalence: repo-open
and first-return tab presentation remain slower in this sample. No overall
application speedup is claimed.

The first integrated timing file, `runtime-after.json`, inherited
`LEAK_BACKTRACE=1`; it is **diagnostically confounded**, not the comparable after
series. GPUI entity handles capture backtraces whenever that variable is
nonempty (`entity_map.rs:870–871`), including `"0"`. The matched run used an
empty value while preserving leak detection. Its commit-selection/reload
medians are10.213/69.585ms rather than the confounded36.385/116.808ms.
All three usable timing logs report `cached=no` for first return; these
measurements must not be advertised as tab-cache-hit latency.

## Split projection cold/warm timings

| Rows | Cold before ns | Cold after ns | Warm before ns | Warm after ns |
|---:|---:|---:|---:|---:|
| 256 | 428417 | 391292 | 10417 | 83 |
| 4096 | 4479959 | 4332209 | 111375 | 83 |
| 32768 | 35500292 | 34795833 | 885042 | 42 |

Cold performs 31 pairings + 31 move derivations in both versions. Each 101-render warm series changes from **101 pairings / 0 move derivations** to **0 / 0**. Warm nanosecond values are below a meaningful application latency scale; the robust result is elimination of O(rows) pairing/allocation per unchanged render, not the quotient of those tiny numbers. A second run observed 42ns warm at each size. Same-title/same-length replacement reproduced **4 wrong moved marks before → 0 after**.

Existing 8-entry cap is retained; new derived-storage budget is 8 MiB, capacity-accounted conservatively. Source text is weakly observed, never retained by the cache. Active renderer storage is outside the retention budget; oversized projections intentionally bypass retention.

PR conflict follow-through: independent review found a per-frame Arc producer that would defeat the new cache. The existing loaded-preview owner was migrated in the same batch. Main smoke: 101 unchanged renders, zero pairings/move derivations; current-locale headers/placeholders and real one-dark→catppuccin-latte syntax refreshed; outstanding old snapshots preserved. 6 permanent PR tests passed. The locale oracle compares actual current translation, not an invalid assumption that EN and JA must differ.

## Durable completion evidence

- Pre-fix native stash drop deleted the stash but recorded zero persistent entries (artifact://80).
- Seven backend/offline-transport regressions passed: full OID recovery, stash-list drift refusal, history undo/redo with index/worktree preservation, changed-HEAD refusal, cleanup recovery/trust refusal, and real local-shell Git remote command success/error. No SSH authentication/network was simulated as successful.
- Native active/stale stash completion, undo/redo, stale cleanup, EN/JA preflight refusal, and active/stale cleanup Backend-open failure passed alongside every existing scenario (artifact://112, 32.40s including build). Each executed/failed attempt is checked for exactly one durable entry; full OIDs are actually used to restore fixture refs/stashes.
- Native history refusal uses raw Enter after drawing the real modal/focus tree. Standalone stash-drop Enter is absent in the unchanged root router, so stash scenarios invoke the same public handler as the confirmation button; they do not claim keyboard coverage for that old gap.
- Main then corrected required GPUI draw-arena cleanup before final assembled verification; no warning suppression added. Independent source-only reviews found no remaining introduced P1/P2 defects.

## Layout reproduction

Baseline actual Table overflow: 320×240, 125% zoom, EN; 320px parent, last column x320..400. The minimal author/stat shrink correction is isolated from cache/safety work. Card speculation was removed; Card and real Editor History remain guard targets. Post-change objective evidence follows below.

## P2 assembled verification

Focused split tests13/13 and PR tests6/6 passed. Full workspace:1864 passed,
0 failed,30 ignored in802.726s (includes compilation and concurrent isolated
layout-build contention, not an app latency). Fmt0.765s, build9.301s,
CI invariants0.362s, clippy61.718s all exited0. Warning classes match the
pre-existing baseline; no LOC ceiling changed. Assembled native runner exited0
with all existing/P1 scenarios (artifact://146,154.77s including build).

### Final integrated cache repeat

The same31-cold/101-warm instrument was rerun against the integrated source:

| Rows | Cold median ns | Warm median ns | Warm pairings / moves |
|---:|---:|---:|---:|
| 256 | 285167 | 83 | 0 / 0 |
| 4096 | 4558041 | 83 | 0 / 0 |
| 32768 | 36737667 | 83 | 0 / 0 |

All cold series performed31 pairings and31 move derivations. Equal-title/size
replacement still had0 incorrect marks. The integrated PR smoke again passed
101 unchanged renders with0 derivations, current EN→JA presentation,
one-dark→catppuccin-latte syntax and preserved old snapshots. This repeat
overlapped an isolated native-harness run; no cold-speedup or statistically
significant end-to-end latency claim is made. Both temporary test modules and
their counting hooks were removed after execution.

## Native layout proof

Isolated final runner exited0 (artifact://164,370.70s including build/native
teardown):704 Card/Table matrix cells, selected/unselected transitions,
independent natural/allocated heights, finite immediate-child bounds and
non-overlapping neighboring rows; actual Editor History with11 inputs and
first-item reordering at fresh1440/1024/1440 windows;160 real-KagiApp
state/size/zoom/locale paints with immediate Root bounds and fixture fingerprints.
No deeply nested Card width, gap-free adjacency, screenshot or in-place resize
claim is made.

Earlier candidate runs passed layout assertions but failed GPUI leak detection
at shutdown. Plain Rust-handle release and an autorelease pool alone did not
drain MacPlatform's native close queue. The final runner drains that queue
before its pool and GPUI context; leak detection remains enabled and exit is
now clean. This is test-platform lifecycle management, not a production
InputState fix. The full loaded Editor scenario/assertions were retained.

The primary-session assembled run also exited0 (artifact://168;428.11s including
build and native teardown), combining every existing/P1 scenario with the704
matrix cells, actual Editor History,160 state paints and the initial timing
instrument. The timing diagnostic confound above does not invalidate layout,
repository fingerprints or operation-recording assertions. Both timing and
state-sweep throwaway hooks were then removed; the permanent matrix, loaded
Editor checks and native resource teardown remain.

## Final source verification

After removing all temporary hooks, Main ran `cargo fmt --all`, then:

| Command | Exit | Elapsed seconds |
|---|---:|---:|
| `cargo fmt --all -- --check` |0|0.753|
| `cargo build --workspace` |0|7.859|
| `cargo test --workspace` with native GUI environment below |0|643.929|
| `uv run --project ci check-all` |0|0.376|
| `cargo clippy --workspace --all-targets` |0|57.989|

Test environment: `KAGI_GUI_E2E=1 KAGI_NO_RESTORE=1
KAGI_NO_SINGLE_INSTANCE=1 LEAK_BACKTRACE=""`. Final workspace totals:
**1864 passed /0 failed /30 pre-existing ignores**,95 result summaries;
the custom native runner separately logged every existing/P1/P3 scenario and
`PASS all scenarios`, then exited successfully. The704-cell and actual loaded
Editor History cases remain in this final source run; the160-paint state sweep
is the separately executed throwaway instrument described above.

Final clippy has the same12 diagnostic/future-warning classes and13 located
diagnostics as the baseline, with no new or increased message/file pair.
Existing debt is retained rather than suppressed. Full command batch711.80s;
test elapsed time includes compilation and native teardown, not app latency.
Records: `layout-final-{fmt,build,test,invariants,clippy}.{json,log}` and
`final-warning-comparison.json` in the evidence directory.

Final post-hook AST collection again found113 fields,64 impls across51 files,
442 cx.notify sites and50 >800-line Rust files (41 source-only). The native
runner is927→992 LOC; the complete unchanged membership and changed lengths
are in AUDIT.md. Manifests, lock and LOC ratchet remain byte-identical.
