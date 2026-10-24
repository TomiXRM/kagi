# Conflict lifecycle trace-checking (2026-10-24)

Companion to the [lifecycle ledger](conflict-lifecycle-ledger.md); split to keep
each new documentation file below 800 lines. Historical local evidence paths
below are provenance, not runtime prerequisites.

This section supersedes the earlier unqualified Skip replacement claim and the
pure-C5 end-to-end fail-open hypothesis. Product source was read-only at
`/tmp/kagi-conflict-hunk-20261023`, HEAD `6f9696df` (the assigned file contents
match runtime build `834a1796`). No Cargo or new GUI drill was run.

### Refined invariant table

| Check / invariant | Previous claim | Refined TLC result | Runtime/code reason |
| --- | --- | --- | --- |
| `skip-replacement` / `SkipDropsOnlyApprovedStep` | REFUTE without distinguishing refresh timing | **HOLD** when refresh precedes second confirm | Changed-mode adoption resets `skip_approval=None`; unchanged observations retain the original armed revision. The historical drill confirms this first-line defence. |
| `skip-refresh-broken` / `SkipRefreshDisarmsChangedStep` | Negative control of final wrong-step effect | **REFUTE** | Remove only refresh disarming and assert the independent arm invariant, even though #1746's backend check would now stop the write. |
| `skip-confirm-cli-race` / `SkipDropsOnlyApprovedStep` | Historical **REFUTE**, 51 states | **HOLD** after #1746 D2 | `ConflictSkipPlan` carries the original operation/full revision; `preflight_skip_revision` runs before preservation and immediately before Git. Rust `skip_refuses_replaced_approved_step_and_preserves_the_next_conflict` now reproduces and refuses the actual external-step replacement. |
| `skip-prewrite-broken` / `SkipDropsOnlyApprovedStep` | New negative control | **REFUTE** | Remove the frozen revision checks; a replacement at preflight/preservation/CLI dispatch drops the unapproved step. |
| `continue-metadata-read` / `MetadataReadErrorNeverAdmitted` | Pure C5 suggested end-to-end fail-open | **HOLD**, 87 states | `approval_metadata.rs:15-21,42-54,111,139` propagates unreadable MERGE_MSG; `conflicts.rs:1266,1292` observes the complete revision before staging. `MetadataReadFailure` enables `MetadataRefused`, not writes. `DrillPreexistingComplete.md:69-78` records refusal and unchanged Git. |
| `continue-metadata-read-broken` | New negative control | **REFUTE**, 33 states | Remove the strict metadata-read admission guard; the unreadable observation reaches preflight. |
| All other lifecycle expectations | Existing checks | **Unchanged** | The historical #1746 D2/#1733-only run had 52 checks and 85,292 summed states. Integrating #1732's atomic/locked draft transitions passes 55 checks and 52,280 summed distinct states; all 15 meaningful broken variants remain red. Every previous HOLD from both parents stays HOLD. The pure blocker-function C5 result is not rewritten: only its invalid end-to-end extrapolation is corrected. |

`reset_all_target` in `actions.rs:134,161,185` is the separate Reset-All arm,
not Skip's arm. The refinement uses the actual `skip_armed` reset through
changed-mode adoption, rather than conflating these controls.

The residual Skip counterexample is:
`Freeze(step 0) → Approve(second confirm) → ReplaceOperation(step 1) →
Preflight(no expected-step guard) → Preserve → Skip(step 1)`.
Refresh **before** the second confirm prevents this; a later UI reset cannot
revoke an already dispatched CLI job. This remains source-grounded model risk,
not a runtime-witnessed race or proof that every external advance is unsafe.

### Trace contract and admission scope

`tla/trace/klog2events.py` extracts the map §7 strings from **34 trials / 46
stderr files**, including all `stderr.prev.*.log` files for these drills, and
records each source line, raw text, full before/after witness, and input SHA-256
in `events.json`. `Trace1.tla`–`Trace34.tla` each contain the constant event
sequence and a cfg. `AllTraces.tla` checks their union in one finite TLC search,
not simulation. `TraceHarness.tla` updates the set of possible observable
runtime states via `ConflictLifecycle!RuntimeAdvance`; `TraceAdmitted` requires
that set to remain nonempty. An impossible event is retained as an empty set
and fails the invariant, rather than disappearing as a disabled transition.
A deliberately forged Skip execution without an arm is **REFUTED**.

**Admission means the observable projection is admitted, not that the missing
internal pc/lease/draft interleaving has been proved to lift to the original
microstep model.** This distinction is required by the actual evidence:

- Stderr has no timestamps and actions.json has no matching log offsets.
  The klog and Git-witness lanes preserve their own order; their concatenation
  is only a representative of independent projections, not invented chronology.
- Relaunch replaces actions.json. Some current stderr files contain an old
  process's tail after a new startup prefix (notably LeaveRebase). Every raw log
  is checked as its own segment; duplicate-looking tails are not counted as
  newly witnessed Git executions. This prevents a false total-order claim.
- Per-hunk/manual adoption, successful autosave, freeze, second confirm, lease,
  preflight, individual WT/index writes, receipt append, and release lack
  dedicated logs. Screenshots, saved `buffer.json`, `ls-files --stage`, blob
  hashes, and operations.jsonl in the drill reports substitute for selected
  observations; they do not establish the unlogged interleaving.
- A status digest is **not** a WT-content digest. Metadata `null` means absent
  **or unreadable**, especially in C5; it is never treated as proof of completion.
- A detection line omits the revision. Its candidates permit unchanged-revision
  retention or changed-revision disarming. The refresh log itself does not
  unconditionally reset the arm (`reload.rs:367` logs after detection).
- Repeated detection, failed choice, merge-panel handoff, and independent
  witness sampling are explicit observation transitions with code citations.
  Unknown lifecycle strings fail closed as `UNMAPPED`, not silently ignored.

No supplied observable projection remained rejected after refinement.
All trials retain **log gaps**; two additionally identify the resolved model
gaps. The JSON reports these limitations per trial rather than declaring the
missing events observed.

### Trace-check table

`P` = admitted observable projection with the unobservable steps above.
`L` = log gap; `M+L` = refined model gap plus remaining log gap.
Events include mapped log lines, segment boundaries, and witness transitions.

| Trial | Events mapped | Admitted? | Gap type | Refinement |
| --- | ---: | --- | --- | --- |
| LeaveAbort | 25 | P | L | Abort arm/execution, restart segments |
| LeaveCrash | 9 | P | L | Restart segment; successful draft write unlogged |
| LeaveExternalIndex | 22 | P | L | Independent external-index witness |
| LeaveExternalWT | 21 | P | L | Save footer plus stage-0/WT report evidence |
| LeaveGrace | 15 | P | L | Draft/restart observations |
| LeaveGraceResume | 7 | P | L | Restart observations |
| LeaveIO | 42 | P | L | DraftFailure; no invented autosave success |
| LeaveRebase | 87 | P | L | Continue/Abort, separate raw relaunch segments |
| LeaveSaveA | 21 | P | L | Save aggregate and Git witness |
| LeaveSaveB | 13 | P | L | External saved-file refresh |
| LeaveTwoA | 15 | P | L | Draft observations, same-file decision loss |
| LeaveTwoB | 8 | P | L | Independent instance; no inferred global lease |
| mc1 | 16 | P | L | Manual edit observations |
| preexisting-comment | 64 | P | M+L | MetadataRefused; readable merge-panel route later |
| preexisting-directory | 171 | P | L | Failed choices, Save aggregates, merge handoff |
| preexisting-file | 37 | P | L | Failed choice and merge handoff |
| preexisting-inspection | 11 | P | L | Resolved merge inspection |
| seq-abort | 23 | P | L | Abort aggregate and relaunch segment |
| seq-abort-loss | 17 | P | L | Successful Abort despite unlogged preservation error |
| seq-abort-ro | 16 | P | L | Abort control; older draft survives |
| seq-chain-full | 26 | P | L | Repeated Continue across conflict steps |
| seq-cherry-abort | 15 | P | L | Cherry-pick Abort |
| seq-crash | 23 | P | L | Continue/relaunch; no inferred receipt-close race |
| seq-kill-150 | 20 | P | L | Continue/relaunch segments |
| seq-kill-300 | 20 | P | L | Continue/relaunch segments |
| seq-kill-50 | 18 | P | L | Truncated completion suffix allowed |
| seq-kill-600 | 22 | P | L | Continue/relaunch segments |
| seq-rebase-complete | 32 | P | L | Save, review, Continue, clear |
| seq-revert | 16 | P | L | Revert review/Continue |
| seq-skip | 15 | P | L | Arm then executed Skip |
| seq-skip-next | 18 | P | L | Skip followed by next conflict detection |
| seq-skip-race | 27 | P | M+L | Changed-revision disarm; new arm, no stale execution |
| seq-stash | 13 | P | L | Stash direct staging continuation, no sequencer review |
| seq-stash-abort | 14 | P | L | Stash Abort |

