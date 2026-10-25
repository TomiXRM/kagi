# Pure conflict predicate counterexample ledger

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
REFUTATIONS remain harness-green while tracked by #1732–#1733 and #1746 (model-external runtime findings: #1736–#1738).
The maintained tables mark C1–C4 and C6 fixed by #1746.
C5 is CONTRACT at runtime (metadata guard,
`continue-metadata-read` HOLD), not an end-to-end fail-open issue.
Unless explicitly amended below, the historical evidence keeps its original classifications.
The #1734/#1735 [lifecycle amendment](conflict-lifecycle-ledger.md#1734--1735-deletion-and-parked-namespace-amendment--2026-10-25)
adds deletion/namespace checks for formerly model-external findings. Explicit
deletion is a confirmed operation, not persisted empty text; these text/raw
draft-readiness predicates and their negative controls are intentionally unchanged.
Update a model
and its expectation to HOLD in the same PR as the corresponding product fix;
never weaken an assertion just to restore green.

### #1746 C1–C4 amendment

C1 persists adopted zero-line sides; C3 checks bounds before materialization;
C4 validates restored mask dimensions and bounds-checks source assembly.
C2's old `AssembleNoMarkerLeak` was mis-posed: raw literal/manual marker text
is emitted by design; blocking is a provenance decision. Its replacement
`MarkerGateRespectsChosenSourceProvenance` checks both unblocked source literals
and blocked manual markers. Rust regressions live in
`crates/kagi-domain/src/resolution/hardening_tests.rs`; backend coverage in
`crates/kagi-git/tests/conflicts/domain_hardening.rs` exercises readiness,
Continue blockers, direct Save, recorded Save and provenance-only tampering.
The #1751 parser collision is closed by choosing delimiters wider than every
line-start marker run in all source blobs, carrying that width with the
materialization, and parsing the exact width with state-specific delimiters.
`marker_parser_preserves_a_literal_source_opener` is now an ordinary passing
regression. Backend/UI tests cover literal separators and incoming openers,
nine-character runs (width 10), clean changes outside hunks on both sides, exact
direct/recorded saves, and legacy width-7 drafts preserved for read-only
Operation Log inspection. Reconstruction against stage blobs is not a valid
proof: merged passthrough legitimately contains the other side's clean changes.
New removed-guard mutants: `empty-adoption-lost`, `invalid-materializes`,
`malformed-index`, `plain-text-gate`. The historical mechanisms and original
mutant witnesses below describe the pre-fix extraction, not current defects.

The superseded `undecide-on-select` mutant is no longer load-bearing once empty
adoption is explicit; `undecide-empty-adoption` instead discards a chosen deletion
when the opposite side is deselected. All five amended negative controls exit 1:

| Mutant | First red property | Witness |
|---|---|---|
| empty-adoption-lost | SideStateChoiceMaskEquivalentIncludingEmpty | lengths 0/0, Current: All → None |
| invalid-materializes | InvalidLineFailureIsNoOp | unresolved 0/0, index 0 returns false but creates masks |
| malformed-index | MalformedSelectionNeverPanics | source lengths 0/1, masks `[true]`/`[]`, positional CurrentFirst |
| plain-text-gate | MarkerGateRespectsChosenSourceProvenance | chosen Current literal `<<<<<<< user` is incorrectly blocked |
| undecide-empty-adoption | UndecideOnlyOnDeselect | chosen empty Current, deselect empty Incoming incorrectly resets |

### #1732 post-merge recovery equality amendment — 2026-10-25

The complete maintained pure run passes **39 properties / 1,695,651 baseline
property/input evaluations**, detecting all **36** deliberate mutants.

`TwoWriterRecoveryRetained` adds 72 finite evaluations: two known identities,
both orderings of distinct bodies from three representatives, disk adoption or
assembled-hunk overrides, and three subsequent other-file saves. The stale
editor's local target body is not the primary being serialized. Compare recovery
equality with that actual record, preserving the exact earlier bytes/identity for
the owned read-only inspection API. The `pre-merge-recovery` mutant restores the
lost-update bug. This is a body/metadata equality extraction, not a proof of CAS,
locking, codec behavior or complete Rust/model equivalence; the lifecycle
`two-writer-recovery` refinement and real backend subprocess test cover those
successful transaction sequences independently.




