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
REFUTATIONS remain harness-green while tracked by #1731–#1733, #1744, #1745, #1746 (model-external runtime findings: #1734–#1738).
The maintained lifecycle/pure expected-outcome tables linked above map each
REFUTE to its issue, owner QUESTION, negative control, or runtime CONTRACT.
The historical ledger below retains its original extraction classifications.
Update a model
and its expectation to HOLD in the same PR as the corresponding product fix;
never weaken an assertion just to restore green.

## P2 maintained-model cutover — #1740, #1743

The current `tla/run.py:CHECKS` list contains **60 configurations**, including
**20 deliberate negative controls**. No TLC execution or new state counts
are claimed by this cutover; the parent validation command supplies that evidence.
Historical traces/counts below remain evidence about the original model only.

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
The draft models and #1732 expectations are intentionally unchanged.

---

## Reproduction and evidence boundary

- Source: `/tmp/kagi-conflict-hunk-20261023` (assignment identifies HEAD `6f9696df`), read-only; no Cargo, product edits, GUI runs, Git mutations or GitHub posts.
- Method: playbook §§0–6, every lifecycle-map section, and the implementation sections cited below were read before modelling. This is **TLC evidence about a hand-extracted finite model**, not an executed Rust/GUI reproduction. BUG means a model trace contradicts a cited declared product contract; reconciliation with an actual fixture remains necessary.
- Re-run from the repository root: `uv run --project formal/conflict formal/conflict/run.sh`. `formal/conflict/tla/run.py` contains the self-check table; the committed `.cfg` files are read without rewriting. Select a debugging subset with `--suite tla CHECK` without overwriting the full-suite summary.
- Dependencies: CPython >=3.11 via `formal/conflict/pyproject.toml` and `uv.lock`; Java 21 on PATH (CI: Temurin 21); pinned TLC release `v1.8.0`, downloaded to cache and verified against `formal/conflict/tla/TLA2TOOLS.sha256` (SHA-256 `d15655eeaff4b3f7b3d65c46b838bc0f35d131b2ef4b1e473689b62c98d4726f`). Historical observed runtime: Adoptium 21.0.13. No third-party Python dependency is used.
- Machine exit: 0 only when every named invariant/property has its expected verdict **and** TLC has the appropriate zero/nonzero exit; 1 unexpected verdict; 2 launch/timeout/unknown-check errors. Expected refutations are intentionally green at the harness level, red at TLC level.
- Initial historical run before trace refinement: **44 / 44 expected outcomes, 22.441 seconds, 59,260 distinct states summed across independent runs**. The maintained model now has **60** checks, including the P2 controls above; see also the [trace appendix](conflict-trace-ledger.md). Counts are TLC distinct states found, not globally unique or exhaustive after a first safety counterexample. Current machine-readable evidence lives under `formal/conflict/tla/results/`.

## Declared vs implicit

**Declared:** repository writes must follow plan → confirm → preflight → execute → verify → oplog (`AGENTS.md:23-37`); Save/Continue/Commit retain frozen revision/owner/lease checks and Save stages the explicitly reviewed file (`docs/adr/0227-progressive-conflict-resolution.md:126-128`); Abort preserves partial resolutions (`crates/kagi-git/src/conflicts.rs:27-33`, `ops/conflict_abort.rs:274-280`); D/F removal is recoverable because a snapshot was taken (`ops/dir_file_conflict.rs:299-300`); shared-ref changes invalidate sibling worktrees (`src/app/settle.rs:272-300`).

**Historical implicit behavior (superseded where fixed above):** WT bytes were not fingerprinted; Save staged by rereading WT; draft JSON used unlocked truncate/write; persistence errors were logged/ignored; Skip consumed a newly observed live step; Continue/Skip released before UI recording; merge handoff success had no append; merge finalization renewed its plan; Conflict was classified as not shared-ref-changing; stash abort filtered paths through UTF-8 and verified only operation/HEAD. These describe the original extraction, not the maintained P2 transitions.

**Map disagreement, material:** map §5 manual-keystroke P1 is obsolete in the inspected code. `draft_owner.rs:13-18` now calls `autosave_drafts` when capture changes text, and `:184-185` invokes capture on Change. `manual-capture.cfg` holds; deleting that persistence call in `manual-capture-broken.cfg` goes red. The nearby `flow.rs:19` comment that navigation “never persists” is also stale relative to the call at `:21`/`:29`. The strong Crash/Quit invariant still fails for real crash windows and failed I/O, not for the obsolete successful-callback ordering story. Other map line offsets drifted; citations here name inspected current lines.

## Finite scope and honest model/code gaps

