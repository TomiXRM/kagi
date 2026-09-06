# Recovery state

The recovery checkpoint below predates PR #481 publication. The latest
user-requested integration is recorded in the final section.

- Objective: execute the original `request.md` in `/Users/tomixrm/Dev/sandbox/git-client`; complete 2–5 independently verified local commits, not a wholesale rewrite. Preserve the safety pipeline, tests and LOC ratchet. No push, release, tags, authentication-dependent operations or writes to real repositories for testing.
- Start HEAD: `5f7a4c80618d270010a1360a4c4a062c75147be5`.
- Original branch: `main`; original status: `?? request.md`. Remote default is `main`; `master` does not exist. Treat user's `master` as the actual default branch.
- Original user checkout and Claude worktree are preserved.
- Recovery worktree: `/Users/tomixrm/Dev/sandbox/kagi-astra-recovery`, branch `dev`, created from `main` at start HEAD. User explicitly chose to defer the genuinely unintegrated package-manager branch; no blind merges or main modifications were performed.
- User-requested 30-minute delay completed (`sleep 1800`: 1800.01 seconds).
- Completed batch: mechanical backend recorder extraction, commit `47678c0`; fmt/build/workspace test (1848 passed, 0 failed, 30 ignored), CI invariants and independent review passed. No behavior change.
- Completed P1: durable operation completion, commit `91097e86`; workspace1855 passed/0 failed/30 ignored, fmt/build/CI and native success/refusal/stale-tab/open-failure/EN/JA passed. Independent reviews passed; the one new unused test import was removed and targeted clippy passed.
- Integration audit: public remote heads matched locally recorded `origin/main`;114 local/tracking refs checked by ancestry, patch equivalence and squash/history review. Evidence: `/tmp/kagi-astra-recovery-evidence/branch-equivalence.json`. Packaging was deferred explicitly, not silently treated as merged.
- Fable safety review completed at `/tmp/kagi-astra-recovery-evidence/fable-safety-design.md`: persistence belongs at the mutation boundary, not guarded UI completion. #476 is now integrated remotely; its original worktrees and local main remain untouched.
- Writers are isolated: `kagi-astra-cache` (split cache), `kagi-astra-layout` (Card + bounds tests), `kagi-astra-backend` (recording boundary), `kagi-astra-optests` (backend regressions). Main alone writes/integrates `kagi-astra-recovery`; no simultaneous writers share a worktree.
- Completed source HEAD: `1291cc903cb8e29eb26ed9f37a47db8e081ed2d5` (P3). Final delivery HEAD is the docs-only commit adding FINAL.md, recoverable with `git log --diff-filter=A -1 --format=%H -- docs/agent-loop/2026-09-astra-recovery/FINAL.md`; the terminal handoff gives its full SHA. All implementation work is committed: minimal Table shrink policy,704-cell/Editor History regressions and native teardown included. Temporary hooks are removed. Final source fmt/build/GUI-enabled workspace/CI/clippy passed:1864/0/30 ignored, no new warnings.
- Four read-only audits completed: ArchitectureAudit, PerformanceAudit, LayoutAudit, SafetyAudit. Full reports materialized under `/tmp/kagi-astra-recovery-evidence/{architecture,performance,layout,safety}-audit.md`. Main's metadata corrects scout's declared-members count: 11 actual packages including implicit gpui-terminal.
- Baseline: fmt/build/clippy/check-all passed (0.775/114.347/116.489/0.592s). Workspace test initially 1814 passed/2 failed/2 ignored (404.117s); cloned xtask binaries embedded a deleted CARGO_MANIFEST_DIR. Targeted artifact rebuild passed 5/5; full baseline rerun passed (79.78s; artifact://47).
- GUI E2E passed all 8 existing scenarios (15.74s). Actual isolated GUI launched with fixture, first commit/file opened, watcher installed; stopped cleanly. CoreGraphics found no on-screen window for the exact owned PID; GPUI screenshots report render_to_image unavailable. Objective bounds lane works (oplog row22→69px, following row738→785px).
- LSP unavailable because rust-analyzer is not installed for the pinned 1.96.0 toolchain; structural activation schema is broken. Compiler validation and explicitly labeled static-analysis fallbacks are required.
- Split cache reproduced4 incorrect marks→0;32768-row warm885042ns→42ns,101 pairings→0. Main's PR smoke passed101 warm renders with no derivation, current-locale headers and refreshed theme syntax while old snapshots stayed immutable. Reviewed five-file cutover is integrated with temporary hooks removed. Full P2 validation passed.
- Layout: Table overflow reproduced at320×240/125%/EN, final column x320..400 outside320px row. Minimal author/stat shrink fix passes704-cell Card/Table matrix and actual Editor History reorder checks. Card production remains unchanged. The160 real-App state paints prove immediate Root bounds only. Native in-place resize/screenshots and deeply nested Card metadata width are not verified.
- Actual application baseline, contemporary control and matched recovery each use7 trials. Initial after timing inherited LEAK_BACKTRACE=1 and is excluded; matched recovery used an empty value with leak detection still enabled. Raw tab key says cached but logs prove first-return uncached presentation. METRICS.md preserves exact medians and the slower repo/tab samples without a blanket speedup claim.
- Closeout documents contain the complete evidence and narrow architecture/migration corrections. Resolve the final delivery commit as above before resuming; once that docs-only commit is present, no recovery implementation remains. Next review target is cleanup's remote-success/local-failure partial outcome; human-only visual checks are listed in FINAL.md. Native teardown preserves leak detection; final structural counts and unchanged manifests/lock/LOC ratchet are recorded in METRICS.md.

## Handoff contract

Read AGENTS.md, this file, current status, last 20 commits, and uncommitted diff. Do not repeat unchanged audits. Update this file before delegation or context handoff. Main owns integration, all validation, GUI execution and local commits; no simultaneous writers in one worktree.

## PR #481 — main into dev integration

- User explicitly requested conflict resolution and a merge into `dev`, not a merge of the PR into `main`.
- Input parents: published dev `8401c16f89204a97af0b26a1db9ff76f625275df` and main `b3bf0e4b49f13657412364ccebbe887a8b3860ec` (#479/#480 worktree writes).
- The sole content conflict was `tests/gui_e2e_runner.rs`: keep main's worktree commit/amend/discard scenarios before dev's native teardown. All recovery scenarios and leak assertions remain. Two incoming documentation lines were corrected to avoid Markdown-list lint and describe the actual Undo assertion.
- Verification: native runner exited0 in327.51s, including both incoming worktree scenarios and all recovery/legacy scenarios. Separate full workspace:1868 passed/0 failed/30 existing ignores; fmt/build/CI/clippy succeeded. Final clippy retains only the13 previously located warnings.
- The initial combined command hit its measurement wrapper's900s timeout; it is not counted as a successful workspace run. Both independent reruns completed, without changing tests, assertions or the checked-in runner's timeout policy.
- Delivery uses a normal merge commit with the two input parents above and an ordinary push to `dev`. The exact merge SHA and current PR state are in the terminal handoff/PR #481. Original recovery metrics remain historical; no new performance measurement or broad refactor was made.
