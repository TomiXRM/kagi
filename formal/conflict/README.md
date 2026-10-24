# Conflict formal regression guard

This is the playbook §5 home for the 2026-10-24 investigation: finite lifecycle
interleavings in TLA+/TLC, plus exhaustive finite-domain Python checks for hunk
assembly, readiness, blockers, drafts, Skip evidence, and intent/write admission.
No product code is imported or changed. There are 57 TLC configurations, 39 pure
properties (1,695,651 input/property evaluations), 16 TLC negative controls,
36 pure mutants, and 34 saved drill projections with a forged-Skip negative control.
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
holes/questions tracked by **#1731–#1733, #1740, #1743–#1746 (model-external runtime findings: #1734–#1738)**,
not promises that those holes are acceptable. The Issue columns below distinguish
product holes, owner questions, deliberate negative controls, and runtime contracts.
TLC itself exits 12 (safety counterexample) or 13 (temporal
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
HOLD expectations and must exit 1 for every guard mutation, with actual violation witnesses.
Every pure `VARIANTS` entry similarly runs as a child that must exit 1 with a
named mismatch. These expected red children are green only in the outer harness.

The refined `skip-replacement` guard HOLDs because an accepted changed revision
disarms Skip. #1746 D2 also makes `skip-confirm-cli-race` HOLD: the plan carries
the original full conflict revision, rechecked immediately before CLI execution.
`skip-refresh-broken` now checks the first-line disarm invariant independently
of that backend defence; `skip-prewrite-broken` removes revision checks and is red.
`SkipClassificationNoFalseSuccess` HOLDs after C6 requires observed movement on
zero exit; `zero-exit-advanced` restores the old classifier and is red.
`continue-metadata-read` HOLDs under the full operation-metadata admission guard,
with a separate red broken variant.

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

| Check/config basename | Property | Expected | Deliberately removed guard | Issue |
|---|---|---|---|---|
| `approval` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-broken` | `NoWriteBeforeApproval` | REFUTE | approval | Negative control; no product issue |
| `approval-skip` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-abort` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-dir-file` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-continue` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-merge-continue` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-merge-commit` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `approval-two-instances` | `NoWriteBeforeApproval` | HOLD | none | CONTRACT |
| `save-payload` | `ApprovedSavePayloadFrozen` | HOLD | none | CONTRACT |
| `save-payload-broken` | `ApprovedSavePayloadFrozen` | REFUTE | payload | Negative control; no product issue |
| `approved-revision` | `ApprovedRevisionFrozen` | REFUTE | none | #1743 |
| `approved-index-race` | `ApprovedRevisionFrozen` | REFUTE | none | #1743 |
| `approved-two-writers` | `ApprovedRevisionFrozen` | REFUTE | none | #1743 |
| `stale-worktree` | `StaleSnapshotNeverWrites` | HOLD | none | #1733 fixed: captured WT provenance checked before mutation |
| `stale-worktree-broken` | `StaleSnapshotNeverWrites` | REFUTE | worktree | Negative control; remove #1733 WT provenance guard |
| `stale-worktree-before-choice` | `StaleSnapshotNeverWrites` | HOLD | none | #1733: watcher refresh before any choice retains original WT provenance |
| `stale-worktree-before-choice-broken` | `StaleSnapshotNeverWrites` | REFUTE | materialization | Negative control; watcher adopts changed WT as fresh source |
| `stale-index` | `StaleSnapshotNeverWrites` | HOLD | none | CONTRACT |
| `stale-index-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision | Negative control; no product issue |
| `stale-head` | `StaleSnapshotNeverWrites` | HOLD | none | CONTRACT |
| `stale-head-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision | Negative control; no product issue |
| `stale-metadata` | `StaleSnapshotNeverWrites` | HOLD | none | CONTRACT |
| `stale-metadata-broken` | `StaleSnapshotNeverWrites` | REFUTE | revision | Negative control; no product issue |
| `stale-after-preflight` | `StaleSnapshotNeverWrites` | REFUTE | none | External CLI index-write TOCTOU after the last check; not closed by #1733 |
| `manual-capture` | `ManualCapturePersisted` | HOLD | none | CONTRACT |
| `manual-capture-broken` | `ManualCapturePersisted` | REFUTE | capture | Negative control; no product issue |
| `draft-crash` | `DraftNeverLost` | REFUTE | none | #1732; capture before acknowledged flush can be lost |
| `draft-io-quit` | `DraftNeverLost` | REFUTE | none | #1731 |
| `draft-torn` | `DraftWriteAtomic` | HOLD | none | #1732 |
| `draft-torn-broken` | `DraftWriteAtomic` | REFUTE | draftAtomic | Negative control; truncate primary |
| `draft-two-writers` | `DraftWriteAtomic` | HOLD | none | #1732 |
| `draft-two-writers-broken` | `DraftWriteAtomic` | REFUTE | draftLock | Negative control; unlocked read/merge/write |
| `draft-lost-update` | `DraftWriteAtomic` | HOLD | none | #1732 |
| `draft-lost-update-broken` | `DraftWriteAtomic` | REFUTE | draftLock | Negative control; stale merge snapshot |
| `two-writer-recovery` | `DraftWriteAtomic` | HOLD | none | #1732: retain an earlier body after stale-editor other-file autosave |
| `two-writer-recovery-broken` | `DraftWriteAtomic` | REFUTE | preMergeRecovery | Negative control; deduplicate against the pre-merge editor |
| `two-instance-leases` | `RepoHasAtMostOneLease` | REFUTE | none | #1746 (owner QUESTION) |
| `two-instance-stale-write` | `StaleSnapshotNeverWrites` | REFUTE | none | Two-instance HOLD expected via #1746 D3 repository lock; external CLI residual stays REFUTE |
| `receipt-continue` | `ReceiptFollowsMutation` | REFUTE | none | #1740 |
| `receipt-merge` | `ReceiptFollowsMutation` | REFUTE | none | #1740 |
| `receipt-skip` | `ReceiptFollowsMutation` | REFUTE | none | #1740 |
| `receipt-save` | `ReceiptFollowsMutation` | HOLD | none | CONTRACT |
| `receipt-save-broken` | `ReceiptFollowsMutation` | REFUTE | receipt | Negative control; no product issue |
| `skip-replacement` | `SkipDropsOnlyApprovedStep` | HOLD | none | CONTRACT |
| `skip-refresh-broken` | `SkipRefreshDisarmsChangedStep` | REFUTE | skipReset | Negative control; no product issue |
| `skip-confirm-cli-race` | `SkipDropsOnlyApprovedStep` | HOLD | none | CONTRACT after #1746 D2 |
| `skip-prewrite-broken` | `SkipDropsOnlyApprovedStep` | REFUTE | skipRevision | Negative control for #1746 D2 |
| `continue-metadata-read` | `MetadataReadErrorNeverAdmitted` | HOLD | none | CONTRACT at runtime (metadata guard) |
| `continue-metadata-read-broken` | `MetadataReadErrorNeverAdmitted` | REFUTE | metadata | Negative control; no product issue |
| `preserve-abort` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none | #1731 |
| `preserve-skip` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none | #1731 |
| `preserve-dir-file` | `NoDestructiveProgressAfterPreservationFailure` | REFUTE | none | #1731 |
| `merge-tree` | `MergeCommitTreeIsApprovedTree` | REFUTE | none | #1743 |
| `sibling-invalidation` | `SiblingWorktreesInvalidatedOnSharedRef` | REFUTE | none | #1744 |
| `stash-non-utf8` | `StashAbortRestoresConflictedBytes` | REFUTE | none | #1745 |
| `continue-liveness` | `ContinueEventuallyRecordedOrReported` | REFUTE | none | #1740 |

The #1732 draft guards cite `resolution/draft_storage.rs`'s OS lock, checked
content-CAS read, and ADR-0191 atomic temp/sync/rename writer. Its backend
`draft_writer.rs` captures memory immediately and offers one ordered background
writer with latest-snapshot coalescing for at most 250 ms before starting.
An in-flight fsync can extend that window. Queue acceptance is **not** a durable
acknowledgment: `DraftSave::wait()` / blocking `flush()` must return `Ok`.
Consequently the unchanged strong `draft-crash` capture invariant remains
REFUTE; the model retains the real pre-commit crash transition rather than
quotienting it away. The preceding acknowledged document remains valid.
This is not a power-loss guarantee. The lock quotient models successful
acquisition, not the timeout's UI presentation; backend child tests cover the
two-second error and SIGKILL release. GUI consumers must await barriers off-thread
and preserve pending/failed close gates; the backend API alone does not wire them.

`two-writer-recovery` refines the successful locked transaction into distinct
primary and recovery bodies: A saves file 1, B replaces it while preserving A,
then A saves file 2 twice while its own file-1 body remains unchanged. Recovery
deduplication compares the actual post-merge primary, not that stale local body.
The pure `TwoWriterRecoveryRetained` property also covers assembled-hunk body/state
overrides and both body orderings. The Rust subprocess regression exercises both
blocking/queued writers and commit orders, checks exact bytes and identity through
the read-only inspector's owned recovery API, and rotates `.prev` repeatedly.



### Pure table (`pure/predict_*.py`)

| Model | Property | Expected | Issue |
|---|---|---|---|
| `predict_hunks` | `SideStateConsistent` | HOLD | CONTRACT |
| `predict_hunks` | `SideStateChoiceMaskEquivalentIncludingEmpty` | HOLD | #1746 C1 fixed: explicit empty-side adoption |
| `predict_hunks` | `SelectionPresenceResolvesEvenEmpty` | HOLD | CONTRACT |
| `predict_hunks` | `ChildTriStateAggregation` | HOLD | CONTRACT |
| `predict_hunks` | `AssembleAllFourOrdersAndProvenance` | HOLD | CONTRACT |
| `predict_hunks` | `AssembleBlockOrderStable` | HOLD | CONTRACT |
| `predict_hunks` | `AssembleNoSyntheticMarkers` | HOLD | CONTRACT |
| `predict_hunks` | `MarkerGateRespectsChosenSourceProvenance` | HOLD | #1746 C2 fixed: manual-only gate; collision-free materialization and width-aware parser (#1751) |
| `predict_hunks` | `UndecideOnlyOnDeselect` | HOLD | CONTRACT |
| `predict_hunks` | `OrderApplicability` | HOLD | CONTRACT |
| `predict_hunks` | `InvalidLineFailureIsNoOp` | HOLD | #1746 C3 fixed: validate before materialization |
| `predict_hunks` | `MalformedSelectionNeverPanics` | HOLD | #1746 C4 fixed: validated dimensions and checked source lookup |
| `predict_hunks` | `ChoiceSelectionPreservesSourceChoices` | HOLD | CONTRACT |
| `predict_hunks` | `RegionAssemblyPreservesOrder` | HOLD | CONTRACT |
| `predict_ready` | `ReadyImpliesNoMarkers` | HOLD | CONTRACT |
| `predict_ready` | `ManualDivergenceIsTextual` | HOLD | CONTRACT |
| `predict_ready` | `EmptyResultIsAResolution` | HOLD | CONTRACT |
| `predict_ready` | `MissingResultOrModelNotDivergent` | HOLD | CONTRACT |
| `predict_blockers` | `BlockersOrderedAndComplete` | HOLD | CONTRACT |
| `predict_blockers` | `IndexContextIdentityErrorsFailClosed` | HOLD | CONTRACT |
| `predict_blockers` | `MessageReadErrorNeverFailOpen` | REFUTE | C5: CONTRACT at runtime (metadata guard, `continue-metadata-read` HOLD) |
| `predict_blockers` | `MessageContentGate` | HOLD | CONTRACT |
| `predict_blockers` | `UIBlockerPrecedence` | HOLD | CONTRACT |
| `predict_drafts` | `DraftNeverAdoptsUnknownAuthority` | HOLD | CONTRACT |
| `predict_drafts` | `DraftPayloadCompatibility` | HOLD | CONTRACT |
| `predict_drafts` | `LatestExplicitNullOutranksOldBody` | HOLD | CONTRACT |
| `predict_drafts` | `NewestCompatibleRecoveryWins` | HOLD | CONTRACT |
| `predict_drafts` | `UnknownOwnerCoalescenceIsFileOnly` | HOLD | CONTRACT |
| `predict_drafts` | `OnlyPendingRecoveryOutranksDisk` | HOLD | CONTRACT |
| `predict_drafts` | `DedupMovesLatestToTail` | HOLD | CONTRACT |
| `predict_drafts` | `TwoWriterRecoveryRetained` | HOLD | #1732: post-merge recovery equality |
| `predict_skip` | `SkipClassificationNoFalseSuccess` | HOLD | CONTRACT after #1746 C6 |
| `predict_skip` | `BrokenSignalDominatesMovement` | HOLD | CONTRACT |
| `predict_skip` | `NextConflictIsSuccessOnObservedMovement` | HOLD | CONTRACT |
| `predict_skip` | `FinishedRequiresCleanRepository` | HOLD | CONTRACT |
| `predict_skip` | `NonzeroUnchangedIsNoProgress` | HOLD | CONTRACT |
| `predict_intent` | `IntentAdmissionConjunctive` | HOLD | CONTRACT |
| `predict_intent` | `PlanApprovalConjunctive` | HOLD | CONTRACT |
| `predict_intent` | `BeginWriteConjunctive` | HOLD | CONTRACT |

C2 replaces the mis-posed `AssembleNoMarkerLeak`: raw literal/manual marker text
is intentionally preserved; blocking is a provenance decision. Manual markers
still block, and the `plain-text-gate` mutant rejects chosen source literals.
Parser attribution is protected by collision-free marker widths across all three
source blobs and an exact-width, state-specific parser (#1751). Rust backend/UI
regressions cover source marker literals, width 10, clean merged changes outside
hunks, and inspection-only legacy drafts after the width fingerprint changes.
Do not compare merged passthrough with either stage: clean changes from the
other side belong in the Result by design.
The C1/C3/C4 removed-guard mutants are `empty-adoption-lost`,
`invalid-materializes`, and `malformed-index`; each must remain red.
`undecide-empty-adoption` supersedes the now-redundant `undecide-on-select`:
explicit adoption means selecting a zero-line side no longer looks empty even
if the deselect-only call guard is removed. The new mutant clears an adopted
deletion when deselecting the other side, and must be detected.


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