---

Source snapshot: `/tmp/kagi-conflict-hunk-20261023`, supplied HEAD `6f9696df`. Rust was read directly, not executed. No product changes.

## Method and limits

Playbook §1 decision for every predicate: deterministic + finite equality/order quotient or explicitly bounded vector domain → exhaustive Python, not Z3. Unbounded text/identity magnitudes do not affect equality/presence decisions. Assembly and history coverage are bounded (line/source lengths 0–3, malformed mask lengths 0–4, recovery history length 0–3); HOLD is a contract over that domain, not an unbounded theorem or a Rust execution result. Provenance, empty vs absent, all four orders, raw payload membership, unknown authority and I/O errors remain distinct.

Readiness takes a supplied cached marker verdict: `ReadyImpliesNoMarkers` means no *cached* marker residue; cache coherence/interleavings belong to the lifecycle models. Marker-bearing source/manual text is enumerated separately from synthetic marker insertion. `Some([])` is not `None`. Path identity and hash computation/codec correctness are not asserted here. File-path permutations are represented by stable positional names; marker-residue subsets include an outside-session path. Canonical index/identity failures remain `ERROR`, never `Ok([])`.

## Run and exit contract

From the repository root: `uv run --project formal/conflict formal/conflict/run.sh --suite pure` (omit `--suite pure` for both model families). Dependencies are declared in `formal/conflict/pyproject.toml` and `uv.lock`: Python >=3.11, standard library only. Individual models: `uv run --project formal/conflict python formal/conflict/pure/predict_hunks.py --self-check`; all support `--broken NAME`. Exit 0 = every named expected HOLD/REFUTE matches, exit 1 = mismatch, exit 2 = invalid CLI/tooling. Suite exits 0 only if all baselines exit 0 and every broken variant exits exactly 1 with a named mismatch. Refuted expectations are green only with a concrete witness. Full normal and mutant witnesses: `formal/conflict/pure/results.json`.

Observed after the #1746 amendment: **38 properties; 1,695,579 baseline property/input evaluations; 35 deliberate mutants**. These are property evaluations, not deduplicated concrete repository states. Per-property domain size below counts every evaluated input (conditional properties count their antecedent domain only).

## Ledger