- Two independent application instances share one repository and one draft file; leases remain per-instance. Concurrent configs explore **all enabled interleavings** of both instances. Single-route configs leave the second actor idle to isolate minimal witnesses. A passive sibling worktree view is represented separately by its invalidation bit.
- At most two paths; one abstract hunk per edited text path; text/blob/namespace payloads are finite enums. Full revision is a tuple of bounded index/HEAD/metadata/operation revision counters. One operation per actor, one external CLI edit/replacement, at most one restart. No hash collisions, path aliases, or SHA internals are modelled.
- Scenario selectors are **environment/scope restrictions**, not product guards. Index/HEAD/MERGE_MSG external edits are separate runs before final preflight; WT-only edits are before preflight or between WT write and add_path; index CLI interference and two-app interference also occur after final preflight. Observation is explicitly split into index/metadata/accept, but these configs do not claim exhaustive mid-observation metadata tearing coverage.
- Read failures/trust/path/marker/identity validators unrelated to the witness are fixed to successful, known, compatible inputs. A failed preflight is a terminal nonmutating path (deadlock checking disabled); rejection UI text is outside this quotient. Continue has one resolved text path and no unexpected unmerged path. D/F has a namespace decision, not a fictitious text hunk. MergeCommit starts at an already resolved commit panel.
- Synchronous edit capture → autosave is split only for crash and cross-process interleaving; **same-process graceful Close/Quit cannot interrupt that callback**. Restart restores same-revision compatible records; corrupted JSON is absent. The two draft writers edit distinct paths, so a missing second record is not legitimate same-file latest-edit coalescing.
- `draft-lost-update` restricts reads to complete JSON to isolate stale overwrite from the shorter torn-read witness. Abort/Skip preservation failure can occur after truncation and destroy an earlier successful save; D/F snapshot failure is nonfatal before namespace deletion.
- IndexWrite now stages the frozen payload; `stagePayload` restores current-WT `add_path` behavior. The index/WT write boundaries remain separate and no stronger exclusion of arbitrary CLI interference is claimed. Raw resolutions, multi-file Continue rollback, cached unrelated-index clobbering, signature/hooks, and detailed CLI retry outcomes remain outside these lifecycle traces; pure-state checks are separate work.
- Receipt HOLD assumes append succeeds and no abrupt process death. It is an ordering guarantee, not fsync/power-loss durability. Continue liveness retains weak fairness for each actor. UI destruction during a dispatched worker does not destroy that worker; release ends its retained lifetime after append. The negative control restores the closed-owner callback loss.
- Safety counterexamples below are BFS-shortest in their configured finite slice. TLC's liveness lasso has no unnecessary loop before its terminal stutter; it is not claimed to minimize real UI gestures. Every event is mapped to source lines below; source-reviewed executable consequences remain [INFERENCE] until a product fixture confirms them.

## Ledger table

State counts separated by `/` follow the check/trace order in that row. CONTRACT rows are bounded holds, not global proofs. BUG/QUESTION classifications apply to the corresponding implementation interpretation, not to artificial broken variants.