### Model ↔ runtime confirmed pairs

| Refuted model check | Real trial | Strength and substitute observation |
| --- | --- | --- |
| `draft-io-quit` / `DraftNeverLost` | LeaveIO | Runtime consequence confirmed: logged autosave I/O failures, empty draft directory, missing manual marker on restart; `DrillLeaveResume.md:7-24`. Unchanged Git alone would not prove this. |
| `preserve-abort` / `NoDestructiveProgressAfterPreservationFailure` | seq-abort-loss | Runtime consequence confirmed: chmod-000 directory, visible B+A decision, executed Abort and changed Git, retained buffer contains only A and no recoveries; `DrillSequencers.md:15-25`. Runtime failure precedes a write, whereas the existing minimal model counterexample chooses a truncation failure. |
| `stale-worktree` / `StaleSnapshotNeverWrites` | LeaveExternalWT | Runtime consequence confirmed: external WT blob `b90a253a1cbd4b9a5296eacbd561b0cb5245118a` replaced by stage-0/WT `fa1707ed6578871525bc9ab3cf5f7e7a5a5326ab`; `DrillLeaveResume.md:43-65`. Not the separate post-preflight add_path race. |
| `draft-lost-update` / `DraftWriteAtomic` | LeaveTwoA + LeaveTwoB | **Mechanism-level confirmation only**: same-file, different-hunk overwrite, not the model's distinct-path interleaving or torn JSON; `DrillLeaveResume.md:26-41`. Do not promote this to an exact trace reproduction. |
| #1732 fixed `draft-lost-update` / `DraftWriteAtomic` | LeaveResume §2a before/after replay | Plain binary on `834a1796`: FAIL, hunk 0 unresolved after restart. Implementation `f5931086`: PASS, incoming hunk 0 plus current-line hunk 1 survive, 2/3 resolved. Screenshots/oracles `/tmp/kagi-1732-evidence/{before,after}/`; no staged-path/blob/mode/flag or operation/HEAD change. Fixed raw-index hash drift reconciled to `.gitmodules` ctime/inode only. This confirms the same-file mechanism repair, not a native torn-write or exact distinct-path trace. |

Not witnessed: post-confirm Skip race; torn JSON; distinct-path stale merge/write;
receipt-close race; post-preflight index/WT race; C6 zero-exit/no-movement.
The normal and refreshed Skip traces and unreadable-MERGE_MSG refusal are
runtime confirmations of the refined guards, not counterexamples to them.

### Reproduction and files

From the repository root run
`uv run --project formal/conflict formal/conflict/run.sh`.
It checks both invariant/pure expectations and the saved runtime traces, returning
0 only when all expectations and negative controls agree. To deliberately
re-extract observations, run the mapper with `--extract --trials PATH`; ordinary
checks use committed `formal/conflict/tla/trace/events.json` without a live app.
Invariant checks use at most six independent single-worker JVMs; trace checks
use one batched search plus one negative-control search. Current timing and TLC
exits are recorded in `formal/conflict/tla/results/summary.json` and
`formal/conflict/tla/trace/results/summary.json`; committed `trace/results.json`
preserves the original trace-reconciliation receipt.
The original complete formal-workspace run exited **0 in 33.42 seconds wall time**:
28.769 seconds for all 48 invariant checks, 3.898 seconds for the 34-trial
trace batch plus negative control, with launch overhead accounting for the
remainder. An earlier sequential runner took 149.99 seconds under load;
the retained bounded runner avoids that startup bottleneck without changing
the checks. Timing is measured on this shared workstation, not a universal
scheduling guarantee.

Repository-owned source/input files are `formal/conflict/tla/ConflictLifecycle.tla`,
`tla/run.py`, all lifecycle `.cfg` files, `tla/trace/check_lifecycle.py`,
`tla/trace/klog2events.py`, `tla/trace/TraceHarness.tla`, the saved `events.json`,
and the historical trace `results.json`. Four refinement configs live beside the
trace mapper: `skip-refresh-broken.cfg`, `skip-confirm-cli-race.cfg`,
`continue-metadata-read.cfg`, and `continue-metadata-read-broken.cfg`.
The mapper generates `Trace1` through `Trace34`, `AllTraces`, and `RejectedSkip`
TLC modules/configs beneath ignored `tla/trace/results/`, along with TLC logs,
state directories, and the current `summary.json`. Raw drill stderr logs are
not committed. The compact saved JSON preserves every projected observation and
source hash without making an external evidence directory a runtime dependency.