| predicate | property (expected) | method | domain size | result | counterexample | Rust lines | classification | existing Rust test? |
|---|---|---|---:|---|---|---|---|---|
| hunk/assembly | SideStateConsistent (HOLD) | exhaustive | 10800 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:110-131,174-181,306-321 | CONTRACT | crates/kagi-domain/src/resolution.rs:972 |
| hunk/assembly | SideStateChoiceMaskEquivalentIncludingEmpty (HOLD) | exhaustive | 192 | HOLD | none in domain | resolution.rs::LineSelection::side_state/from_choice | CONTRACT (#1746 C1 fixed) | hardening_tests.rs::empty_side_state_survives_choice_materialization |
| hunk/assembly | SelectionPresenceResolvesEvenEmpty (HOLD) | exhaustive | 5400 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:297-301 | CONTRACT | crates/kagi-domain/src/resolution/order_tests.rs:28-62 |
| hunk/assembly | ChildTriStateAggregation (HOLD) | exhaustive | 40 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:124-131,536-538 | CONTRACT | crates/kagi-domain/src/resolution.rs:972 |
| hunk/assembly | AssembleAllFourOrdersAndProvenance (HOLD) | exhaustive | 5400 | HOLD | none in domain | crates/kagi-domain/src/resolution/assembly.rs:17-46 | CONTRACT | crates/kagi-domain/src/resolution/order_tests.rs:86-113,140-178 |
| hunk/assembly | AssembleBlockOrderStable (HOLD) | exhaustive | 92 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:78-85,157-173,564-577; crates/kagi-domain/src/resolution/assembly.rs:17-46 | CONTRACT | crates/kagi-domain/src/resolution/order_tests.rs:116,128 |
| hunk/assembly | AssembleNoSyntheticMarkers (HOLD) | exhaustive | 5480 | HOLD | none in domain | crates/kagi-domain/src/resolution/assembly.rs:48-70 | CONTRACT | crates/kagi-domain/src/resolution.rs:1023 |
| hunk/assembly | MarkerGateRespectsChosenSourceProvenance (HOLD) | exhaustive | 30 | HOLD | none in domain | resolution.rs::resolved_lines_have_conflict_marker | CONTRACT (#1746 C2 fixed) | hardening_tests.rs::marker_gate_respects_chosen_source_provenance |
| hunk/assembly | UndecideOnlyOnDeselect (HOLD) | exhaustive | 92544 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:334-351,554-577 | CONTRACT | crates/kagi-domain/src/resolution.rs:723,750; crates/kagi-domain/src/resolution/order_tests.rs:54-61 |
| hunk/assembly | OrderApplicability (HOLD) | exhaustive | 21984 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:580-627 | CONTRACT | crates/kagi-domain/src/resolution/order_tests.rs:7,28,86,171 |
| hunk/assembly | InvalidLineFailureIsNoOp (HOLD) | exhaustive | 64 | HOLD | none in domain | resolution.rs::HunkModel::set_hunk_line | CONTRACT (#1746 C3 fixed) | hardening_tests.rs::invalid_hunk_line_edit_is_a_complete_noop |
| hunk/assembly | MalformedSelectionNeverPanics (HOLD) | exhaustive | 61504 | HOLD | none in domain | resolution/assembly.rs::ConflictHunk::visit_resolved_lines | CONTRACT (#1746 C4 fixed) | hardening_tests.rs::stale_selection_dimensions_never_panic_or_emit_missing_lines |
| hunk/assembly | ChoiceSelectionPreservesSourceChoices (HOLD) | exhaustive | 64 | HOLD | none in domain | crates/kagi-domain/src/resolution.rs:157-173; crates/kagi-domain/src/resolution/assembly.rs:17-58 | CONTRACT | crates/kagi-domain/src/resolution.rs:863; crates/kagi-domain/src/resolution/order_tests.rs:116,128 |
| hunk/assembly | RegionAssemblyPreservesOrder (HOLD) | exhaustive | 14425 | HOLD | none in domain | crates/kagi-domain/src/resolution/assembly.rs:76-84 | CONTRACT | crates/kagi-domain/src/resolution.rs:789 |
| ready | ReadyImpliesNoMarkers (HOLD) | exhaustive | 21904 | HOLD | none in domain | src/ui/conflict_view/flow.rs:115-122; crates/kagi-git/src/resolution.rs:627-656; crates/kagi-domain/src/resolution/assembly.rs:87-92 | CONTRACT | crates/kagi-git/tests/conflicts_test.rs:440,2254 |
| manual divergence | ManualDivergenceIsTextual (HOLD) | exhaustive | 21904 | HOLD | none in domain | src/ui/conflict_view/flow.rs:115-122; crates/kagi-git/src/resolution.rs:627-656; crates/kagi-domain/src/resolution/assembly.rs:87-92 | CONTRACT | None found for this exact property |
| ready | EmptyResultIsAResolution (HOLD) | exhaustive | 148 | HOLD | none in domain | src/ui/conflict_view/flow.rs:115-122; crates/kagi-git/src/resolution.rs:627-656; crates/kagi-domain/src/resolution/assembly.rs:87-92 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:207 |
| manual divergence | MissingResultOrModelNotDivergent (HOLD) | exhaustive | 21904 | HOLD | none in domain | src/ui/conflict_view/flow.rs:115-122; crates/kagi-git/src/resolution.rs:627-656; crates/kagi-domain/src/resolution/assembly.rs:87-92 | CONTRACT | None found for this exact property |
| continue blockers | BlockersOrderedAndComplete (HOLD) | exhaustive | 577000 | HOLD | none in domain | crates/kagi-git/src/conflicts.rs:803-929; crates/kagi-git/src/resolution/conflict_boundary.rs:45-79 | CONTRACT | crates/kagi-git/tests/conflicts_test.rs:911 |
| continue blockers | IndexContextIdentityErrorsFailClosed (HOLD) | exhaustive | 767824 | HOLD | none in domain | crates/kagi-git/src/conflicts.rs:803-929; crates/kagi-git/src/resolution/conflict_boundary.rs:45-79 | CONTRACT | None found for this exact property |
| merge message | MessageReadErrorNeverFailOpen (REFUTE) | exhaustive | 5 | REFUTE | {"actual":["OK",[]],"events":["merge in progress, no unresolved files","read_to_string(MERGE_MSG) => permission/I/O/invalid UTF-8 error","merge_message_is_empty => false","continue_blockers => Ok([])"],"message":"error"} | crates/kagi-git/src/conflicts.rs:914-929 | QUESTION | None found for non-ENOENT read error |
| merge message | MessageContentGate (HOLD) | exhaustive | 5 | HOLD | none in domain | crates/kagi-git/src/conflicts.rs:925-929 | CONTRACT | crates/kagi-git/tests/conflicts_test.rs:911 |
| UI continue | UIBlockerPrecedence (HOLD) | exhaustive | 28850 | HOLD | none in domain | src/ui/conflict_view.rs:360-391 | CONTRACT | None found for this exact property |
| draft match | DraftNeverAdoptsUnknownAuthority (HOLD) | exhaustive | 13824 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:90-105,186-234 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:166; conflicts/draft_unknown.rs:25,69 |
| draft match | DraftPayloadCompatibility (HOLD) | exhaustive | 13824 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:94-105 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:287,451 |
| draft precedence | LatestExplicitNullOutranksOldBody (HOLD) | exhaustive | 48 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:176-234 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:207,581 |
| draft precedence | NewestCompatibleRecoveryWins (HOLD) | exhaustive | 1755 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:215-249 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:479 |
| draft serialization | UnknownOwnerCoalescenceIsFileOnly (HOLD) | exhaustive | 324 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:253-290 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:166 (authority); no exact pure owner predicate test |
| draft serialization | OnlyPendingRecoveryOutranksDisk (HOLD) | exhaustive | 3160 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:260-267,303-311 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:659; crates/kagi-git/tests/conflicts/draft_recency.rs:7 |
| draft preservation | DedupMovesLatestToTail (HOLD) | exhaustive | 504 | HOLD | none in domain | crates/kagi-git/src/resolution/drafts.rs:160-173 | CONTRACT | crates/kagi-git/tests/conflicts/draft_revision.rs:659 |
| draft serialization | TwoWriterRecoveryRetained (HOLD) | exhaustive | 72 | HOLD in maintained guard | none in domain; pre-merge mutant drops the first recovery | `resolution/drafts.rs::autosave_json; resolution/draft_merge.rs::matches_record` | CONTRACT (#1732 P1) | `resolution/draft_storage_tests.rs::two_writer_recovery_survives_other_file_autosaves`; independent-hunk merge both orderings |
| Skip classification | SkipClassificationNoFalseSuccess (HOLD) | exhaustive | 1152 | HOLD | none in domain after #1746 C6 | crates/kagi-domain/src/sequencer_skip.rs:125-151; crates/kagi-git/src/backend/conflict_continue.rs::conflict_skip_outcome | CONTRACT | `zero_exit_requires_observed_movement_while_operation_remains`; `zero_exit_without_movement_maps_to_warning_not_success` |
| Skip evidence | BrokenSignalDominatesMovement (HOLD) | exhaustive | 1152 | HOLD | none in domain | crates/kagi-domain/src/sequencer_skip.rs:87-102 | CONTRACT | crates/kagi-domain/src/sequencer_skip.rs:263-334 |
| Skip classification | NextConflictIsSuccessOnObservedMovement (HOLD) | exhaustive | 46 | HOLD | none in domain | crates/kagi-domain/src/sequencer_skip.rs:139-146 | CONTRACT | crates/kagi-domain/src/sequencer_skip.rs:176,186 |
| Skip classification | FinishedRequiresCleanRepository (HOLD) | exhaustive | 1152 | HOLD | none in domain | crates/kagi-domain/src/sequencer_skip.rs:126-138 | CONTRACT | crates/kagi-domain/src/sequencer_skip.rs:218 |
| Skip classification | NonzeroUnchangedIsNoProgress (HOLD) | exhaustive | 33 | HOLD | none in domain | crates/kagi-domain/src/sequencer_skip.rs:139-146 | CONTRACT | crates/kagi-domain/src/sequencer_skip.rs:232 |
| UI intent | IntentAdmissionConjunctive (HOLD) | exhaustive | 882 | HOLD | none in domain | src/ui/operations/conflict.rs:159-170,859-869 | CONTRACT | src/ui/operations/conflict.rs:890 |
| plan admission | PlanApprovalConjunctive (HOLD) | exhaustive | 128 | HOLD | none in domain | src/app/flow.rs:204-214,261-299 | CONTRACT | tests/app_writer_admission_test.rs (integration family; not executed) |
| write admission | BeginWriteConjunctive (HOLD) | exhaustive | 32 | HOLD | none in domain | src/app/flow.rs:312-324 | CONTRACT | tests/app_writer_admission_test.rs (integration family; not executed) |

## Declared versus implicit extraction

- **SideStateConsistent** — Declared: Masks aggregate None/All/Partial; empty masks are None. Implicit: none beyond the explicitly stated finite-domain boundary.
- **SideStateChoiceMaskEquivalentIncludingEmpty** — Declared: named contract checked against the cited implementation Implicit: Choice-only All on an empty adopted side becomes None when masks are materialized.
- **SelectionPresenceResolvesEvenEmpty** — Declared: Some(selection), including zero bits, is resolved. Implicit: none beyond the explicitly stated finite-domain boundary.
- **ChildTriStateAggregation** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **AssembleAllFourOrdersAndProvenance** — Declared: Position order or whole-side concatenation; origin retained. Implicit: none beyond the explicitly stated finite-domain boundary.
- **AssembleBlockOrderStable** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **AssembleNoSyntheticMarkers** — Declared: Marker-free source/manual inputs receive no synthetic marker when resolved. Implicit: none beyond the explicitly stated finite-domain boundary.
- **AssembleNoMarkerLeak** — Declared: named contract checked against the cited implementation Implicit: Resolved is a decision flag, not a marker-free verdict; manual/source marker text survives.
- **UndecideOnlyOnDeselect** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **OrderApplicability** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **InvalidLineFailureIsNoOp** — Declared: named contract checked against the cited implementation Implicit: ensure_line_selection precedes set_line failure: false can still resolve an unresolved hunk.
- **MalformedSelectionNeverPanics** — Declared: named contract checked against the cited implementation Implicit: Block zip truncates; positional selection indexes beyond a shorter source. Shape must be a caller invariant.
- **ChoiceSelectionPreservesSourceChoices** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **RegionAssemblyPreservesOrder** — Declared: Passthrough and hunk regions preserve sequence and context provenance. Implicit: none beyond the explicitly stated finite-domain boundary.
- **ReadyImpliesNoMarkers** — Declared: Ready is a Result/raw choice and a false cached marker verdict. Implicit: No statement about stale cached verdicts.
- **ManualDivergenceIsTextual** — Declared: Identical text with different provenance is not divergent. Implicit: none beyond the explicitly stated finite-domain boundary.
- **EmptyResultIsAResolution** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **MissingResultOrModelNotDivergent** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **BlockersOrderedAndComplete** — Declared: All six produced categories in source order, not enum declaration order. Raw/deletion also get generic unresolved. Implicit: none beyond the explicitly stated finite-domain boundary.
- **IndexContextIdentityErrorsFailClosed** — Declared: Unreadable index and unknown/stale authority are Result errors, never empty lists. Implicit: none beyond the explicitly stated finite-domain boundary.
- **MessageReadErrorNeverFailOpen** — Declared: Missing message intentionally permits synthesized summary. Implicit: Every read/UTF-8 error also means not empty: message mechanism alone fails open.
- **MessageContentGate** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **UIBlockerPrecedence** — Declared: Missing binary dominates deletion, then generic; only then cached markers. Implicit: none beyond the explicitly stated finite-domain boundary.
- **DraftNeverAdoptsUnknownAuthority** — Declared: Only known identical identity authorizes adoption. Implicit: none beyond the explicitly stated finite-domain boundary.
- **DraftPayloadCompatibility** — Declared: Raw payload must be one of exact live side OID/mode pairs; text cannot carry raw result. Implicit: none beyond the explicitly stated finite-domain boundary.
- **LatestExplicitNullOutranksOldBody** — Declared: Latest record wins even when undone-to-None. Implicit: none beyond the explicitly stated finite-domain boundary.
- **NewestCompatibleRecoveryWins** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **UnknownOwnerCoalescenceIsFileOnly** — Declared: Unknown identity may coalesce only immediately previous same-editor FILE, never preserved history. Implicit: none beyond the explicitly stated finite-domain boundary.
- **OnlyPendingRecoveryOutranksDisk** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **DedupMovesLatestToTail** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **TwoWriterRecoveryRetained** — Declared: a stale unchanged local body must not deduplicate the only recovery of that body when the actual post-merge primary differs. Implicit: successful merge selection is supplied; disk adoption and assembled-hunk body/state overrides are covered, not the CAS/lock algorithm.
- **SkipClassificationNoFalseSuccess** — #1746 C6 requires observed movement while the operation remains active, including exit zero. Unchanged, absent and broken signals are Unclear and map to the existing Unknown warning. The `zero-exit-advanced` mutant restores the historical false-success policy.
- **BrokenSignalDominatesMovement** — Declared: A broken signal cannot be interpreted as progress. Implicit: none beyond the explicitly stated finite-domain boundary.
- **NextConflictIsSuccessOnObservedMovement** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **FinishedRequiresCleanRepository** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **NonzeroUnchangedIsNoProgress** — Declared: named contract checked against the cited implementation Implicit: none beyond the explicitly stated finite-domain boundary.
- **IntentAdmissionConjunctive** — Declared: Current token, full revision, whole Attachment and pane mutation admission must all match. Implicit: none beyond the explicitly stated finite-domain boundary.
- **PlanApprovalConjunctive** — Declared: Ready/current token-policy, attached identity, worktree and accepted conflict observation all required. Implicit: none beyond the explicitly stated finite-domain boundary.
- **BeginWriteConjunctive** — Declared: Current approved state/revision, no same-scope reconcile, no lease and successful reservation. Implicit: none beyond the explicitly stated finite-domain boundary.

## Broken-variant evidence

| model | removed/changed guard | exit | first red property | witness |
|---|---|---:|---|---|
| predict_hunks | both-positional | 1 | AssembleBlockOrderStable | {"actual":[["c0","c"],["i1","i"]],"events":["choose BI","uncheck side 1 line 0"],"expected":[["i1","i"],["c0","c"]],"lengths":[1,2]} |
| predict_hunks | empty-all | 1 | SideStateConsistent | {"choice":"C","lengths":[0,0],"selection":[[],[],"CF"],"side":0} |
| predict_hunks | undecide-on-select | 1 | UndecideOnlyOnDeselect | {"after":["U",null],"choice":"C","expected":["C",[[],[],"CF"]],"index":null,"lengths":[0,0],"selection":[[],[],"CF"],"side":0,"taken":true} |
| predict_hunks | order-without-tie | 1 | OrderApplicability | {"changed":true,"choice":"C","lengths":[0,0],"requested":"IF","selection":[[],[],"CF"]} |
| predict_hunks | resolved-choice-only | 1 | SelectionPresenceResolvesEvenEmpty | {"choice":"U","lengths":[0,0],"selection":[[],[],"CF"]} |
| predict_hunks | wrong-origin | 1 | AssembleAllFourOrdersAndProvenance | {"actual":[["i0","m"]],"choice":"C","expected":[["i0","i"]],"lengths":[0,1],"selection":[[],[true],"CF"]} |
| predict_hunks | emit-marker | 1 | AssembleAllFourOrdersAndProvenance | {"actual":[["=======","m"]],"choice":"C","expected":[],"lengths":[0,0],"selection":[[],[],"CF"]} |
| predict_hunks | reverse-regions | 1 | RegionAssemblyPreservesOrder | {"actual":[["c","c"],["context","x"]],"expected":[["context","x"],["c","c"]],"regions":[["context",["context"]],["hunk",[["c"],["i"],"C",null,"manual"]]]} |
| predict_ready | ignore-marker | 1 | ReadyImpliesNoMarkers | {"diverges":false,"marker":true,"model":null,"raw":true,"ready":true,"result":null} |
| predict_ready | compare-provenance | 1 | ManualDivergenceIsTextual | {"diverges":true,"marker":false,"model":[["a","i"]],"raw":false,"ready":true,"result":[["a","c"]]} |
| predict_ready | empty-is-missing | 1 | EmptyResultIsAResolution | {"diverges":false,"marker":false,"model":null,"raw":false,"ready":false,"result":[]} |
| predict_blockers | drop-raw | 1 | BlockersOrderedAndComplete | {"actual":["OK",[["unresolved-files",["0"]]]],"expected":["OK",[["unresolved-files",["0"]],["binary-unresolved",["0"]]]],"files":[["binary",false]],"merge":false,"message":"missing","residue":[],"unexpected":[]} |
| predict_blockers | ignore-index-error | 1 | IndexContextIdentityErrorsFailClosed | {"actual":["OK",[]],"conflict_iteration_ok":true,"context_known_equal":true,"files":[],"identities":[],"index_ok":false,"operation_equal":true} |
| predict_blockers | reverse-blockers | 1 | BlockersOrderedAndComplete | {"actual":["OK",[["empty-merge-message",[]],["index-unmerged",["outside"]]]],"expected":["OK",[["index-unmerged",["outside"]],["empty-merge-message",[]]]],"files":[],"merge":true,"message":"empty","residue":[],"unexpected":["outside"]} |
| predict_blockers | ui-ignore-markers | 1 | UIBlockerPrecedence | {"actual":null,"expected":"marker","files":[],"residue":["0"]} |
| predict_drafts | unknown-matches | 1 | DraftNeverAdoptsUnknownAuthority | {"after":{"binary":false,"draft_state":true,"hunk":null,"identity":null,"live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":[],"redo":true,"result":null,"sides":"same","undo":false},"current":{"binary":false,"draft_state":false,"hunk":null,"identity":null,"live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":[],"redo":false,"result":null,"sides":"same","undo":false},"saved":{"binary":false,"draft_state":false,"hunk":null,"identity":null,"live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":true,"result":null,"sides":"same","undo":false}} |
| predict_drafts | allow-other-raw | 1 | DraftPayloadCompatibility | {"compatible":true,"current":{"binary":false,"draft_state":false,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":true,"raw_result":null,"raw_sides":[],"redo":false,"result":null,"sides":"same","undo":false},"saved":{"binary":false,"draft_state":false,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":"C","raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false}} |
| predict_drafts | oldest-recovery | 1 | NewestCompatibleRecoveryWins | {"after":{"binary":false,"draft_state":true,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false},"current":{"binary":false,"draft_state":false,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false},"expected":{"binary":false,"draft_state":true,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":[],"sides":"same","undo":false},"history":[{"binary":false,"draft_state":true,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false},{"binary":false,"draft_state":true,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":[],"sides":"same","undo":false}]} |
| predict_drafts | null-not-state | 1 | LatestExplicitNullOutranksOldBody | {"after":{"binary":false,"draft_state":false,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false},"current_identity":"A","events":["old recovery body","latest explicit null record","retain_drafts"],"old":{"binary":false,"draft_state":false,"hunk":null,"identity":null,"live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false},"previous":{"binary":false,"draft_state":true,"hunk":null,"identity":"A","live":false,"origin":null,"pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":null,"sides":"same","undo":false}} |
| predict_drafts | coalesce-preserved | 1 | UnknownOwnerCoalescenceIsFileOnly | {"current":{"binary":false,"draft_state":true,"hunk":null,"identity":null,"live":true,"origin":"X","pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":["live"],"sides":"same","undo":false},"disk":{"binary":false,"draft_state":true,"hunk":null,"identity":null,"live":false,"origin":"X","pending":false,"raw":false,"raw_result":null,"raw_sides":["C","I"],"redo":false,"result":["disk"],"sides":"same","undo":false},"preserved":true,"serialized_history":[]} |
| predict_drafts | ignore-pending-recency | 1 | OnlyPendingRecoveryOutranksDisk | {"actual":[],"disk":[],"expected":[[["A"],true]],"local":[[["A"],true]]} |
| predict_drafts | pre-merge-recovery | 1 | TwoWriterRecoveryRetained | A and B save different target bodies; A's other-file save writes B primary but deletes A recovery by comparing it with A's unchanged local target |
| predict_skip | ignore-broken | 1 | BrokenSignalDominatesMovement | {"after":["A",0],"before":[null,0],"classified":"Unclear","events":["observe before","git <op> --skip returns nonzero","observe after","classify Unclear"],"in_progress":false,"ok":false,"signals":["Broken","Unchanged"],"unmerged":false} |
| predict_skip | nonzero-always-fails | 1 | NextConflictIsSuccessOnObservedMovement | {"after":[null,1],"before":[null,0],"classified":"NoProgress","events":["observe before","git <op> --skip returns nonzero","observe after","classify NoProgress"],"in_progress":false,"ok":false,"signals":["Absent","Advanced"],"unmerged":true} |
| predict_skip | exit-alone-finishes | 1 | FinishedRequiresCleanRepository | {"after":[null,null],"before":[null,null],"classified":"Finished","events":["observe before","git <op> --skip returns 0","observe after","classify Finished"],"in_progress":false,"ok":true,"signals":["Absent","Absent"],"unmerged":true} |
| predict_intent | intent-or | 1 | IntentAdmissionConjunctive | {"admitted":true,"current_owner":[0,0,"repo","worktree",0],"current_revision":null,"current_token":null,"frozen_owner":[0,0,"repo","worktree",0],"pane_admitted":true} |
| predict_intent | ignore-visit | 1 | IntentAdmissionConjunctive | {"admitted":true,"current_owner":[0,0,"repo","worktree",1],"current_revision":"A","current_token":"A","frozen_owner":[0,0,"repo","worktree",0],"pane_admitted":true} |
| predict_intent | ignore-pane | 1 | IntentAdmissionConjunctive | {"admitted":true,"current_owner":[0,0,"repo","worktree",0],"current_revision":"A","current_token":"A","frozen_owner":[0,0,"repo","worktree",0],"pane_admitted":false} |
| predict_intent | ignore-policy | 1 | PlanApprovalConjunctive | {"approved":true,"ready_token_policy_attached_attachment_worktree_observation":[true,true,false,true,true,true,true]} |
| predict_intent | ignore-reconcile | 1 | BeginWriteConjunctive | {"admitted":true,"approved_revision_reconcile_lease_reserve":[true,true,true,false,true]} |
| predict_intent | ignore-lease | 1 | BeginWriteConjunctive | {"admitted":true,"approved_revision_reconcile_lease_reserve":[true,true,false,true,true]} |

## Owner questions / mechanisms

- **Message error fail-open:** `conflicts.rs:921-923` returns false for *any* read error; missing-file fallback is explicitly justified at 918-919, but permission/I/O/invalid-UTF-8 is silently treated the same. This mechanism alone permits `Ok([])`; index open/refresh/conflict iteration and live-context/file identity instead fail closed (808-828,885-892; conflict_boundary.rs:53-77). Classify as QUESTION, not proof that final Git commit succeeds with unreadable metadata.
- **Skip success without movement (fixed by #1746 C6):** the historical zero-exit bypass returned Advanced for unchanged, absent or broken movement. The domain regression captured `left: Advanced, right: Unclear` before the fix. The current classifier requires observed movement and the UI mapping regression asserts Unknown plus the existing partially-applied footer. This is a predicate-level witness, not a claim that ordinary Git produces a successful no-op skip.
- **Empty-side visual asymmetry:** choose an empty Current side → choice-derived tri-state All → materialize masks (even via an invalid line edit) → empty mask tri-state None, while `is_resolved` remains true. Is adopted-empty All vs mask-empty None an intended UI contract?
- **False-return mutation:** unresolved hunk without masks → call set_hunk_line(side,index=len,taken=true) → seed masks → out-of-range set_line returns false → hunk now reports resolved with no selected bits. This is an API/caller-precondition QUESTION; no claim that ordinary UI generates invalid indices.
- **Resolved marker leak:** choose Manual("<<<<<<< user") or accept source containing a marker → `is_resolved=true`, assembly retains marker. Separate cached/fresh marker gates protect readiness/Save/Continue; decision completion must not be equated with marker freedom.
- **Malformed vector shape:** Current empty, Incoming nonempty, Current taken bit at position zero; positional order indexes empty Current and panics, while block zip truncates. Shape construction appears guarded by length-based constructors; QUESTION for public mutation/deserialization boundaries, not an asserted reachable UI panic.

Rust tests listed were located/read or identified by their declaration; none were run. No cargo, fixture, GUI or repository mutation was used.
