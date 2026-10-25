# Conflict lifecycle counterexample ledger

## Runtime reconciliation — 2026-10-24, main `834a1796`

These are historical, hand-extracted models, not Rust execution or an automatic
model/code equivalence proof. The original evidence below is preserved; this
header supersedes its earlier “not runtime tested” qualification only as follows.
Drill sources: `/tmp/kagi-conflict-formal-20261024/drills/DrillLeaveResume.md`,
`DrillSequencers.md`, and `DrillPreexistingComplete.md` (screenshots and Git/draft
witnesses are named there; those local artifacts are not committed).

- **Reproduced:** `draft-io-quit` (silent failed autosave loses the latest edit on
  graceful quit); `preserve-abort` (Abort proceeds after recovery I/O failure and
  loses the latest partial resolution); `stale-worktree` (Save overwrites an
  external WT-only edit). Two instances also lost independent decisions on the
  **same file / different hunks**; this supports the draft race mechanism, not
  the model's distinct-path trace or torn-JSON claim.
- **Partially reconciled:** `approved-revision` / `merge-tree` renew the initial
  Continue tree, but the staged list refreshed before final Commit. The drill
  did **not** commit an unseen tree; the meaning of frozen approval is separate.
- **Runtime hypotheses refuted in the tested scenarios:** C5 unreadable
  `MERGE_MSG` fails closed through the full operation-metadata guard (unchanged
  HEAD/index/status; no commit panel). Skip TOCTOU's naive replacement drill is
  stopped by refresh resetting the armed control. C6's external-final-skip
  drill removes Conflict Mode, and real Git returns 128, not zero/no movement.
  C6 was an isolated-predicate REFUTATION at that snapshot; #1746 D2 now fixes it.
  Receipt-after-kill runs at **479, 568, 732, and 1037 ms measured elapsed** all
  retained exactly one successful Continue receipt; the narrower pre-479 ms
  mutation/accounting gap was not exercised or disproved.
- **Controls held:** completed manual-input autosave survives restart; external
  index replacement removes obsolete Save; ordinary Abort and sequencer replay
  preserve the expected bytes/identity. Non-UTF-8 stash restoration, torn JSON,
  distinct-path lost updates, post-preflight staging races, and sibling-cache
  invalidation were not established by these drills.
- **New runtime findings outside these models:** both-deleted and preexisting
  D/F namespace workflows, comment-character message pollution, and misleading
  Continue/Abort/stash review text. Do not relabel them as formal counterexamples.

Run the maintained guard from the repository root with
`uv run --project formal/conflict formal/conflict/run.sh`; see
[scope and expected outcomes](../../formal/conflict/README.md). Known model
REFUTATIONS remain harness-green while tracked by #1732–#1733 and #1746 (model-external runtime findings: #1736–#1738). The deletion/namespace amendment below adds explicit coverage for #1734/#1735 without relabeling historical observations; the P2 amendment supersedes its corresponding original holes.
The maintained lifecycle/pure expected-outcome tables linked above map each
REFUTE to its issue, owner QUESTION, negative control, or runtime CONTRACT.
The historical traces below retain their original extraction classifications;
the maintained ledger table incorporates the P2 and draft-preservation amendments.
Update a model
and its expectation to HOLD in the same PR as the corresponding product fix;
never weaken an assertion just to restore green.

## Integration of P2 with #1731/#1784

The maintained model combines worker-owned receipts, frozen merge/staging
payloads, planned-ref invalidation and byte-faithful stash restoration with
failed-draft close holds, Abort/Skip/D/F preservation refusal, and current-generation
off-thread flush barriers. No assertion or negative-control expectation is weakened.
The integrated tree with main `cd733a50` passed all **81 configured TLC
verdicts / 89,533 summed distinct states**. All **35** removed-guard controls
remained red against their HOLD assertions (**10,755 states**). The pure suite
passed **39 properties / 1,695,651 evaluations / 36 mutants**; runtime replay
passed **34 trials / 46 logs** and rejected forged Skip. The predecessor
measurements below remain historical, not measurements of this integration.

## Historical combined-tree validation — #1732 and P2

The integrated tree combining `a3c359de` with main `af06cd7a` passed all
**69 configured TLC verdicts / 89,229 summed distinct states**. All **27**
negative controls remained red against their HOLD assertions (10,578 states).
Both the atomic-draft and P2 HOLD sets remain HOLD without weakened assertions.
The pure suite passed **39 properties / 1,695,651 evaluations / 36 mutants**;
runtime replay passed **34 trials / 46 logs** and rejected forged Skip.

The combined native filter passed 79 of 83 scenarios, including repaired
draft-restore admission followed by a recorded Save of frozen, clean-filtered
CRLF content with an index-preserved executable mode. The remaining four are
the separately confirmed main dev-red scenarios tracked by #1776; they are not
claimed as passing integration evidence. Workspace tests and the remaining
build, lint and repository gates passed.

## P2 maintained-model cutover — #1740, #1743, #1744, #1745

The P2-only harness run passed **64 configured verdicts / 147,458 distinct
states**, detected **23 TLC negative controls** (16,274 states), and passed
38 pure properties / 1,695,579 evaluations / 35 mutants. Those are historical
pre-integration counts; the historical atomic-draft/P2 evidence above superseded them.
TLC's invariant exit 12 and temporal-property exit 13 remain distinct.