| Invariant | Expectation | Result | States explored | Trace (if refuted) | Code lines | Classification | Owner question / fix sketch |
|---|---|---|---:|---|---|---|---|
| `NoWriteBeforeApproval` | HOLD | HELD | 34811 total (8 runs) | — | `app/flow.rs:261-347; app/session.rs:516-558` | CONTRACT | Retain confirmation/admission guards for all seven writer routes and both instances. |
| `ApprovedRevisionFrozen (merge + staging races)` | HOLD | Awaiting parent TLC run | See maintained results | Historical traces retained below | Frozen merge plan; frozen staging payload | CONTRACT (#1743) | `mergePlan` and `stagePayload` controls retain all three original flaws. |
| `ApprovedSavePayloadFrozen (WT payload)` | HOLD | HELD (historical) | 65 | — | `backend/conflict_ops.rs:195-218,281-288; resolution/conflict_boundary.rs:103-107,170-195` | CONTRACT | Keep immutable payload binding at WT materialization and index staging. |
| `StaleSnapshotNeverWrites (WT edit before per-file validation)` | HOLD | HELD | 87 | — (historical [stale-worktree](#trace-stale-worktree)) | `resolution/conflict_boundary.rs::validate_save_binding, validate_current_paths; resolution/worktree_state.rs::validate` | CONTRACT (#1733) | Freeze the live draft's WT-byte fingerprint and refuse changed bytes before mutation; broken WT guard remains a negative control. |
| `StaleSnapshotNeverWrites (WT edit + watcher before first choice)` | HOLD | HELD | See maintained TLC results | — | `resolution/drafts.rs::retain_drafts; resolution/worktree_state.rs::retain_provenance` | CONTRACT (#1733 review) | Untouched live files retain the original fingerprint too. `stale-worktree-before-choice-broken` removes only that carry and must REFUTE. Exact frozen-text rewrites remain idempotently retryable after Partial execution. |
| `StaleSnapshotNeverWrites (final-check gap / second instance)` | REFUTE | REFUTED | See maintained TLC results | [stale-after-preflight](#trace-stale-after-preflight), [two-instance-stale-write](#trace-two-instance-stale-write) | `conflicts.rs::execute_conflict_save_with_progress; resolution/worktree_state.rs::validate` | QUESTION (CLI TOCTOU); #1746 D3 (instances) | Keep validation and write as distinct model transitions. A last-read fingerprint does not exclude arbitrary CLI writes or serialize independent instances. |
| `StaleSnapshotNeverWrites (index/HEAD/MERGE_MSG drift before preflight)` | HOLD | HELD | 71 / 71 / 71 | — | `backend/conflict_ops.rs:419-425; resolution/conflict_boundary.rs:170-175; conflicts.rs:1261-1297` | CONTRACT | Retain full-revision refusal; it protects preflight-time drift, not the later write window. |
| `DraftNeverLost (captured edit, same revision)` | REFUTE | REFUTED | 11 / 15 | [draft-crash](#trace-draft-crash), [draft-io-quit](#trace-draft-io-quit) | `ui/conflict_view/draft_owner.rs:13-18,233-236; resolution.rs:800-822; ui/editor_workspace/host_close.rs:62-68` | QUESTION (instant crash); BUG (silent I/O failure + close) | Define the capture/durability boundary; failed persistence must keep close blocked or visibly require recovery. |
| `DraftWriteAtomic` | REFUTE | REFUTED | 20 / 178 / 251 | [draft-torn](#trace-draft-torn), [draft-two-writers](#trace-draft-two-writers), [draft-lost-update](#trace-draft-lost-update) | `resolution.rs:800-822; resolution/drafts.rs:253-301; conflicts.rs:27-33` | BUG | Use atomic replacement plus interprocess serialization/CAS of the load-merge-write document. |
| `ManualCapturePersisted (successful synchronous callback)` | HOLD | HELD | 14 | — | `ui/conflict_view/draft_owner.rs:13-18,180-187` | CONTRACT; map P1 DISPROVED | The current callback already autosaves; remove the obsolete map suspicion, not the working guard. |
| `ReceiptFollowsMutation (Continue / merge handoff / Skip)` | HOLD | Awaiting parent TLC run | See maintained results | Historical traces retained below | Backend execute/observe/finalize before worker settlement | CONTRACT (#1740) | Three `workerReceipt` controls restore release-before-append or absent merge accounting. |
| `ReceiptFollowsMutation (Save, append succeeds)` | HOLD | HELD | 65 | — | `backend/conflict_ops.rs:581-603; app/settle.rs:147-149` | CONTRACT | Keep backend-owned receipt before completion; no crash/fsync or append-failure guarantee is inferred. |
| `SkipDropsOnlyApprovedStep` | HOLD | HELD after #1746 D2 | see current TLC summary | historical [skip-replacement](#trace-skip-replacement) | `conflicts.rs::preflight_skip_revision`, `execute_conflict_skip`; `conflict_view/actions.rs::skip_request_arm` | CONTRACT | Frozen full revision and operation survive plan/confirm/dispatch; both initial and immediately-pre-CLI checks refuse a replacement. Rust `skip_refuses_replaced_approved_step_and_preserves_the_next_conflict` proves step 2 survives. |
| `NoDestructiveProgressAfterPreservationFailure (Abort / Skip / D-F)` | REFUTE | REFUTED | 36 / 32 / 16 | [preserve-abort](#trace-preserve-abort), [preserve-skip](#trace-preserve-skip), [preserve-dir-file](#trace-preserve-dir-file) | `ops/conflict_abort.rs:274-286,342-351; conflicts.rs:1880-1894; ops/dir_file_conflict.rs:202-222,299-316` | BUG | Fail closed when the required recovery copy cannot be created; do not destroy the previous copy in place. |
| `MergeCommitTreeIsApprovedTree` | HOLD | Awaiting parent TLC run | See maintained results | Historical [merge-tree](#trace-merge-tree) | Approved merge plan; `ops/commit.rs::approved_commit_index` | CONTRACT (#1743) | Preserve approved index snapshot; `mergePlan` restores replanning. |
| `SiblingWorktreesInvalidatedOnSharedRef` | REFUTE | REFUTED | 62 | [sibling-invalidation](#trace-sibling-invalidation) | `ops/conflict_abort.rs:367-400; app/flow.rs:78; app/settle.rs:272-300` | BUG | Mark ref-moving Abort as shared-ref-changing so retained sibling worktree views are invalidated. |
| `ContinueEventuallyRecordedOrReported` | HOLD | Awaiting parent TLC run | See maintained results | Historical [continue-liveness](#trace-continue-liveness) | Backend accounting survives UI destruction | CONTRACT (#1740) | `workerReceipt` restores weak-owner callback loss. |
| `RepoHasAtMostOneLease (two independent apps)` | REFUTE | REFUTED | 3286 | [two-instance-leases](#trace-two-instance-leases) | `app/session.rs:261-278,544-558; ui/mod.rs:1601` | QUESTION | Is repository-global admission promised, or is independent-app concurrency intentionally unsupported? |
| `StashAbortRestoresConflictedBytes (non-UTF-8)` | REFUTE | REFUTED | 44 | [stash-non-utf8](#trace-stash-non-utf8) | `ops/conflict_abort.rs:513-537,582-583; backend/conflict_observe.rs:443-467` | BUG | Keep raw path authority through checkout; verify restored WT/index bytes, not only HEAD/operation absence. |

## Load-bearing negative controls

These cfg files set `Broken` in the same source model; they are deliberate model patches, not product fixes. The ordinary cfg values remain `"none"`. No expected product refutation is reclassified as a negative-control bug.

| HELD invariant | Broken cfg | Deliberately removed guard | TLC result | States |
|---|---|---|---|---:|
| `NoWriteBeforeApproval (all route configs)` | [`approval-broken.cfg`](../../formal/conflict/tla/approval-broken.cfg) | Bypass the explicit approval gate while keeping preflight | REFUTE (exit 12) | 28 |
| `ApprovedSavePayloadFrozen` | [`save-payload-broken.cfg`](../../formal/conflict/tla/save-payload-broken.cfg) | Remove payload == frozen binding guard at WT materialization | REFUTE (exit 12) | 29 |
| `StaleSnapshotNeverWrites (index)` | [`stale-index-broken.cfg`](../../formal/conflict/tla/stale-index-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 43 |
| `StaleSnapshotNeverWrites (HEAD)` | [`stale-head-broken.cfg`](../../formal/conflict/tla/stale-head-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 43 |
| `StaleSnapshotNeverWrites (MERGE_MSG)` | [`stale-metadata-broken.cfg`](../../formal/conflict/tla/stale-metadata-broken.cfg) | Remove consolidated final revision/identity preflight guard | REFUTE (exit 12) | 43 |
| `ManualCapturePersisted` | [`manual-capture-broken.cfg`](../../formal/conflict/tla/manual-capture-broken.cfg) | Remove persistence on changed captured input | REFUTE (exit 12) | 6 |
| `ReceiptFollowsMutation (Save)` | [`receipt-save-broken.cfg`](../../formal/conflict/tla/receipt-save-broken.cfg) | Remove append-before-release ordering gate | REFUTE (exit 12) | 41 |
| `ReceiptFollowsMutation; ContinueEventuallyRecordedOrReported` | `receipt-continue-broken`, `receipt-merge-broken`, `receipt-skip-broken`, `continue-liveness-broken` | `workerReceipt`: release before UI-owned append; omit merge append | Expected REFUTE; pending parent run | — |
| `ApprovedRevisionFrozen; MergeCommitTreeIsApprovedTree` | `approved-revision-broken`, `merge-tree-broken` | `mergePlan`: silently renew approved tree/revision | Expected REFUTE; pending parent run | — |
| `ApprovedRevisionFrozen` | `approved-index-race-broken`, `approved-two-writers-broken` | `stagePayload`: stage mutable WT instead of approved payload | Expected REFUTE; pending parent run | — |

The source has several repeated validation reads; the model deliberately consolidates the *last* full-revision/binding guard. The broken revision variant removes that consolidated check, not a claim that deleting only one of several Rust checks would suffice. The payload negative variant admits a non-frozen token at the write boundary to exercise the binding assertion. MergeCommit's modeled lease represents its synchronous busy UI interval, not a claim that its legacy synchronous path allocates a Sessions lease.

## Historical complete minimal event traces

The values below were copied from the original TLC `event` field without reordering. Their verdicts, source offsets, and counts describe that historical extraction, not new executions after the P2 cutover. For #1740/#1743, current baselines expect HOLD and the paired broken configs preserve refutability; see the maintained table above. Each event has its enabling source lines; 1/2 denote independent app instances. For artificial negative controls, interpret the removed guard as the changed semantics.

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
