# Conflict formal regression guard

This is the playbook §5 home for the 2026-10-24 investigation: finite lifecycle
interleavings in TLA+/TLC, plus exhaustive finite-domain Python checks for hunk
assembly, readiness, blockers, drafts, Skip evidence, and intent/write admission.
No product code is imported or changed. There are 48 TLC configurations, 38 pure
properties (1,695,574 input/property evaluations), nine TLC negative controls,
30 pure mutants, and 34 saved drill projections with a forged-Skip negative control.
Bounds, extracted source citations, and witnesses are in
[the lifecycle ledger](../../docs/formal/conflict-lifecycle-ledger.md) and
[the pure ledger](../../docs/formal/conflict-pure-ledger.md).

## Run

From the repository root, with `uv`, `curl`, and **Java 21 on PATH**:

```sh
uv run --project formal/conflict formal/conflict/run.sh
```

CI uses `actions/setup-java` Temurin 21. Locally, an existing mise installation
can supply the same runtime without changing the harness:

```sh
mise exec java@temurin-21 -- uv run --project formal/conflict formal/conflict/run.sh
```

Python >=3.11 and an empty third-party dependency set are declared in
`pyproject.toml`/`uv.lock`; Z3 is not used. The launcher downloads the pinned
[TLC v1.8.0 release](https://github.com/tlaplus/tlaplus/releases/tag/v1.8.0)
to `${XDG_CACHE_HOME:-$HOME/.cache}/kagi-formal-conflict/v1.8.0/tla2tools.jar`
on first use and verifies `tla/TLA2TOOLS.sha256` **on every run**, including cached
bytes, before launching Java. The jar is never committed. A bad cached hash is a
tooling failure; remove the reported cache file and rerun. First use requires
network access; a verified cached release runs offline. No mise dependency is
required by CI or by the harness itself.

The one command runs both baselines and demands that every deliberate broken
variant is red. Full-suite JSON is `summary.json`; detailed traces/logs are in
`tla/results/`, `tla/broken-results/`, `tla/trace/results/`, and
`pure/results.json` (all ignored). The recorded `tla/trace/events.json` and
historical `tla/trace/results.json` are committed; raw drill stderr logs are not.
Generated per-trial/batch TLC modules, configs, logs, and the current trace
`summary.json` stay under ignored `tla/trace/results/`. The harness never rewrites
committed model configs or the reconciled documentation ledgers. Up to six
single-worker JVMs check independent lifecycle configs concurrently.

| Exit | Meaning |
|---:|---|
| 0 | All baseline expected verdicts match, and every broken variant was detected |
| 1 | A model expectation changed or a negative control stopped detecting a fault |
| 2 | Invalid arguments, missing tools, checksum failure, timeout, or invalid evidence |

Focused debugging (does not replace the full-suite summary):

```sh
uv run --project formal/conflict formal/conflict/run.sh --suite pure
uv run --project formal/conflict formal/conflict/run.sh --suite tla stale-index
# These commands MUST exit 1: the original HOLD assertion rejects the mutation.
uv run --project formal/conflict formal/conflict/run.sh --suite tla --broken
uv run --project formal/conflict python formal/conflict/pure/predict_ready.py --self-check --broken ignore-marker
```

## Expected outcomes and update policy

**Expected REFUTATIONS are green at the harness level.** They preserve known
holes/questions tracked by issues **#1731–#1738**, not promises that those holes
are acceptable. TLC itself exits 12 (safety counterexample) or 13 (temporal
counterexample); the harness requires the named violation and a trace, not just
any nonzero exit. The pure checks require a nonempty finite domain and a concrete
witness for a REFUTATION. Unknown errors never count as a counterexample.

When a product fix lands, update the extracted model to reflect that fix. Its
old expected REFUTATION then flips the self-check **red** because the property
now HOLDs; update the expectation to HOLD **in the same PR as the fix**. Never
weaken the assertion, silently bless an unexpected verdict, or update the model
without reviewing its Rust/runtime mapping. A product fix alone cannot magically
change these manually extracted models. Runtime-refuted hypotheses below remain
model REFUTATIONS only within their isolated assumptions, not confirmed app bugs.

The TLC `*-broken.cfg` rows remove a known working guard. The ordinary table
expects their counterexamples; a separate `--broken` pass retains the original
HOLD expectations and must exit 1 for all nine, with actual violation witnesses.
Every pure `VARIANTS` entry similarly runs as a child that must exit 1 with a
named mismatch. These expected red children are green only in the outer harness.

The refined `skip-replacement` guard HOLDs because an accepted changed revision
disarms Skip; its removed-guard variant is red. The narrower
`skip-confirm-cli-race` remains REFUTED. `continue-metadata-read` HOLDs under the
full operation-metadata admission guard, with a separate red broken variant.

### Saved runtime projections

The same entry point replays `tla/trace/events.json` through `TraceHarness.tla`.
All 34 observable drill projections must be admitted, no lifecycle line may
remain `UNMAPPED`, and a forged Skip execution without an arm must be rejected
with TLC exit 12. Admission is **not** a proof that the unlogged lease, preflight,
draft-write, or receipt interleaving lifts to the original microstep model.
Independent klog and Git-witness lanes retain their own order; no cross-lane
chronology or successful autosave is invented. See the full
[trace reconciliation appendix](../../docs/formal/conflict-trace-ledger.md).
`--extract --trials PATH` on the mapper explicitly refreshes projections from
external drill artifacts; ordinary runs require no live app or external files.

### Lifecycle table (`tla/run.py:CHECKS`)

| Check/config basename | Property | Expected | Deliberately removed guard |
|---|---|---|---|
| `approval` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-broken` | `NoWriteBeforeApproval` | REFUTE | approval |
| `approval-skip` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-abort` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-dir-file` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-continue` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-merge-continue` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-merge-commit` | `NoWriteBeforeApproval` | HOLD | none |
| `approval-two-instances` | `NoWriteBeforeApproval` | HOLD | none |
| `save-payload` | `ApprovedSavePayloadFrozen` | HOLD | none |
| `save-payload-broken` | `ApprovedSavePayloadFrozen` | REFUTE | payload |
| `approved-revision` | `ApprovedRevisionFrozen` | REFUTE | none |
| `approved-index-race` | `ApprovedRevisionFrozen` | REFUTE | none |
| `approved-two-writers` | `ApprovedRevisionFrozen` | REFUTE | none |
| `stale-worktree` | `StaleSnapshotNeverWrites` | REFUTE | none |
| `stale-index` | `StaleSnapshotNeverWrites` | HOLD | none |
| `stale-index-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision |
| `stale-head` | `StaleSnapshotNeverWrites` | HOLD | none |
| `stale-head-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision |
| `stale-metadata` | `StaleSnapshotNeverWrites` | HOLD | none |
| `stale-metadata-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision |
| `stale-after-preflight` | `StaleSnapshotNeverWrites` | REFUTE | none |
| `manual-capture` | `ManualCapturePersisted` | HOLD | none |
| `manual-capture-broken` | `ManualCapturePersisted` | REFUTE | capture |
| `draft-crash` | `DraftNeverLost` | REFUTE | none |
| `draft-io-quit` | `DraftNeverLost` | REFUTE | none |
| `draft-torn` | `DraftWriteAtomic` | REFUTE | none |
| `draft-two-writers` | `DraftWriteAtomic` | REFUTE | none |
| `draft-lost-update` | `DraftWriteAtomic` | REFUTE | none |
| `two-instance-leases` | `RepoHasAtMostOneLease` | REFUTE | none |
| `two-instance-stale-write` | `StaleSnapshotNeverWrites` | REFUTE | none |
| `receipt-continue` | `ReceiptFollowsMutation` | REFUTE | none |
| `receipt-merge` | `ReceiptFollowsMutation` | REFUTE | none |
| `receipt-skip` | `ReceiptFollowsMutation` | REFUTE | none |
| `receipt-save` | `ReceiptFollowsMutation` | HOLD | none |
| `receipt-save-broken` | `ReceiptFollowsMutation` | REFUTE | receipt |
| `skip-replacement` | `SkipDropsOnlyApprovedStep` | HOLD | none |
| `skip-refresh-broken` | `SkipDropsOnlyApprovedStep` | REFUTE | skipReset |
| `skip-confirm-cli-race` | `SkipDropsOnlyApprovedStep` | REFUTE | none |
| `continue-metadata-read` | `MetadataReadErrorNeverAdmitted` | HOLD | none |
| `continue-metadata-read-broken` | `MetadataReadErrorNeverAdmitted` | REFUTE | metadata |
| `preserve-abort` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none |
| `preserve-skip` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none |
| `preserve-dir-file` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none |
| `merge-tree` | `MergeCommitTreeIsApprovedTree` | REFUTE | none |
| `sibling-invalidation` | `SiblingWorktreesInvalidatedOnSharedRef` | REFUTE | none |
| `stash-non-utf8` | `StashAbortRestoresConflictedBytes` | REFUTE | none |
| `continue-liveness` | `ContinueEventuallyRecordedOrReported` | REFUTE | none |

### Pure table (`pure/predict_*.py`)

| Model | Property | Expected |
|---|---|---|
| `predict_hunks` | `SideStateConsistent` | HOLD |
| `predict_hunks` | `SideStateChoiceMaskEquivalentIncludingEmpty` | REFUTE |
| `predict_hunks` | `SelectionPresenceResolvesEvenEmpty` | HOLD |
| `predict_hunks` | `ChildTriStateAggregation` | HOLD |
| `predict_hunks` | `AssembleAllFourOrdersAndProvenance` | HOLD |
| `predict_hunks` | `AssembleBlockOrderStable` | HOLD |
| `predict_hunks` | `AssembleNoSyntheticMarkers` | HOLD |
| `predict_hunks` | `AssembleNoMarkerLeak` | REFUTE |
| `predict_hunks` | `UndecideOnlyOnDeselect` | HOLD |
| `predict_hunks` | `OrderApplicability` | HOLD |
| `predict_hunks` | `InvalidLineFailureIsNoOp` | REFUTE |
| `predict_hunks` | `MalformedSelectionNeverPanics` | REFUTE |
| `predict_hunks` | `ChoiceSelectionPreservesSourceChoices` | HOLD |
| `predict_hunks` | `RegionAssemblyPreservesOrder` | HOLD |
| `predict_ready` | `ReadyImpliesNoMarkers` | HOLD |
| `predict_ready` | `ManualDivergenceIsTextual` | HOLD |
| `predict_ready` | `EmptyResultIsAResolution` | HOLD |
| `predict_ready` | `MissingResultOrModelNotDivergent` | HOLD |
| `predict_blockers` | `BlockersOrderedAndComplete` | HOLD |
| `predict_blockers` | `IndexContextIdentityErrorsFailClosed` | HOLD |
| `predict_blockers` | `MessageReadErrorNeverFailOpen` | REFUTE |
| `predict_blockers` | `MessageContentGate` | HOLD |
| `predict_blockers` | `UIBlockerPrecedence` | HOLD |
| `predict_drafts` | `DraftNeverAdoptsUnknownAuthority` | HOLD |
| `predict_drafts` | `DraftPayloadCompatibility` | HOLD |
| `predict_drafts` | `LatestExplicitNullOutranksOldBody` | HOLD |
| `predict_drafts` | `NewestCompatibleRecoveryWins` | HOLD |
| `predict_drafts` | `UnknownOwnerCoalescenceIsFileOnly` | HOLD |
| `predict_drafts` | `OnlyPendingRecoveryOutranksDisk` | HOLD |
| `predict_drafts` | `DedupMovesLatestToTail` | HOLD |
| `predict_skip` | `SkipClassificationNoFalseSuccess` | REFUTE |
| `predict_skip` | `BrokenSignalDominatesMovement` | HOLD |
| `predict_skip` | `NextConflictIsSuccessOnObservedMovement` | HOLD |
| `predict_skip` | `FinishedRequiresCleanRepository` | HOLD |
| `predict_skip` | `NonzeroUnchangedIsNoProgress` | HOLD |
| `predict_intent` | `IntentAdmissionConjunctive` | HOLD |
| `predict_intent` | `PlanApprovalConjunctive` | HOLD |
| `predict_intent` | `BeginWriteConjunctive` | HOLD |

## Runtime reconciliation and limits

The ledgers preserve the historical extraction snapshot and line references;
those references are not an executable link to current Rust. Their new headers
reconcile real drills on main `834a1796`: failed draft I/O/quit, failed preservation
before Abort, WT-only stale Save, and same-file independent-instance decision loss
were reproduced. The draft race drill is not the distinct-path/torn-JSON trace.
Merge replanning was observed, but the final visible staged list had refreshed,
so no unseen-tree commit was established.

C5 unreadable-message end-to-end fail-open was blocked by the full metadata guard.
Skip TOCTOU's naive drill was blocked by armed-control refresh. C6's proposed
external-final-skip drill returned Git 128, not its required zero/no-movement
antecedent. All receipt-after-kill samples at **479 ms or later** retained exactly
one successful receipt; earlier mutation/accounting gaps were not sampled.
These are runtime-refuted drill hypotheses, not global proofs or grounds to erase
isolated model counterexamples. See both reconciliation headers for exact scope.

The models do not run Rust, emulate the GUI, or establish model/code equivalence.
HOLD is bounded by the finite equality/order quotients and vector/text bounds;
leases are per modeled instance, crashes are explicit events, and cached marker
verdicts are supplied inputs. The advisory `formal-conflict` CI job runs this same
entry point with a five-minute job limit; it is not part of `blocking-ci`.