- **#1740:** `Verify → AppendReceipt → ReleaseLease → Deliver` now applies to
  Continue, Skip, and merge handoff, matching the backend-owned execute/observe/
  finalize boundary. `successful` still means verified mutation, not receipt
  completion. Close during dispatch sets `closing` and detaches the UI; `life`
  represents the worker lifetime until release. The worker can therefore append
  without resurrecting a weak UI owner. `ReceiptFollowsMutation` (three configs)
  and `ContinueEventuallyRecordedOrReported` expect HOLD. Their four
  `workerReceipt` controls restore release-before-callback accounting (and the
  merge handoff's missing append). Receipt I/O failure, process kill and fsync
  durability are outside this fault-free ordering guarantee.
- **#1743:** the merge approval's original tree/revision survive dispatch;
  `MergeRefused` terminates drift without mutation. Staging uses approved frozen
  payloads rather than rereading mutable WT, including cross-instance schedules.
  All three `ApprovedRevisionFrozen` configs and `MergeCommitTreeIsApprovedTree`
  expect HOLD. `mergePlan` controls restore silent renewal; `stagePayload`
  controls restore the original live-WT staging races. The CLI final-check gap
  and per-instance lease questions remain separate, unchanged properties.
- **#1744:** `Deliver` classifies Abort from its planned ref restore, rather than
  inferring shared-ref scope from a successful result. It invalidates the passive
  sibling even when UI completion is detached. `SiblingWorktreesInvalidatedOnSharedRef`
  expects HOLD; `sharedRef` removes that settlement classification.
- **#1745:** stash `WTWrite` checks out the raw-byte path; `Verify` checks both
  restored WT and index. `StashAbortRestoresConflictedBytes` is strengthened to
  require the expected index as well as the original WT. `stash-non-utf8` and
  `stash-verify` expect HOLD. `stashRestore` restores the original combined
  UTF-8 omission/verification hole; `stashVerify` independently removes
  verification under a WT-or-index drift schedule.
  On this macOS APFS filesystem, invalid-UTF-8 creation returned EILSEQ and the
  Unix integration explicitly skipped that unsupported path. The filesystem-
  independent raw-path builder and representable Unicode restoration/corruption
  oracles passed locally; those checks support this HOLD cutover without claiming
  raw-filename filesystem execution. The Ubuntu CI workspace test job exercises
  the Unix integration on a filesystem that accepts the name; its remote result
  remains separate evidence.

### Historical #1731/#1733 merge validation

The predecessor full harness passed **55/55** expected lifecycle
verdicts (85,344 distinct states summed), all **15** removed-guard controls produced
their named counterexamples, all **38** pure properties / **30** pure mutants
matched, and all **34** historical drill projections remained admitted with the
forged-Skip projection rejected. Generated receipts under
`formal/conflict/tla/results/` were generated for that historical run; detailed
counterexample sections below remain historical, not claims that repaired paths
still fail.

### #1732 atomic-draft amendment — 2026-10-24

Implementation `f5931086` replaces truncating writes with the exact ADR-0191
temp/write/sync/rename algorithm, a checked `.prev` fallback, and a bounded
kernel-released advisory lock plus content-hash re-read/merge. The model keeps
the original invariants: `draft-torn`, `draft-two-writers` and `draft-lost-update`
HOLD; their broken variants still REFUTE. `draft-crash` remains REFUTE: the
backend's ordered writer captures memory synchronously but coalesces pending
snapshots for up to 250 ms before starting, plus any in-flight fsync delay.
Only `DraftSave::wait()` / `flush()` returning `Ok` acknowledges the durable
primary rename. The model retains the unacknowledged crash transition and
does not weaken `DraftNeverLost`. The preceding acknowledged document remains
readable; power-loss/directory-fsync durability is not promised. GUI consumers
must await barriers off-thread and retain #1731's pending/failed refusal gates.

The earlier queued-boundary run passed **52 expected outcomes / 33,865 summed
distinct states**. Its claimed 13 controls included a redundant crash control
against an already-refuted baseline; that control and its count are removed.
`draft-crash` still exposes capture → crash → restart without weakening it.

After merging #1733 and removing that control, the earlier run passed
**54 expected outcomes / 52,223 summed distinct states**, with all **14**
remaining negative controls detected against baseline-HOLD properties.
Integrating #1746 D1/D2 and #1757's width-aware materialization preserved those
draft guards and added the frozen Skip revision/prewrite defence: that
pre-integration run passed **55 expected outcomes / 52,280 summed distinct states**,
with **15** meaningful negative controls detected. Its pure lane retained 38
properties (1,695,579 evaluations) and detected 35 mutants.

### #1732 two-writer recovery-retention amendment — 2026-10-25

The historical draft-only recovery-retention run passed **57 expected outcomes / 52,289
summed distinct states**, detected all **16** meaningful TLC negative controls,
and passed **39 pure properties / 1,695,651 evaluations / 36 detected mutants**.
The new recovery check explores **5** states; its broken counterpart refutes
in **4** states (exit 12): Init → A primary → B primary + A recovery →
A other-file save adopts B but deletes A recovery.

`two-writer-recovery` adds a successful-transaction refinement with explicit
primary and recovery records. A and B load the same empty document, save different
file-1 Results in order, and A then saves file 2 twice with its unchanged local
file-1 body. `resolution/draft_merge.rs::matches_record` compares each recovery
against exactly the disk-selected or body-overridden primary that `record` writes.
The unchanged `DraftWriteAtomic` assertion includes acknowledged recovery bodies,
not merely current primaries. The expected HOLD has a matching
`preMergeRecovery` negative control that drops A at the first other-file save.
The bounded refinement treats each successful advisory-locked transaction as one
transition; it does not replace the separate torn-write/lock/crash interleavings.
Backend subprocess tests cover both commit orders, blocking and queued writers,
exact owned-inspector bytes/identity, and repeated `.prev` rotations.


The reproducible 100-call, 3-file/5-hunk, 209,475-byte backend benchmark measured
the repeat synchronous path at p50 **69.765 ms** / p95 **200.917 ms**, still
above the owner's 4 ms UI budget (initial p95 **477.702 ms**). Ordered queue
capture measured p50 **0.094 ms** / p95 **0.110 ms**, maximum **0.937 ms**;
its final flush completed successfully. These measure the backend API, not
GUI consumer integration.

The earlier synchronous implementation passed **52 expected outcomes / 33,868
summed distinct states**; its then-13 controls included the now-removed crash variant. Its native
`conflict_draft_revision_same_revision` test observed disk immediately after
Change; the queued backend instead acknowledges only durable completion.
Backend regressions exercise two real writer processes (distinct paths and same-file independent
hunks), both merge orders, corrupt/half-temp recovery, bounded lock timeout,
and SIGKILL releasing a child's held lock. Native DrillLeaveResume §2a replay
with two plain-binary instances and one shared isolated `KAGI_LOG_DIR` failed
on `834a1796` (first hunk unresolved after restart) and passed on `f5931086`
(both decisions live, 2/3 resolved). Local screenshots, document witnesses and
oracles: `/tmp/kagi-1732-evidence/{before,after}/`. HEAD/status/operation witnesses
were unchanged. The fixed run's raw index hash changed only in `.gitmodules`
ctime/inode fields; parsed staged paths/OIDs/modes/flags and extensions matched
the original fixture exactly (`after/index-stat-reconciliation.json`).

### #1760 UI barrier cutover — 2026-10-25

The UI now captures ordinary edits/selections with the existing ordered writer.
`conflict_view/draft_owner.rs::flush_drafts` performs the blocking barrier on
the background executor and rejects a changed generation or pane revision;
`operations/conflict.rs::flush_conflict_drafts` also rechecks the attachment and
entity before approval. `editor_workspace/host_close.rs::save_host_dirty_guard`
retains the modal continuation until flush completes; its unsaved gate reads
`has_unflushed_drafts()`, not only an already-presented failure flag.

The eight `draft-flush-*` checks add a two-generation ordering refinement:
failure cannot authorize progress, an older completion cannot approve a newer
edit, close cannot precede durability, and the UI flush's draft transaction lock
must be released before repository admission. Each HOLD has an independently
removed-guard REFUTATION. This does not model preservation inside an already
admitted backend operation. It does not change `DraftNeverLost`: `draft-crash` remains REFUTE
through the up-to-250 ms coalescing interval and any in-flight I/O. Explicit
two-step Discard remains outside the no-intentional-loss scenarios.

The historical incoming-main cutover run matched **69/69 expected outcomes**, exploring
**52,578 summed distinct states**, and detected all **24** removed-guard controls.
The pure lane passed **39 properties / 1,695,651 evaluations / 36 mutants**;
all **34** historical projections remained admitted and forged Skip was rejected.
The same-machine 100-call benchmark measured synchronous persistence at
p50 **65.987 ms** / p95 **69.350 ms**, versus queued capture at
p50 **0.096 ms** / p95 **0.122 ms** (maximum **1.087 ms**). These are backend
API measurements, not end-to-end GUI frame timings.

### #1734 / #1735 deletion and parked-namespace amendment — 2026-10-25

The baseline had no `dd-rename` or `df-head-parked` model rows. Both were runtime
findings outside the initial extraction. The maintained lifecycle model now
contains these named HOLD obligations and `modify-delete`, plus five deliberately
broken variants; this is an added extraction, not a retrospective claim that the
2026-10-24 drill failures were TLC counterexamples.

The domain's `conflict_family::Resolution::Delete` and `ConflictDraft::Delete`
represent absence, not an empty file. Captured side presence determines the
available actions. The existing recorded conflict boundary freezes the exact
delete/namespace operation; the operation's plan/preflight/execute/verify owns
all filesystem and index changes. `ops/dir_file_conflict.rs` attributes a parked
file to its original path with source-tree and index identity, then resolves
that whole namespace rather than saving the parked filename.

`NamespaceNext` in `ConflictLifecycle.tla` extracts classification → exact plan →
confirmation → preflight → recovery → index/worktree writes → verification
→ receipt. It fixes index-before-worktree as an abstract order; production deletion
removes the worktree artifact before the index entry, while D/F writes the index
first. No model action interleaves those writes, so this is not a partial-write
ordering proof. It explores six paths (source, two rename destinations, parked file,
child and an unrelated lookalike), both modify/delete orientations, DD with and
without an artifact, both D/F choices, proven/unproven namespace ownership,
cancel, one preflight-time external change and recovery success/failure.
`DeletionRouteCompletes` checks actual route availability after classification;
`ParkedNamespaceCompletes` rejects unproven suffix mapping. Both require the
complete approved index/worktree maps after mutation and a receipt at completion.
They do not claim eventual execution, OS atomicity or exclusion of external Git
after preflight. Unrelated lifecycle and pure-property assumptions are unchanged.

The five red controls independently restore absent-side-only DD choices,
deletion-as-empty-blob, literal parked-path staging, suffix-only attribution and
leftover parked worktree content. Before P2 integration this amendment's matrix
had **77 TLC checks / 29 negative controls**; the combined matrix now has **89 /
40**. Generated full-suite receipts remain the source for observed verdicts.

Observed on `fix/delete-routes-1734` before the P2 integration:
the complete `formal/conflict/run.sh` exited 0 in **57.369 seconds**. All **77**
declared verdicts matched across **52,915 distinct states**; all **29** forced-HOLD
negative controls refuted (**1,371 states**). Specifically, `dd-rename`,
`modify-delete`, and `df-head-parked` held and their five broken variants stayed
red. Pure checks remained **39 properties / 1,695,651 evaluations / 36 mutants**.
The unchanged historical projection also passed **34 trials / 46 logs**, retaining
its explicit log gaps and forged-Skip refusal. These historical logs are not new
GUI evidence for this amendment.


---

## Reproduction and evidence boundary

- Source: `/tmp/kagi-conflict-hunk-20261023` (assignment identifies HEAD `6f9696df`), read-only; no Cargo, product edits, GUI runs, Git mutations or GitHub posts.
- Method: playbook §§0–6, every lifecycle-map section, and the implementation sections cited below were read before modelling. This is **TLC evidence about a hand-extracted finite model**, not an executed Rust/GUI reproduction. BUG means a model trace contradicts a cited declared product contract; reconciliation with an actual fixture remains necessary.
- Re-run from the repository root: `uv run --project formal/conflict formal/conflict/run.sh`. `formal/conflict/tla/run.py` contains the self-check table; the committed `.cfg` files are read without rewriting. Select a debugging subset with `--suite tla CHECK` without overwriting the full-suite summary.
- Dependencies: CPython >=3.11 via `formal/conflict/pyproject.toml` and `uv.lock`; Java 21 on PATH (CI: Temurin 21); pinned TLC release `v1.8.0`, downloaded to cache and verified against `formal/conflict/tla/TLA2TOOLS.sha256` (SHA-256 `d15655eeaff4b3f7b3d65c46b838bc0f35d131b2ef4b1e473689b62c98d4726f`). Historical observed runtime: Adoptium 21.0.13. No third-party Python dependency is used.
- Machine exit: 0 only when every named invariant/property has its expected verdict **and** TLC has the appropriate zero/nonzero exit; 1 unexpected verdict; 2 launch/timeout/unknown-check errors. Expected refutations are intentionally green at the harness level, red at TLC level.
- Initial historical run before trace refinement: **44 / 44 expected outcomes, 22.441 seconds, 59,260 distinct states summed across independent runs**. The maintained union has **89** checks and **40** negative controls, retaining P2, #1731 preservation, #1732 atomic drafts, #1746 D1/D2, #1760/#1784 flush barriers and #1734/#1735 deletion/namespace guards. See the [trace appendix](conflict-trace-ledger.md). Counts are TLC distinct states found, not globally unique or exhaustive after a first safety counterexample. Current combined machine-readable evidence is generated under `formal/conflict/tla/results/`; the 77- and 81-check runs above remain predecessor evidence.

## Declared vs implicit

**Declared:** repository writes must follow plan → confirm → preflight → execute → verify → oplog (`AGENTS.md:23-37`); Save/Continue/Commit retain frozen revision/owner/lease checks and Save stages the explicitly reviewed file (`docs/adr/0227-progressive-conflict-resolution.md:126-128`); Abort preserves partial resolutions (`crates/kagi-git/src/conflicts.rs:27-33`, `ops/conflict_abort.rs:274-280`); D/F removal is recoverable because a snapshot was taken (`ops/dir_file_conflict.rs:299-300`); shared-ref changes invalidate sibling worktrees (`src/app/settle.rs:272-300`).

**Historical implicit behavior (superseded where fixed above):** WT bytes were not fingerprinted; Save staged by rereading WT; draft JSON used unlocked truncate/write; persistence errors were logged/ignored; Skip consumed a newly observed live step; Continue/Skip released before UI recording; merge handoff success had no append; merge finalization renewed its plan; Conflict was classified as not shared-ref-changing; stash abort filtered paths through UTF-8 and verified only operation/HEAD. These describe the original extraction, not the maintained P2 transitions.

**Map disagreement, material:** map §5's missing manual-capture persistence call was obsolete in the inspected synchronous implementation. The ordered backend now separates immediate capture/queue acceptance from successful durable writer completion; `manual-capture.cfg` holds only at the latter boundary, and deleting the commit path goes red. `draft-crash` deliberately remains REFUTE for abrupt death between capture and commit. The UI must await `flush` off-thread at lifecycle/operation boundaries and preserve errors rather than interpreting queue acceptance as saved. Historical map line offsets drifted; the original trace citations remain extraction evidence.

## Finite scope and honest model/code gaps

- Two independent application instances share one repository and one draft file; leases remain per-instance. Concurrent configs explore **all enabled interleavings** of both instances. Single-route configs leave the second actor idle to isolate minimal witnesses. A passive sibling worktree view is represented separately by its invalidation bit.
- Ordinary lifecycle scenarios have at most two paths; the #1734/#1735 refinement above has six. One abstract hunk per edited text path; text/blob/namespace payloads are finite enums. Full revision is a tuple of bounded index/HEAD/metadata/operation revision counters. One operation per actor, one external CLI edit/replacement, at most one restart. No hash collisions, path aliases, or SHA internals are modelled.
- Scenario selectors are **environment/scope restrictions**, not product guards. Index/HEAD/MERGE_MSG external edits are separate runs before final preflight; WT-only edits are before preflight or between WT write and add_path; index CLI interference and two-app interference also occur after final preflight. Observation is explicitly split into index/metadata/accept, but these configs do not claim exhaustive mid-observation metadata tearing coverage.
- Read failures/trust/path/marker/identity validators unrelated to the witness are fixed to successful, known, compatible inputs. A failed preflight is a terminal nonmutating path (deadlock checking disabled); rejection UI text is outside this quotient. Continue has one resolved text path and no unexpected unmerged path. D/F has a namespace decision, not a fictitious text hunk. MergeCommit starts at an already resolved commit panel.
- Immediate capture and queued completion are distinct transitions. Pending snapshots coalesce for at most 250 ms before starting, and in-flight fsync may extend the unacknowledged window; abrupt exit can lose that newest captured edit, so `DraftNeverLost` remains REFUTE without changing its assertion. Successful `flush` is the durable boundary. Restart restores same-revision compatible records; a corrupt primary uses the last good `.prev`. Without a valid backup, restore preserves positively damaged documents under exclusively reserved `.corrupt[.N]` names, records and announces the failure, then loads the editable base; ordinary writers still refuse corrupt input. This upgrade recovery is covered by backend/native tests, not an additional TLA transition. The two model writers edit distinct paths; backend/native regressions also cover independent same-file hunk decisions.
- Draft reads acquire the bounded advisory lock and re-read/merge the latest document; the model quotients successful lock acquisition, while backend tests cover timeout and death release. The historical `draft-lost-update` trace remains reproducible by its broken-lock variant. #1731 makes Abort/Skip/D/F preservation failure refuse before mutation; its `preservation` controls restore the historical continuation after failure. The deletion/namespace refinement retains the same recovery prerequisite.
- IndexWrite now stages the frozen payload; `stagePayload` restores current-WT `add_path` behavior. The index/WT write boundaries remain separate and no stronger exclusion of arbitrary CLI interference is claimed. Raw resolutions, multi-file Continue rollback, cached unrelated-index clobbering, signature/hooks, and detailed CLI retry outcomes remain outside these lifecycle traces; pure-state checks are separate work.
- Receipt HOLD assumes append succeeds and no abrupt process death. It is an ordering guarantee, not fsync/power-loss durability. Continue liveness retains weak fairness for each actor. UI destruction during a dispatched worker does not destroy that worker; release ends its retained lifetime after append. The negative control restores the closed-owner callback loss.
- Safety counterexamples below are BFS-shortest in their configured finite slice. TLC's liveness lasso has no unnecessary loop before its terminal stutter; it is not claimed to minimize real UI gestures. Every event is mapped to source lines below; source-reviewed executable consequences remain [INFERENCE] until a product fixture confirms them.

## Ledger table

Expectations describe the maintained union. Results and state counts are historical predecessor evidence, not a combined-run claim: P2 rows retain the atomic-draft/P2 measurements, while failed-close, preservation and barrier rows retain incoming main's measurements. Counts separated by `/` follow the check/trace order. Parent validation supplies current combined evidence. CONTRACT rows are bounded holds, not global proofs. BUG/QUESTION classifications apply to the corresponding implementation interpretation, not artificial broken variants.

| Invariant | Expectation | Result | States explored | Trace (if refuted) | Code lines | Classification | Owner question / fix sketch |
|---|---|---|---:|---|---|---|---|
| `NoWriteBeforeApproval` | HOLD | HELD | 32779 total (8 runs) | — | `app/flow.rs:261-347; app/session.rs:516-558` | CONTRACT | Retain confirmation/admission guards for all seven writer routes and both instances. |
| `ApprovedRevisionFrozen (merge + staging races)` | HOLD | HELD | 40 / 79 / 32205 | Historical traces retained below | Frozen merge plan; frozen staging payload | CONTRACT (#1743) | `mergePlan` and `stagePayload` controls retain all three original flaws. |
| `ApprovedSavePayloadFrozen (WT payload)` | HOLD | HELD | 71 | — | `backend/conflict_ops.rs:195-218,281-288; resolution/conflict_boundary.rs:103-107,170-195` | CONTRACT | Keep immutable payload binding at WT materialization and index staging. |
| `StaleSnapshotNeverWrites (WT edit before per-file validation)` | HOLD | HELD | 89 | — (historical [stale-worktree](#trace-stale-worktree)) | `resolution/conflict_boundary.rs::validate_save_binding, validate_current_paths; resolution/worktree_state.rs::validate` | CONTRACT (#1733) | Freeze the live draft's WT-byte fingerprint and refuse changed bytes before mutation; broken WT guard remains a negative control. |
| `StaleSnapshotNeverWrites (WT edit + watcher before first choice)` | HOLD | HELD | 143 | — | `resolution/drafts.rs::retain_drafts; resolution/worktree_state.rs::retain_provenance` | CONTRACT (#1733 review) | Untouched live files retain the original fingerprint too. `stale-worktree-before-choice-broken` removes only that carry and must REFUTE. Exact frozen-text rewrites remain idempotently retryable after Partial execution. |
| `StaleSnapshotNeverWrites (final-check gap / second instance)` | REFUTE | REFUTED | 39 / 8874 | [stale-after-preflight](#trace-stale-after-preflight), [two-instance-stale-write](#trace-two-instance-stale-write) | `conflicts.rs::execute_conflict_save_with_progress; resolution/worktree_state.rs::validate` | QUESTION (CLI TOCTOU); #1746 D3 (instances) | Keep validation and write as distinct model transitions. A last-read fingerprint does not exclude arbitrary CLI writes or serialize independent instances. |
| `StaleSnapshotNeverWrites (index/HEAD/MERGE_MSG drift before preflight)` | HOLD | HELD | 77 / 77 / 77 | — | `backend/conflict_ops.rs:419-425; resolution/conflict_boundary.rs:170-175; conflicts.rs:1261-1297` | CONTRACT | Retain full-revision refusal; it protects preflight-time drift, not the later write window. |
| `DraftNeverLost (captured but unacknowledged, same revision)` | REFUTE | REFUTED | 11 | [draft-crash](#trace-draft-crash) | `resolution/draft_writer.rs::{queue_autosave,flush}; resolution/draft_storage.rs::write_atomic` | EXPLICIT CRASH WINDOW (#1732) | Capture/queue acceptance is not durable; up to 250 ms coalescing plus in-flight I/O may be lost. Only successful flush acknowledges disk. |
| `DraftWriteAtomic (torn / two writers / lost update)` | HOLD | HELD | 25 / 253 / 253 | — (historical traces below retained; broken controls refute) | `resolution/draft_storage.rs::{read,lock,replace,save}; resolution/draft_merge.rs::merge_file; resolution/draft_storage_tests.rs` | CONTRACT (#1732) | Atomic replacement, last-good backup, and interprocess locked read/CAS/merge retain acknowledged decisions. |
| `DraftNeverLost (failed autosave + graceful close; no explicit discard)` | HOLD | HELD (TLC exit 0) | 21 | [historical draft-io-quit](#trace-draft-io-quit) | `ui/conflict_view/draft_owner.rs:note_draft_result; ui/editor_workspace/host_close.rs:hold_host_close,save_host_dirty_guard` | #1731 fixed | Keep the live buffer and sheet on failure; only successful Retry clears the hold. Native EN/JA tests additionally exercise the separately approved two-step intentional discard, outside this unchanged no-loss invariant. |
| `DraftWriteAtomic (two-writer-recovery)` | HOLD | HELD | 5 | —; `two-writer-recovery-broken` retains the lost-body witness | `resolution/drafts.rs::autosave_json; resolution/draft_merge.rs::{record,matches_record}; resolution/draft_storage_tests.rs::two_writer_recovery_survives_other_file_autosaves` | CONTRACT (#1732 P1) | Compare recovered bodies with the serialized post-merge primary, never the stale in-memory editor record. |
| `ManualCapturePersisted (fault-free durable writer completion)` | HOLD | HELD | 12 | — | `resolution/draft_writer.rs::{DraftSave::wait,flush}; resolution/draft_storage.rs::write_atomic` | CONTRACT; map P1 DISPROVED | The writer's successful completion, not queue acceptance, acknowledges the captured body. |
| `BarrierFailureNeverApproved` | HOLD | HELD (TLC exit 0) | 28 | — | `ui/conflict_view/draft_owner.rs::flush_drafts; ui/operations/conflict.rs::flush_conflict_drafts` | CONTRACT (#1760) | A failed off-thread flush retains the live pane and cannot authorize an operation. |
| `BarrierApprovalCurrentGeneration` | HOLD | HELD (TLC exit 0) | 28 | — | `resolution/draft_writer.rs::DraftSave::is_current; ui/conflict_view/draft_owner.rs::flush_drafts` | CONTRACT (#1760) | Recheck the current ticket and pane revision after completion; superseded success is not storage failure or approval. |
| `BarrierCloseDurable` | HOLD | HELD (TLC exit 0) | 28 | — | `ui/editor_workspace/host_close.rs::save_host_dirty_guard_inner; ui/tabs.rs::close_tab` | CONTRACT (#1760) | Without explicit two-step Discard, close follows successful current-generation durability. |
| `BarrierLocksNeverNested` | HOLD | HELD (TLC exit 0) | 28 | — | `ui/operations/conflict.rs::flush_conflict_drafts; resolution/draft_writer.rs::DraftSave::flush` | CONTRACT (#1760 UI boundary) | Complete and release the UI flush's draft transaction before repository admission; backend preservation inside an admitted operation is outside this refinement. |
| `ReceiptFollowsMutation (Continue / merge handoff / Skip)` | HOLD | HELD | 123 / 123 / 103 | Historical traces retained below | Backend execute/observe/finalize before worker settlement | CONTRACT (#1740) | Three `workerReceipt` controls restore release-before-append or absent merge accounting. |
| `ReceiptFollowsMutation (Save, append succeeds)` | HOLD | HELD | 71 | — | `backend/conflict_ops.rs:581-603; app/settle.rs:147-149` | CONTRACT | Keep backend-owned receipt before completion; no crash/fsync or append-failure guarantee is inferred. |
| `SkipDropsOnlyApprovedStep` | HOLD | HELD after #1746 D2 | 203 / 141 | historical [skip-replacement](#trace-skip-replacement) | `conflicts.rs::preflight_skip_revision`, `execute_conflict_skip`; `conflict_view/actions.rs::skip_request_arm` | CONTRACT | Frozen full revision and operation survive plan/confirm/dispatch; both initial and immediately-pre-CLI checks refuse a replacement. Rust `skip_refuses_replaced_approved_step_and_preserves_the_next_conflict` proves step 2 survives. |
| `NoDestructiveProgressAfterPreservationFailure (Abort / Skip / D-F)` | HOLD | HELD (TLC exit 0) | 41 / 41 / 20 | [historical preserve-abort](#trace-preserve-abort), [historical preserve-skip](#trace-preserve-skip), [historical preserve-dir-file](#trace-preserve-dir-file) | `ops/conflict_abort.rs:execute_conflict_abort_expecting; conflicts.rs:execute_conflict_skip; ops/dir_file_conflict.rs:apply_dir_file_resolution_with_progress` | #1731 fixed | Required recovery failure returns Refused before repository mutation. Recovery file atomicity is guarded by #1732. |
| `MergeCommitTreeIsApprovedTree` | HOLD | HELD | 40 | Historical [merge-tree](#trace-merge-tree) | Approved merge plan; `ops/commit.rs::approved_commit_index` | CONTRACT (#1743) | Preserve approved index snapshot; `mergePlan` restores replanning. |
| `SiblingWorktreesInvalidatedOnSharedRef` | HOLD | HELD | 79 | Historical [sibling-invalidation](#trace-sibling-invalidation) | Planned Abort ref restoration → shared-ref settlement | CONTRACT (#1744) | `sharedRef` removes planned invalidation. |
| `ContinueEventuallyRecordedOrReported` | HOLD | HELD | 123 | Historical [continue-liveness](#trace-continue-liveness) | Backend accounting survives UI destruction | CONTRACT (#1740) | `workerReceipt` restores weak-owner callback loss. |
| `RepoHasAtMostOneLease (two independent apps)` | REFUTE | REFUTED | 2124 | [two-instance-leases](#trace-two-instance-leases) | `app/session.rs:261-278,544-558; ui/mod.rs:1601` | QUESTION | Is repository-global admission promised, or is independent-app concurrency intentionally unsupported? |
| `StashAbortRestoresConflictedBytes (non-UTF-8; post-checkout drift)` | HOLD | HELD | 75 / 83 | Historical [stash-non-utf8](#trace-stash-non-utf8) | Raw checkout paths; restored WT/index verification | CONTRACT (#1745) | `stashRestore` restores original omission; `stashVerify` removes independent postconditions. |

## Load-bearing negative controls

These cfg files set `Broken` in the same source model; they are deliberate model patches, not product fixes. The ordinary cfg values remain `"none"`. No expected product refutation is reclassified as a negative-control bug.

The following measurements are from the integrated P2/#1731/#1784 removed-guard
run: all 35 controls refuted their unchanged HOLD assertions. Grouped state
counts follow the listed cfg order; temporal violations retain TLC exit 13.

| HELD invariant | Broken cfg | Deliberately removed guard | TLC result | States |
|---|---|---|---|---:|
| `NoWriteBeforeApproval (all route configs)` | [`approval-broken.cfg`](../../formal/conflict/tla/approval-broken.cfg) | Bypass the explicit approval gate while keeping preflight | REFUTE (exit 12) | 32 |
| `ApprovedSavePayloadFrozen` | [`save-payload-broken.cfg`](../../formal/conflict/tla/save-payload-broken.cfg) | Remove payload == frozen binding guard at WT materialization | REFUTE (exit 12) | 33 |
| `StaleSnapshotNeverWrites (index)` | [`stale-index-broken.cfg`](../../formal/conflict/tla/stale-index-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 51 |
| `StaleSnapshotNeverWrites (HEAD)` | [`stale-head-broken.cfg`](../../formal/conflict/tla/stale-head-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 51 |
| `StaleSnapshotNeverWrites (MERGE_MSG)` | [`stale-metadata-broken.cfg`](../../formal/conflict/tla/stale-metadata-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 51 |
| `StaleSnapshotNeverWrites (WT and pre-choice WT)` | `stale-worktree-broken`, `stale-worktree-before-choice-broken` | Remove WT provenance validation, including untouched live files | REFUTE (exit 12) | 51 / 122 |
| `ManualCapturePersisted` | [`manual-capture-broken.cfg`](../../formal/conflict/tla/manual-capture-broken.cfg) | Remove persistence on changed captured input | REFUTE (exit 12) | 6 |
| `DraftWriteAtomic (torn)` | [`draft-torn-broken.cfg`](../../formal/conflict/tla/draft-torn-broken.cfg) | Restore truncation of the primary before write completion | REFUTE (exit 12) | 20 |
| `DraftWriteAtomic (two writers)` | [`draft-two-writers-broken.cfg`](../../formal/conflict/tla/draft-two-writers-broken.cfg) | Remove serialization of load/merge/write | REFUTE (exit 12) | 222 |
| `DraftWriteAtomic (lost update)` | [`draft-lost-update-broken.cfg`](../../formal/conflict/tla/draft-lost-update-broken.cfg) | Remove serialization while requiring complete JSON reads | REFUTE (exit 12) | 222 |
| `DraftWriteAtomic (two-writer recovery)` | [`two-writer-recovery-broken.cfg`](../../formal/conflict/tla/two-writer-recovery-broken.cfg) | Compare recovery equality against the editor's pre-merge body | REFUTE (exit 12) | 4 |
| `ReceiptFollowsMutation (Save)` | [`receipt-save-broken.cfg`](../../formal/conflict/tla/receipt-save-broken.cfg) | Remove append-before-release ordering gate | REFUTE (exit 12) | 45 |
| `ReceiptFollowsMutation; ContinueEventuallyRecordedOrReported` | `receipt-continue-broken`, `receipt-merge-broken`, `receipt-skip-broken`, `continue-liveness-broken` | `workerReceipt`: release before UI-owned append; omit merge append | REFUTE (exit 12 / 12 / 12 / 13) | 82 / 82 / 62 / 121 |
| `ApprovedRevisionFrozen; MergeCommitTreeIsApprovedTree` | `approved-revision-broken`, `merge-tree-broken` | `mergePlan`: silently renew approved tree/revision | REFUTE (exit 12) | 25 / 25 |
| `ApprovedRevisionFrozen` | `approved-index-race-broken`, `approved-two-writers-broken` | `stagePayload`: stage mutable WT instead of approved payload | REFUTE (exit 12) | 43 / 8906 |
| `SiblingWorktreesInvalidatedOnSharedRef` | `sibling-invalidation-broken` | `sharedRef`: omit planned sibling invalidation | REFUTE (exit 12) | 62 |
| `StashAbortRestoresConflictedBytes` | `stash-non-utf8-broken`, `stash-verify-broken` | Original UTF-8 checkout/verification omission; independently remove restore verification | REFUTE (exit 12) | 44 / 54 |
| `SkipRefreshDisarmsChangedStep; SkipDropsOnlyApprovedStep` | `skip-refresh-broken`, `skip-prewrite-broken` | Remove the refresh-disarm or final prewrite revision guard | REFUTE (exit 12) | 32 / 89 |
| `MetadataReadErrorNeverAdmitted` | `continue-metadata-read-broken` | Ignore failed operation-metadata reads before Continue | REFUTE (exit 12) | 41 |
| `DraftNeverLost (failed autosave + close)` | [`draft-io-quit-broken.cfg`](../../formal/conflict/tla/draft-io-quit-broken.cfg) | Remove only the failed-draft close hold, preserving the lease gate | REFUTE (exit 12) | 15 |
| `NoDestructiveProgressAfterPreservationFailure (Abort)` | [`preserve-abort-broken.cfg`](../../formal/conflict/tla/preserve-abort-broken.cfg) | Ignore failed recovery and continue to destructive Abort | REFUTE (exit 12) | 36 |
| `NoDestructiveProgressAfterPreservationFailure (Skip)` | [`preserve-skip-broken.cfg`](../../formal/conflict/tla/preserve-skip-broken.cfg) | Ignore failed recovery and run Skip | REFUTE (exit 12) | 42 |
| `NoDestructiveProgressAfterPreservationFailure (D/F)` | [`preserve-dir-file-broken.cfg`](../../formal/conflict/tla/preserve-dir-file-broken.cfg) | Ignore failed snapshot and mutate the index/working tree | REFUTE (exit 12) | 16 |
| `BarrierFailureNeverApproved` | [`draft-flush-failure-broken.cfg`](../../formal/conflict/tla/draft-flush-failure-broken.cfg) | Accept an errored flush as approval | REFUTE (exit 12) | 18 |
| `BarrierApprovalCurrentGeneration` | [`draft-flush-generation-broken.cfg`](../../formal/conflict/tla/draft-flush-generation-broken.cfg) | Accept a successful superseded generation | REFUTE (exit 12) | 21 |
| `BarrierCloseDurable` | [`draft-flush-close-broken.cfg`](../../formal/conflict/tla/draft-flush-close-broken.cfg) | Close immediately after requesting the barrier | REFUTE (exit 12) | 8 |
| `BarrierLocksNeverNested` | [`draft-flush-lock-order-broken.cfg`](../../formal/conflict/tla/draft-flush-lock-order-broken.cfg) | Admit the repository lock before the UI draft lock is released | REFUTE (exit 12) | 21 |

The source has several repeated validation reads; the model deliberately consolidates the *last* full-revision/binding guard. The broken revision variant removes that consolidated check, not a claim that deleting only one of several Rust checks would suffice. The payload negative variant admits a non-frozen token at the write boundary to exercise the binding assertion. MergeCommit's modeled lease represents its synchronous busy UI interval, not a claim that its legacy synchronous path allocates a Sessions lease.

## Historical complete minimal event traces

The values below were copied from the original TLC `event` field without reordering. Their verdicts, source offsets, and counts describe that historical extraction, not new executions after the P2 cutover. For #1740/#1743/#1744/#1745, current baselines expect HOLD and the paired broken configs preserve refutability; see the maintained table above. Each event has its enabling source lines; 1/2 denote independent app instances. For artificial negative controls, interpret the removed guard as the changed semantics.

<a id="trace-approval-broken"></a>
### approval-broken

`NoWriteBeforeApproval` — expected REFUTE; TLC REFUTED; 28 distinct / 32 generated; 0.436s; [full trace](../../formal/conflict/tla/results/approval-broken.log); [cfg](../../formal/conflict/tla/approval-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-save-payload-broken"></a>
### save-payload-broken

`ApprovedSavePayloadFrozen` — expected REFUTE; TLC REFUTED; 29 distinct / 34 generated; 0.45s; [full trace](../../formal/conflict/tla/results/save-payload-broken.log); [cfg](../../formal/conflict/tla/save-payload-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-approved-revision"></a>
### approved-revision

`ApprovedRevisionFrozen` — expected REFUTE; TLC REFUTED; 25 distinct / 26 generated; 0.445s; [full trace](../../formal/conflict/tla/results/approved-revision.log); [cfg](../../formal/conflict/tla/approved-revision.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
6. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
7. `<<"ExternalGit", "merge">>` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
8. `<<"RenewMergePlan", 1>>` — `src/ui/operations/commit.rs:1191-1209`; `crates/kagi-git/src/backend.rs:979-998` — rebuild plan from current index, not displayed index.
9. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
10. `<<"MergeCommit", 1>>` — `crates/kagi-git/src/ops/commit.rs:74-83,334-351` — commit checked private index (the newly renewed one).

<a id="trace-approved-index-race"></a>
### approved-index-race

`ApprovedRevisionFrozen` — expected REFUTE; TLC REFUTED; 39 distinct / 45 generated; 0.476s; [full trace](../../formal/conflict/tla/results/approved-index-race.log); [cfg](../../formal/conflict/tla/approved-index-race.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `"ExternalGit(worktree before add_path)"` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
15. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.

<a id="trace-approved-two-writers"></a>
### approved-two-writers

`ApprovedRevisionFrozen` — expected REFUTE; TLC REFUTED; 9754 distinct / 21255 generated; 0.944s; [full trace](../../formal/conflict/tla/results/approved-two-writers.log); [cfg](../../formal/conflict/tla/approved-two-writers.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"ObserveIndex", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
15. `<<"ObserveMetadata", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
16. `<<"AcceptRead", 2>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
17. `<<"OpenFile", 2, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
18. `<<"LineToggle", 2>>` — `src/ui/conflict_view/actions.rs:34-79,129-145` — source selection change then autosave.
19. `<<"AutosaveRead", 2>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
20. `<<"AutosaveWriteStart(truncate)", 2>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
21. `<<"AutosaveWrite", 2>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
22. `<<"Freeze(plan)", 2>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
23. `<<"Approve(confirm)", 2>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
24. `<<"Preflight", 2>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
25. `<<"WTWrite", 2>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
26. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.

<a id="trace-stale-worktree"></a>
### stale-worktree

`StaleSnapshotNeverWrites` — current expectation **HOLD** after #1733's per-file WT provenance check. The steps below retain the historical `834a1796` counterexample (43 distinct / 52 generated). Removing the new WT guard is checked by `stale-worktree-broken`, which must still REFUTE.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `"ExternalGit(worktree)"` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
13. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
14. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-stale-index-broken"></a>
### stale-index-broken

`StaleSnapshotNeverWrites` — expected REFUTE; TLC REFUTED; 43 distinct / 60 generated; 0.438s; [full trace](../../formal/conflict/tla/results/stale-index-broken.log); [cfg](../../formal/conflict/tla/stale-index-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"ExternalGit", "staleIndex">>` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
13. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
14. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-stale-head-broken"></a>
### stale-head-broken

`StaleSnapshotNeverWrites` — expected REFUTE; TLC REFUTED; 43 distinct / 60 generated; 0.474s; [full trace](../../formal/conflict/tla/results/stale-head-broken.log); [cfg](../../formal/conflict/tla/stale-head-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"ExternalGit", "staleHead">>` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
13. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
14. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-stale-metadata-broken"></a>
### stale-metadata-broken

`StaleSnapshotNeverWrites` — expected REFUTE; TLC REFUTED; 43 distinct / 60 generated; 0.443s; [full trace](../../formal/conflict/tla/results/stale-metadata-broken.log); [cfg](../../formal/conflict/tla/stale-metadata-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"ExternalGit", "staleMeta">>` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
13. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
14. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-stale-after-preflight"></a>
### stale-after-preflight

`StaleSnapshotNeverWrites` — expected REFUTE; TLC REFUTED; 35 distinct / 41 generated; 0.448s; [full trace](../../formal/conflict/tla/results/stale-after-preflight.log); [cfg](../../formal/conflict/tla/stale-after-preflight.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `"ExternalGit(index after final preflight)"` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
14. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-manual-capture-broken"></a>
### manual-capture-broken

`ManualCapturePersisted` — expected REFUTE; TLC REFUTED; 6 distinct / 6 generated; 0.408s; [full trace](../../formal/conflict/tla/results/manual-capture-broken.log); [cfg](../../formal/conflict/tla/manual-capture-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.

<a id="trace-draft-crash"></a>
### draft-crash

`DraftNeverLost` — expected REFUTE; TLC REFUTED; 11 distinct / 12 generated; 0.413s; [full trace](../../formal/conflict/tla/results/draft-crash.log); [cfg](../../formal/conflict/tla/draft-crash.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
7. `<<"Crash", 1>>` — Environmental process death between source operations; `crates/kagi-git/src/resolution.rs:800-806` is not atomic replacement.
8. `<<"Restart(restore draft)", 1>>` — `src/ui/operations/conflict_detect.rs:78-84`; `crates/kagi-git/src/resolution.rs:817-822`; `resolution/drafts.rs:176-247` — corrupt document absent, only matching identities adopted.

<a id="trace-draft-io-quit"></a>
### draft-io-quit

`DraftNeverLost` — expected REFUTE; TLC REFUTED; 15 distinct / 16 generated; 0.43s; [full trace](../../formal/conflict/tla/results/draft-io-quit.log); [cfg](../../formal/conflict/tla/draft-io-quit.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveFailure", 1>>` — `src/ui/conflict_view/draft_owner.rs:233-236` — failure is logged, with no close hold.
9. `<<"CloseWindow", 1>>` — `src/ui/operations/app_bridge.rs:751-758`; `src/ui/editor_workspace/host_close.rs:62-68,107-111` — no lease, no conflict dirty gate.
10. `<<"Restart(restore draft)", 1>>` — `src/ui/operations/conflict_detect.rs:78-84`; `crates/kagi-git/src/resolution.rs:817-822`; `resolution/drafts.rs:176-247` — corrupt document absent, only matching identities adopted.

<a id="trace-draft-torn"></a>
### draft-torn

`DraftWriteAtomic` — expected REFUTE; TLC REFUTED; 20 distinct / 21 generated; 0.43s; [full trace](../../formal/conflict/tla/results/draft-torn.log); [cfg](../../formal/conflict/tla/draft-torn.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"Crash", 1>>` — Environmental process death between source operations; `crates/kagi-git/src/resolution.rs:800-806` is not atomic replacement.
10. `<<"Restart(restore draft)", 1>>` — `src/ui/operations/conflict_detect.rs:78-84`; `crates/kagi-git/src/resolution.rs:817-822`; `resolution/drafts.rs:176-247` — corrupt document absent, only matching identities adopted.

<a id="trace-draft-two-writers"></a>
### draft-two-writers

`DraftWriteAtomic` — expected REFUTE; TLC REFUTED; 178 distinct / 348 generated; 0.468s; [full trace](../../formal/conflict/tla/results/draft-two-writers.log); [cfg](../../formal/conflict/tla/draft-two-writers.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"ObserveIndex", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
10. `<<"ObserveMetadata", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
11. `<<"AcceptRead", 2>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
12. `<<"OpenFile", 2, 2>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
13. `<<"ManualEdit", 2>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
14. `<<"AutosaveRead", 2>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.

<a id="trace-draft-lost-update"></a>
### draft-lost-update

`DraftWriteAtomic` — expected REFUTE; TLC REFUTED; 251 distinct / 466 generated; 0.479s; [full trace](../../formal/conflict/tla/results/draft-lost-update.log); [cfg](../../formal/conflict/tla/draft-lost-update.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ManualEdit", 1>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"ObserveIndex", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
9. `<<"ObserveMetadata", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
10. `<<"AcceptRead", 2>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
11. `<<"OpenFile", 2, 2>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
12. `<<"ManualEdit", 2>>` — `src/ui/conflict_view/draft_owner.rs:13-18,180-187,210-230` — capture changed owned text and immediately enter autosave.
13. `<<"AutosaveRead", 2>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
14. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
15. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
16. `<<"AutosaveWriteStart(truncate)", 2>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
17. `<<"AutosaveWrite", 2>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.

<a id="trace-two-instance-leases"></a>
### two-instance-leases

`RepoHasAtMostOneLease` — expected REFUTE; TLC REFUTED; 3286 distinct / 6508 generated; 0.708s; [full trace](../../formal/conflict/tla/results/two-instance-leases.log); [cfg](../../formal/conflict/tla/two-instance-leases.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"ObserveIndex", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
13. `<<"ObserveMetadata", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
14. `<<"AcceptRead", 2>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
15. `<<"OpenFile", 2, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
16. `<<"Choose", 2>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
17. `<<"AutosaveRead", 2>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
18. `<<"AutosaveWriteStart(truncate)", 2>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
19. `<<"AutosaveWrite", 2>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
20. `<<"Freeze(plan)", 2>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
21. `<<"Approve(confirm)", 2>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.

<a id="trace-two-instance-stale-write"></a>
### two-instance-stale-write

`StaleSnapshotNeverWrites` — expected REFUTE; TLC REFUTED; 9735 distinct / 21230 generated; 0.953s; [full trace](../../formal/conflict/tla/results/two-instance-stale-write.log); [cfg](../../formal/conflict/tla/two-instance-stale-write.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"ObserveIndex", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
15. `<<"ObserveMetadata", 2>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
16. `<<"AcceptRead", 2>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
17. `<<"OpenFile", 2, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
18. `<<"Choose", 2>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
19. `<<"AutosaveRead", 2>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
20. `<<"AutosaveWriteStart(truncate)", 2>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
21. `<<"AutosaveWrite", 2>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
22. `<<"Freeze(plan)", 2>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
23. `<<"Approve(confirm)", 2>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
24. `<<"Preflight", 2>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
25. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
26. `<<"WTWrite", 2>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.

<a id="trace-receipt-continue"></a>
### receipt-continue

`ReceiptFollowsMutation` — expected REFUTE; TLC REFUTED; 44 distinct / 52 generated; 0.437s; [full trace](../../formal/conflict/tla/results/receipt-continue.log); [cfg](../../formal/conflict/tla/receipt-continue.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
15. `<<"Continue", 1, "continue">>` — `crates/kagi-git/src/conflicts.rs:1145,1158-1167,1195-1245`; merge staging-only route `src/ui/operations/conflict.rs:404-408`.
16. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
17. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.

<a id="trace-receipt-merge"></a>
### receipt-merge

`ReceiptFollowsMutation` — expected REFUTE; TLC REFUTED; 44 distinct / 52 generated; 0.454s; [full trace](../../formal/conflict/tla/results/receipt-merge.log); [cfg](../../formal/conflict/tla/receipt-merge.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
15. `<<"Continue", 1, "mergeContinue">>` — `crates/kagi-git/src/conflicts.rs:1145,1158-1167,1195-1245`; merge staging-only route `src/ui/operations/conflict.rs:404-408`.
16. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
17. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.

<a id="trace-receipt-skip"></a>
### receipt-skip

`ReceiptFollowsMutation` — expected REFUTE; TLC REFUTED; 40 distinct / 56 generated; 0.445s; [full trace](../../formal/conflict/tla/results/receipt-skip.log); [cfg](../../formal/conflict/tla/receipt-skip.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"Preserve", 1, "ok">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
14. `<<"Skip", 1, 0>>` — `crates/kagi-git/src/conflicts.rs:1893-1894` — observe live position and invoke --skip without approved-step comparison.
15. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
16. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.

<a id="trace-receipt-save-broken"></a>
### receipt-save-broken

`ReceiptFollowsMutation` — expected REFUTE; TLC REFUTED; 41 distinct / 47 generated; 0.447s; [full trace](../../formal/conflict/tla/results/receipt-save-broken.log); [cfg](../../formal/conflict/tla/receipt-save-broken.cfg). **Artificial broken variant**.

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
15. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
16. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.

<a id="trace-skip-replacement"></a>
### skip-replacement

`SkipDropsOnlyApprovedStep` — expected REFUTE; TLC REFUTED; 51 distinct / 75 generated; 0.448s; [full trace](../../formal/conflict/tla/results/skip-replacement.log); [cfg](../../formal/conflict/tla/skip-replacement.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `"ReplaceOperation(next rebase step)"` — Environmental CLI advances/replaces step; `crates/kagi-git/src/conflicts.rs:1869-1894` has no expected revision and uses the new live position.
13. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
14. `<<"Preserve", 1, "ok">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
15. `<<"Skip", 1, 1>>` — `crates/kagi-git/src/conflicts.rs:1893-1894` — observe live position and invoke --skip without approved-step comparison.

<a id="trace-preserve-abort"></a>
### preserve-abort

`NoDestructiveProgressAfterPreservationFailure` — expected REFUTE; TLC REFUTED; 36 distinct / 42 generated; 0.444s; [full trace](../../formal/conflict/tla/results/preserve-abort.log); [cfg](../../formal/conflict/tla/preserve-abort.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"Preserve", 1, "failed">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
14. `<<"Abort", 1>>` — `crates/kagi-git/src/ops/conflict_abort.rs:263-286,342-351,479-537` — proceed from recovery attempt to index restore/removal.
15. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.

<a id="trace-preserve-skip"></a>
### preserve-skip

`NoDestructiveProgressAfterPreservationFailure` — expected REFUTE; TLC REFUTED; 32 distinct / 48 generated; 0.438s; [full trace](../../formal/conflict/tla/results/preserve-skip.log); [cfg](../../formal/conflict/tla/preserve-skip.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"Preserve", 1, "failed">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
14. `<<"Skip", 1, 0>>` — `crates/kagi-git/src/conflicts.rs:1893-1894` — observe live position and invoke --skip without approved-step comparison.

<a id="trace-preserve-dir-file"></a>
### preserve-dir-file

`NoDestructiveProgressAfterPreservationFailure` — expected REFUTE; TLC REFUTED; 16 distinct / 18 generated; 0.43s; [full trace](../../formal/conflict/tla/results/preserve-dir-file.log); [cfg](../../formal/conflict/tla/preserve-dir-file.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"ChooseNamespace", 1>>` — `src/ui/conflict_view/special.rs:125-129` — D/F keep-file decision, no text merge.
7. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
8. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
9. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
10. `<<"Preserve", 1, "failed">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
11. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.

<a id="trace-merge-tree"></a>
### merge-tree

`MergeCommitTreeIsApprovedTree` — expected REFUTE; TLC REFUTED; 25 distinct / 26 generated; 0.434s; [full trace](../../formal/conflict/tla/results/merge-tree.log); [cfg](../../formal/conflict/tla/merge-tree.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
6. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
7. `<<"ExternalGit", "merge">>` — Environmental CLI write; `crates/kagi-git/src/backend/conflict_observe.rs:91-125,148-198` defines fingerprint coverage; `conflicts.rs:1646-1656` separates WT write/add_path; no Kagi-global lock (`src/app/session.rs:278`, `src/ui/mod.rs:1601`).
8. `<<"RenewMergePlan", 1>>` — `src/ui/operations/commit.rs:1191-1209`; `crates/kagi-git/src/backend.rs:979-998` — rebuild plan from current index, not displayed index.
9. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
10. `<<"MergeCommit", 1>>` — `crates/kagi-git/src/ops/commit.rs:74-83,334-351` — commit checked private index (the newly renewed one).

<a id="trace-sibling-invalidation"></a>
### sibling-invalidation

`SiblingWorktreesInvalidatedOnSharedRef` — expected REFUTE; TLC REFUTED; 62 distinct / 70 generated; 0.447s; [full trace](../../formal/conflict/tla/results/sibling-invalidation.log); [cfg](../../formal/conflict/tla/sibling-invalidation.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"Preserve", 1, "ok">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
14. `<<"Abort", 1>>` — `crates/kagi-git/src/ops/conflict_abort.rs:263-286,342-351,479-537` — proceed from recovery attempt to index restore/removal.
15. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
16. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
17. `<<"RefCAS", 1>>` — `crates/kagi-git/src/ops/conflict_abort.rs:367-400` — restore shared branch using expected-target CAS.
18. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
19. `<<"AppendReceipt", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:581-603` before backend return; deferred Continue `src/ui/operations/conflict.rs:674-698`; Skip `operations/conflict_skip.rs:123-149`.
20. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.
21. `<<"Deliver", 1>>` — `src/app/settle.rs:222-244,272-318`; `src/app/flow.rs:78` excludes Conflict from shared-ref invalidation.

<a id="trace-stash-non-utf8"></a>
### stash-non-utf8

`StashAbortRestoresConflictedBytes` — expected REFUTE; TLC REFUTED; 44 distinct / 50 generated; 0.443s; [full trace](../../formal/conflict/tla/results/stash-non-utf8.log); [cfg](../../formal/conflict/tla/stash-non-utf8.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"Preserve", 1, "ok">>` — `crates/kagi-git/src/ops/conflict_abort.rs:274-286`; `conflicts.rs:1880-1881`; `ops/dir_file_conflict.rs:202-218` — errors nonfatal; direct-write failure can follow truncation (`resolution.rs:800-806`).
14. `<<"Abort", 1>>` — `crates/kagi-git/src/ops/conflict_abort.rs:263-286,342-351,479-537` — proceed from recovery attempt to index restore/removal.
15. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
16. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
17. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.

<a id="trace-continue-liveness"></a>
### continue-liveness

`ContinueEventuallyRecordedOrReported` — expected REFUTE; TLC REFUTED; 73 distinct / 107 generated; 0.475s; [full trace](../../formal/conflict/tla/results/continue-liveness.log); [cfg](../../formal/conflict/tla/continue-liveness.cfg).

1. `"Init"` — Finite fixture: `backend/conflict_observe.rs:91-125`; operation in progress; two-path quotient (not a product transition).
2. `<<"ObserveIndex", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:91-101` — refresh/read index.
3. `<<"ObserveMetadata", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:148-198` — read HEAD and operation metadata separately from index.
4. `<<"AcceptRead", 1>>` — `src/ui/tab_view_access.rs:310-324`; `src/app/conflict.rs:175-188` — attached accepted read publishes authority.
5. `<<"OpenFile", 1, 1>>` — `src/ui/conflict_view/flow.rs:28-31` — select the pending conflict path.
6. `<<"Choose", 1>>` — `src/ui/conflict_view/flow.rs:42-50,90-95`; `actions.rs:129-145` — idle/non-divergent adoption schedules synchronous autosave.
7. `<<"AutosaveRead", 1>>` — `crates/kagi-git/src/resolution/drafts.rs:256-301` — unlocked load + merge; corrupt disk becomes absent via `resolution.rs:817-822`.
8. `<<"AutosaveWriteStart(truncate)", 1>>` — `crates/kagi-git/src/resolution.rs:800-806` — std::fs::write truncates before completing the write.
9. `<<"AutosaveWrite", 1>>` — `crates/kagi-git/src/resolution.rs:800-814` — successful write completes and marks pending edits saved.
10. `<<"Freeze(plan)", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:195-218,244-288` — capture revision and payload; Skip: `conflicts.rs:1809-1854`; merge displayed tree: `src/ui/operations/commit.rs:1054-1065`.
11. `<<"Approve(confirm)", 1>>` — `src/app/flow.rs:261-347`; legacy Continue `src/ui/operations/conflict.rs:374-384,598-631`; Skip `src/ui/conflict_view/actions.rs:432-441`, `operations/conflict_skip.rs:80-87`; merge `operations/commit.rs:1068-1076`.
12. `<<"Preflight", 1>>` — `crates/kagi-git/src/backend/conflict_ops.rs:419-425`; `resolution/conflict_boundary.rs:170-175`; Continue `conflicts.rs:1261-1297`; Skip has no expected-revision argument at `:1869-1894`; merge checks renewed digest `ops/commit.rs:38-44,74-83`.
13. `<<"WTWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1646-1648` Save; `:1380-1404` Continue; `ops/conflict_abort.rs:360-361,513-537,582-583` Abort; `ops/dir_file_conflict.rs:292-316` namespace removal.
14. `<<"IndexWrite", 1>>` — `crates/kagi-git/src/conflicts.rs:1650-1656,1406-1420` rereads WT with add_path; `ops/conflict_abort.rs:342-351,519-531`; `ops/dir_file_conflict.rs:219,228-289`.
15. `<<"Continue", 1, "continue">>` — `crates/kagi-git/src/conflicts.rs:1145,1158-1167,1195-1245`; merge staging-only route `src/ui/operations/conflict.rs:404-408`.
16. `<<"Verify", 1>>` — `crates/kagi-git/src/backend/conflict_observe.rs:348-395,443-467` — Save checks bytes; Abort checks only operation absence/restored HEAD; Continue measured state `conflicts.rs:1234-1245`.
17. `<<"ReleaseLease", 1>>` — `src/app/session.rs:661-669,760-776`; `src/app/settle.rs:147-149` — stopped worker release; no deferred-receipt condition.
18. `<<"Quit", 1>>` — `src/ui/operations/app_bridge.rs:751-758`; `src/ui/mod.rs:3661-3669` — settings/issue draft flush only, not conflict-buffer/accounting callback.
19. `Stuttering` — Temporal suffix: weak owner is gone, so `src/ui/operations/conflict.rs:674` cannot run the UI append; no enabled callback is unfairly postponed.

## P1 reconciliation checklist

- Manual keystroke consumed before autosave: **refuted as a current-code bug** by `ManualCapturePersisted` and the changed-capture persistence call at `draft_owner.rs:13-18`; negative control red.
- Skip executes a replacement step: **concrete trace**, approved step 0, replacement step 1, Skip drops 1.
- WT-only edit is not stale: **concrete trace**, same full revision passes preflight and overwrites external WT; owner must define snapshot scope.
- Draft crash/concurrent writer loss: **three concrete traces**, interrupted truncate/restart, a second reader during truncation, and stale load/merge/write erasing a completed different-file save.
- Destructive progress after preservation failure: **three concrete traces**, Abort index mutation, Skip CLI, and D/F losing namespace index removal after failed preservation. The D/F trace stops at the first destructive index write; the enabled following WTWrite models directory removal at `dir_file_conflict.rs:315-316`.
- Continue/Skip success before accounting/close release: **concrete traces**; fair Continue liveness trace goes through Quit to permanent stutter. Merge handoff success has no append action at all.
- Merge confirmation renews staged set: **concrete trace**, shown tree `shown`, external tree `external`, renewed plan and committed tree `external`. Backend digest protects only the renewed plan.
- Non-UTF-8 stash abort: **concrete trace**, raw conflict entry removed, filtered checkout list empty, WT remains `markers`, Verify succeeds on operation absence/HEAD. The empty-list return guard confirms the no-op, not safety.

No product changes are proposed as already implemented. Fix sketches in the ledger are owner decisions, and all model-to-runtime consequences above remain source-grounded [INFERENCE] pending fixture reconciliation.


## Trace-checking (2026-10-24)

The refined model admits 34 saved drill projections and rejects a forged Skip
without an arm. `skip-replacement` HOLDs under refresh-disarm. #1746 D2 makes
`skip-confirm-cli-race` HOLD under frozen approval and immediate pre-CLI validation,
including replacement after initial preflight. `skip-prewrite-broken` removes
those checks and remains red; `skip-refresh-broken` independently checks disarming.
Unreadable operation metadata retains its named HOLD guard and negative control.
See the [trace reconciliation appendix](conflict-trace-ledger.md) for the historical
34-trial table and its evidence boundaries; those saved drills did not exercise
the Rust regression's post-confirm replacement schedule.
