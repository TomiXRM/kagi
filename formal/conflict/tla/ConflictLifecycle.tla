------------------------ MODULE ConflictLifecycle ------------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC
CONSTANTS Scenario, Broken
VARIABLE s
vars == <<s>>
Actors == {1, 2}
Files == {1, 2}
DraftScenario == Scenario \in {"manual", "draftCrash", "draftIO", "draftAtomic", "draftRace", "draftRaceLost"}
Concurrent == Scenario \in {"draftRace", "draftRaceLost", "saveRace"}
Participants == IF Concurrent THEN Actors ELSE {1}
Kind == CASE Scenario \in {"skip", "skipRefresh", "preserveSkip", "receiptSkip"} -> "skip"
          [] Scenario \in {"preserveAbort", "sibling"} -> "abort"
          [] Scenario = "stashAbort" -> "stashAbort"
          [] Scenario = "preserveDF" -> "df"
          [] Scenario \in {"receiptContinue", "liveness", "metadataUnreadable"} -> "continue"
          [] Scenario = "receiptMerge" -> "mergeContinue"
          [] Scenario = "merge" -> "mergeCommit"
          [] OTHER -> "save"
DeferredReceipt == Kind \in {"skip", "continue", "mergeContinue"}
File(a) == IF Scenario \in {"draftRace", "draftRaceLost"} THEN a ELSE 1
Revision == <<s.indexRev, s.headRev, s.metadataRev, s.operation>>
Record(a) == <<File(a), s.context[a], s.text[a]>>
At(a, p) == a \in Participants /\ s.life[a] = "open" /\ s.pc[a] = p
ActorInit == [a \in Actors |-> 0]
Init == s = [pc |-> [a \in Actors |-> "observeIndex"],
    life |-> [a \in Actors |-> "open"], attached |-> [a \in Actors |-> TRUE],
    indexRev |-> 0, headRev |-> 0, metadataRev |-> 0, operation |-> 0,
    wt |-> [f \in Files |-> IF Kind = "mergeCommit" THEN "shown" ELSE IF Kind = "df" /\ f = 2 THEN "child" ELSE "markers"],
    index |-> [f \in Files |-> IF Kind = "mergeCommit" THEN "shown" ELSE IF Kind = "df" /\ f = 2 THEN "child" ELSE IF f = 1 \/ Scenario \in {"draftRace","draftRaceLost"} THEN "unmerged" ELSE "absent"],
    observedIndex |-> ActorInit, observation |-> [a \in Actors |-> <<0,0,0,0>>],
    context |-> ActorInit, text |-> [a \in Actors |-> "none"],
    materializedWT |-> [a \in Actors |-> "none"],
    captured |-> {}, disk |-> {}, diskValid |-> TRUE,
    pending |-> [a \in Actors |-> {}], committed |-> {}, restored |-> {},
    readCorrupt |-> FALSE, restarted |-> [a \in Actors |-> FALSE],
    draftPersistFailed |-> [a \in Actors |-> FALSE],
    closeSheet |-> [a \in Actors |-> FALSE],
    frozen |-> [a \in Actors |-> "none"], shownTree |-> [a \in Actors |-> "unmerged"],
    planTree |-> [a \in Actors |-> "unmerged"], planRev |-> [a \in Actors |-> <<0,0,0,0>>],
    approvedStep |-> ActorInit, approved |-> [a \in Actors |-> FALSE],
    lease |-> [a \in Actors |-> FALSE], preflight |-> [a \in Actors |-> FALSE],
    draftOwner |-> 0,
    recoveryStep |-> 0, recoveryPrimary |-> "none", recoveryHistory |-> {},
    executed |-> [a \in Actors |-> "none"], successful |-> [a \in Actors |-> FALSE],
    receipt |-> [a \in Actors |-> FALSE], released |-> [a \in Actors |-> FALSE],
    reported |-> [a \in Actors |-> FALSE], mutated |-> [a \in Actors |-> FALSE],
    wroteUnapproved |-> FALSE, wroteStale |-> FALSE, wrongStep |-> FALSE, stagedDifferent |-> FALSE,
    preservationFailed |-> [a \in Actors |-> FALSE], destructiveAfterFailure |-> FALSE,
    sharedMoved |-> FALSE, siblingInvalidated |-> FALSE,
    external |-> FALSE, committedTree |-> "none", abandoned |-> FALSE,
    refreshPending |-> FALSE, metadataReadable |-> TRUE, metadataFailOpen |-> FALSE,
    barrier |-> [generation |-> 0, durable |-> 0, ticket |-> 0,
        flushed |-> 0, phase |-> "edit", failed |-> FALSE,
        draftLock |-> FALSE, repoLock |-> FALSE,
        badApproval |-> FALSE, badClose |-> FALSE, badGeneration |-> FALSE],
    event |-> "Init"]

\* backend/conflict_observe.rs:91-101: index.read(true) precedes operation_parts.
ObserveIndex(a) == /\ At(a, "observeIndex")
    /\ s' = [s EXCEPT !.observedIndex[a] = s.indexRev,
        !.pc[a] = "observeMetadata", !.event = <<"ObserveIndex",a>>]
\* backend/conflict_observe.rs:148-198: HEAD + operation metadata, no WT bytes.
ObserveMetadata(a) == /\ At(a, "observeMetadata")
    /\ s' = [s EXCEPT !.observation[a] = <<s.observedIndex[a],s.headRev,s.metadataRev,s.operation>>,
        !.context[a] = s.operation, !.pc[a] = "accept", !.event = <<"ObserveMetadata",a>>]
\* app/conflict.rs:175-188; ui/tab_view_access.rs:310-324: attached, not InFlight.
AcceptRead(a) == /\ At(a, "accept") /\ s.attached[a] /\ ~s.lease[a]
    \* commit.rs:1054-1065 resolved merge is already displaying the commit panel.
    /\ s' = [s EXCEPT !.pc[a] = IF Kind = "mergeCommit" THEN "ready" ELSE "openFile", !.event = <<"AcceptRead",a>>]
\* ui/conflict_view/flow.rs:28-31: captures input before changing editing path.
\* #1733 resolution.rs:295-301 captures WT provenance when the live file is built;
\* OpenFile is the finite materialization boundary (external edits occur later).
OpenFile(a) == /\ At(a, "openFile")
    /\ s' = [s EXCEPT !.materializedWT[a] = s.wt[File(a)],
        !.pc[a] = "edit", !.event = <<"OpenFile",a,File(a)>>]
\* flow.rs:42-50,90-95 and actions.rs:129-145: idle, no divergence, autosave.
\* One hunk and one non-repeated source adoption are the finite edit quotient.
Choose(a) == /\ At(a, "edit") /\ ~s.lease[a] /\ ~DraftScenario /\ Kind # "df"
    /\ s' = [s EXCEPT !.text[a] = "chosen", !.pc[a] = "autosaveRead",
        !.captured = @ \cup {<<File(a),s.context[a],"chosen">>}, !.event = <<"Choose",a>>]
\* actions.rs:34-79,129-145: same writer/manual guards and persistence as Choose.
LineToggle(a) == /\ At(a, "edit") /\ ~s.lease[a] /\ ~DraftScenario /\ Kind # "df"
    /\ s' = [s EXCEPT !.text[a] = "lines", !.pc[a] = "autosaveRead",
        !.captured = @ \cup {<<File(a),s.context[a],"lines">>}, !.event = <<"LineToggle",a>>]
\* ui/conflict_view/special.rs:125-129: D/F is a namespace decision, no text/hunks.
ChooseNamespace(a) == /\ At(a, "edit") /\ Kind = "df" /\ ~s.lease[a]
    /\ s' = [s EXCEPT !.text[a] = "file", !.pc[a] = "ready", !.event = <<"ChooseNamespace",a>>]
\* #1732 draft_writer.rs: queue_autosave captures immediately, not durably.
\* Only the completed autosave/flush acknowledges the primary rename.
ManualEdit(a) == /\ At(a, "edit") /\ ~s.lease[a] /\ DraftScenario
    /\ s' = [s EXCEPT !.text[a] = "manual", !.pc[a] = IF Broken = "capture" THEN "ready" ELSE "autosaveRead",
        !.captured = @ \cup {<<File(a),s.context[a],"manual">>}, !.event = <<"ManualEdit",a>>]
\* #1732 draft_storage.rs::save_draft_document: acquire the OS sidecar lock
\* before the checked content-CAS read/merge. This read observes the latest
\* complete document, including any commit since this editor's original load.
AutosaveRead(a) == /\ At(a, "autosaveRead")
    /\ (Broken = "draftLock" \/ s.draftOwner \in {0, a})
    \* Schedule restriction isolates stale overwrite from the shorter torn-read bug.
    /\ (Scenario # "draftRaceLost" \/ s.diskValid)
    /\ s' = [s EXCEPT !.pending[a] = (IF s.diskValid THEN s.disk ELSE {}) \cup {Record(a)},
        !.draftOwner = IF Broken = "draftLock" THEN @ ELSE a,
        !.readCorrupt = @ \/ ~s.diskValid, !.pc[a] = "autosaveStart", !.event = <<"AutosaveRead",a>>]
\* #1732 draft_storage.rs::write_atomic: same-directory temp, sync_all, rename.
\* Until rename, neither a reader nor process death can damage the old primary.
\* Broken="draftAtomic" restores the old truncating destination write.
AutosaveWriteStart(a) == /\ At(a, "autosaveStart")
    /\ s' = [s EXCEPT !.diskValid = IF Broken = "draftAtomic" THEN FALSE ELSE @,
        !.disk = IF Broken = "draftAtomic" THEN {} ELSE @,
        !.pc[a] = "autosaveFinish", !.event = <<"AutosaveWriteStart(temp)",a>>]
\* resolution.rs::autosave: mark_saved only after draft_storage's successful rename.
AutosaveWrite(a) == /\ At(a, "autosaveFinish")
    /\ s' = [s EXCEPT !.diskValid = TRUE, !.disk = s.pending[a],
        !.draftOwner = IF s.draftOwner = a THEN 0 ELSE @,
        !.committed = @ \cup {Record(a)}, !.draftPersistFailed[a] = FALSE,
        !.closeSheet[a] = FALSE, !.pc[a] = "ready", !.event = <<"AutosaveWrite(rename)",a>>]
\* #1731 draft_owner.rs: note_draft_result retains failure until autosave succeeds.
AutosaveFailure(a) == /\ Scenario = "draftIO" /\ At(a, "autosaveStart")
    /\ s' = [s EXCEPT !.draftPersistFailed[a] = TRUE,
        !.pc[a] = "ready", !.draftOwner = 0, !.event = <<"AutosaveFailure",a>>]
\* #1731 host_close.rs: hold_host_close/save_host_dirty_guard retain live text.
\* This no-discard safety scenario excludes a separately approved intentional
\* discard; the native conflict_draft_persist_failure test checks its two steps.
OpenCloseSheet(a) == /\ Scenario = "draftIO" /\ At(a, "ready")
    /\ s.draftPersistFailed[a] /\ ~s.closeSheet[a] /\ Broken # "draftClose"
    /\ s' = [s EXCEPT !.closeSheet[a] = TRUE, !.event = <<"OpenCloseSheet",a>>]
RetryDraft(a) == /\ At(a, "ready") /\ s.closeSheet[a] /\ s.draftPersistFailed[a]
    /\ s' = [s EXCEPT !.pc[a] = "autosaveRead", !.event = <<"RetryDraft",a>>]
CancelCloseSheet(a) == /\ At(a, "ready") /\ s.closeSheet[a]
    /\ s' = [s EXCEPT !.closeSheet[a] = FALSE, !.event = <<"CancelCloseSheet",a>>]
\* backend/conflict_ops.rs:195-218,244-288: revision/payload-bound immutable plan.
\* merge panel's shown staged set: ui/operations/commit.rs:1054-1065.
Freeze(a) == /\ At(a, "ready") /\ ~DraftScenario /\ s.attached[a]
    /\ (Kind = "skip" \/ s.observation[a] = Revision)
    /\ s' = [s EXCEPT !.frozen[a] = s.text[a], !.planRev[a] = s.observation[a],
        !.shownTree[a] = s.index[1], !.planTree[a] = s.index[1],
        !.approvedStep[a] = s.operation, !.pc[a] = "approve", !.event = <<"Freeze(plan)",a>>]
\* app/flow.rs:261-299,312-343: confirmation binds token and cloned payload,
\* per-Sessions lease (session.rs:278); ui/mod.rs:1601 creates separate instances.
\* Legacy routes abstract their explicit confirmation + reserve_write boundary:
\* ui/operations/conflict.rs:374-384,598-631 (Continue),
\* ui/conflict_view/actions.rs:432-441 and operations/conflict_skip.rs:40-45,68-87.
\* MergeCommit is synchronous at ui/operations/commit.rs:1068-1076,1191-1209;
\* its lease bit represents the busy synchronous UI interval, not a new backend lease.
\* Broken="approval" bypasses confirmation, not the later preflight.
Approve(a) == /\ At(a, "approve") /\ s.attached[a] /\ ~s.lease[a]
    /\ ~s.refreshPending
    /\ s' = [s EXCEPT !.approved[a] = Broken # "approval", !.lease[a] = TRUE,
        !.pc[a] = "preflight", !.event = <<"Approve(confirm)",a>>]
\* ui/operations/commit.rs:1191-1209: silently renews shown merge plan at click.
RenewMergePlan(a) == /\ At(a, "preflight") /\ Kind = "mergeCommit"
    /\ s' = [s EXCEPT !.planTree[a] = s.index[1], !.planRev[a] = Revision,
        !.pc[a] = "mergePreflight", !.event = <<"RenewMergePlan",a>>]
\* conflict_ops.rs:419-425 and conflict_boundary.rs:170-181: full revision check.
\* Per-file index/WT provenance is checked again by ValidateFile before WTWrite.
\* #1746: conflicts.rs::preflight_skip_revision validates the frozen full revision.
\* Commit index checked against the NEW plan (ops/commit.rs:38-44,74-83).
Preflight(a) == /\ At(a, IF Kind = "mergeCommit" THEN "mergePreflight" ELSE "preflight")
    /\ (s.metadataReadable \/ Broken = "metadata")
    /\ (s.planRev[a] = Revision \/ Broken = "revision"
        \/ (Kind = "skip" /\ Broken = "skipRevision"))
    /\ (Kind # "mergeCommit" \/ s.planTree[a] = s.index[1])
    /\ s' = [s EXCEPT !.preflight[a] = TRUE,
        !.metadataFailOpen = @ \/ (~s.metadataReadable /\ Kind = "continue"),
        !.pc[a] = CASE Kind \in {"skip","abort","stashAbort","df"} -> "preserve"
                  [] Kind = "mergeCommit" -> "commit" [] OTHER -> "wtValidate",
        !.event = <<"Preflight",a>>]
\* #1733 conflict_boundary.rs::validate_save_binding / validate_current_paths;
\* worktree_state.rs::validate accepts only the original WT or exact frozen bytes.
\* fresh file identity + captured WT hash immediately before the executor write.
\* This is a separate read transition, NOT an atomic guard-and-write assumption.
ValidateFile(a) == /\ At(a, "wtValidate")
    /\ (s.planRev[a] = Revision \/ Broken = "revision")
    /\ (s.materializedWT[a] = s.wt[File(a)] \/ s.frozen[a] = s.wt[File(a)] \/ Broken = "worktree")
    /\ s' = [s EXCEPT !.pc[a] = "wt", !.event = <<"ValidateFile(index,WT)",a>>]
FileRefused(a) == /\ At(a, "wtValidate")
    /\ ((s.planRev[a] # Revision /\ Broken # "revision") \/
        (s.materializedWT[a] # s.wt[File(a)] /\ s.frozen[a] # s.wt[File(a)] /\ Broken # "worktree"))
    /\ s' = [s EXCEPT !.lease[a] = FALSE, !.pc[a] = "done",
        !.event = <<"FileRefused",a>>]
\* #1731 conflict_abort.rs: execute_conflict_abort_expecting, conflicts.rs:
\* execute_conflict_skip, dir_file_conflict.rs: apply_dir_file_resolution_with_progress.
\* Required recovery failure returns Blocked before destructive repository work.
Preserve(a) == /\ At(a, "preserve")
    /\ s' = [s EXCEPT !.preservationFailed[a] = Scenario \in {"preserveAbort","preserveSkip","preserveDF"},
        \* A possible fs::write error AFTER truncation destroys even an older save.
        \* resolution.rs:800-806; atomic draft replacement is tracked by #1732.
        !.diskValid = IF Scenario \in {"preserveAbort","preserveSkip"} THEN FALSE ELSE @,
        !.disk = IF Scenario \in {"preserveAbort","preserveSkip"} THEN {} ELSE @,
        !.pc[a] = CASE Scenario \in {"preserveAbort","preserveSkip","preserveDF"}
                          /\ Broken # "preservation" -> "refused"
                  [] Kind = "skip" -> "skip"
                  [] Kind \in {"abort","stashAbort"} -> "abort" [] OTHER -> "index",
        !.event = <<"Preserve",a,IF Scenario \in {"preserveAbort","preserveSkip","preserveDF"} THEN "failed" ELSE "ok">>]
\* backend/conflict_ops.rs:562-581 and UI conflict_skip.rs:138-152 map errors
\* to Refused; neither admission settlement nor presentation mutates the repo.
RefusePreservation(a) == /\ At(a, "refused") /\ s.preservationFailed[a]
    /\ s' = [s EXCEPT !.lease[a] = FALSE, !.receipt[a] = TRUE,
        !.reported[a] = TRUE, !.pc[a] = "done", !.event = <<"RefusePreservation",a>>]
\* conflicts.rs:1646-1648 Save; :1380-1404 Continue rename; dir_file_conflict.rs:315-324.
\* Abort index precedes checkout (conflict_abort.rs:342-361); non-UTF8 stash path
\* omitted at :513-517 and empty checkout is no-op at :582-583.
WTWrite(a) == \E payload \in {s.frozen[a], "tampered"} :
    /\ At(a, "wt") /\ s.preflight[a]
    \* conflict_boundary.rs:103-107 binding guard; mutation removes this guard.
    /\ (payload = s.frozen[a] \/ Broken = "payload")
    /\ s' = [s EXCEPT !.wt[1] = IF Kind = "stashAbort" THEN @ ELSE IF Kind = "abort" THEN "original" ELSE payload,
        !.wt[2] = IF Kind = "df" THEN "absent" ELSE @,
        !.executed[a] = payload, !.mutated[a] = TRUE,
        !.wroteUnapproved = @ \/ ~s.approved[a],
        !.wroteStale = @ \/ (Kind \in {"save","continue","mergeContinue"} /\
            (s.planRev[a] # Revision \/ s.wt[1] = "external")),
        !.destructiveAfterFailure = @ \/ s.preservationFailed[a],
        !.pc[a] = CASE Kind = "abort" -> "ref" [] Kind \in {"stashAbort","df"} -> "verify" [] OTHER -> "index",
        !.event = <<"WTWrite",a>>]
\* conflicts.rs:1650-1656 and :1406-1420; conflict_abort.rs:342-351,519-531;
\* dir_file_conflict.rs:219-222. Each index flush is atomic, not whole lifecycle.
IndexWrite(a) == /\ At(a, "index") /\ s.preflight[a]
    \* conflicts.rs:1651,1407-1409: add_path reads CURRENT WT, not frozen bytes.
    /\ s' = [s EXCEPT !.index[1] = CASE Kind = "stashAbort" -> "absent" [] Kind = "abort" -> "original" [] Kind = "df" -> s.frozen[a] [] OTHER -> s.wt[1],
        !.stagedDifferent = @ \/ (Kind \in {"save","continue","mergeContinue"} /\ s.wt[1] # s.frozen[a]),
        !.index[2] = IF Kind = "df" THEN "absent" ELSE @,
        !.indexRev = @ + 1, !.mutated[a] = TRUE, !.wroteUnapproved = @ \/ ~s.approved[a],
        !.destructiveAfterFailure = @ \/ s.preservationFailed[a],
        !.pc[a] = CASE Kind \in {"abort","stashAbort","df"} -> "wt"
                  [] Kind \in {"continue","mergeContinue"} -> "continue" [] OTHER -> "verify",
        !.event = <<"IndexWrite",a>>]
\* conflict_abort.rs:273-286,342-351: original target + guard already preflighted.
Abort(a) == /\ At(a, "abort") /\ Kind \in {"abort","stashAbort"}
    /\ s' = [s EXCEPT !.pc[a] = "index", !.event = <<"Abort",a>>]
\* conflict_abort.rs:377-389: restore ref compare-and-swap, not force update.
RefCAS(a) == /\ At(a, "ref") /\ s.headRev = s.planRev[a][2]
    /\ s' = [s EXCEPT !.headRev = @ + 1, !.sharedMoved = TRUE,
        !.pc[a] = "verify", !.event = <<"RefCAS",a>>]
\* #1746: execute_conflict_skip rechecks the same full approval immediately before run_git.
\* Broken="skipRevision" removes both plan-time and final expected-revision checks.
Skip(a) == /\ At(a, "skip")
    /\ ((s.planRev[a] = Revision /\ s.operation = s.approvedStep[a])
        \/ Broken = "skipRevision")
    /\ s' = [s EXCEPT !.wrongStep = @ \/ s.operation # s.approvedStep[a],
        !.operation = @ + 1, !.mutated[a] = TRUE, !.wroteUnapproved = @ \/ ~s.approved[a],
        !.destructiveAfterFailure = @ \/ s.preservationFailed[a],
        !.pc[a] = "verify", !.event = <<"Skip",a,s.operation>>]
\* conflicts.rs:1145,1158-1167,1195-1245; UI merge route :404-408 stages only.
\* One successful CLI iteration represents bounded nonconflicting retries.
Continue(a) == /\ At(a, "continue")
    /\ s' = [s EXCEPT !.operation = IF Kind = "continue" THEN @ + 1 ELSE @,
        !.pc[a] = "verify", !.event = <<"Continue",a,Kind>>]
\* ops/commit.rs:74-83,334-351 uses checked private index, not subsequent disk.
MergeCommit(a) == /\ At(a, "commit")
    /\ s' = [s EXCEPT !.committedTree = s.planTree[a], !.executed[a] = s.planTree[a],
        !.mutated[a] = TRUE, !.wroteUnapproved = @ \/ ~s.approved[a],
        !.pc[a] = "verify", !.event = <<"MergeCommit",a>>]
\* conflict_observe.rs:348-395 checks save WT/index; :443-467 abort only HEAD/op.
\* Continue/Skip measured summaries: conflicts.rs:1234-1245,1898-1906.
Verify(a) == /\ At(a, "verify")
    /\ (Kind # "save" \/ (s.wt[1] = s.frozen[a] /\ s.index[1] = s.frozen[a]))
    /\ s' = [s EXCEPT !.successful[a] = TRUE,
        !.pc[a] = IF DeferredReceipt THEN "release" ELSE "append",
        !.event = <<"Verify",a>>]
\* conflict_ops.rs:581-603 backend records before returning; Continue/Skip UI
\* callbacks append later (ui/operations/conflict.rs:659,674-698; conflict_skip.rs:107,149).
\* Merge Continue success only logs :444-448, with no append path.
AppendReceipt(a) == /\ At(a, "append") /\ Kind # "mergeContinue"
    /\ s' = [s EXCEPT !.receipt[a] = TRUE,
        !.pc[a] = IF DeferredReceipt THEN "deliver" ELSE "release",
        !.event = <<"AppendReceipt",a>>]
\* app/session.rs:661-669,760-776; app/settle.rs:147-149. No durable-receipt
\* requirement for deferred writers. Broken="receipt" removes Save ordering.
ReleaseLease(a) == /\ (At(a, "release") \/ (Broken = "receipt" /\ At(a, "append")))
    /\ s' = [s EXCEPT !.lease[a] = FALSE, !.released[a] = TRUE,
        !.pc[a] = IF DeferredReceipt /\ Kind # "mergeContinue" THEN "append" ELSE "deliver",
        !.event = <<"ReleaseLease",a>>]
\* app/settle.rs:222-244,272-300; flow.rs:78 says Conflict changes_shared_refs=false.
\* No sibling invalidation even if an Abort moved the shared branch.
Deliver(a) == /\ At(a, "deliver")
    /\ s' = [s EXCEPT !.reported[a] = Kind # "mergeContinue", !.pc[a] = "done",
        !.event = <<"Deliver",a>>]
\* ui/tabs.rs:231-251; app/flow.rs:287-290: departure invalidates pending approval.
\* Boundary restriction: departure once, after plan or after admission.
Depart(a) == /\ a \in Participants /\ s.life[a] = "open" /\ s.attached[a]
    /\ s.pc[a] \in {"approve","preflight","append","deliver"}
    /\ s' = [s EXCEPT !.attached[a] = FALSE, !.event = <<"Depart(tab switch)",a>>]
\* #1731 host_close.rs: failed conflict drafts join the existing host dirty gate.
\* Broken="draftClose" removes only this new hold, leaving the lease gate intact.
GracefulExit(a, reason) == /\ a \in Participants /\ s.life[a] = "open" /\ ~s.lease[a]
    /\ s.pc[a] \in {"ready","append","deliver","done"}
    /\ (~s.draftPersistFailed[a] \/ Broken = "draftClose")
    /\ s' = [s EXCEPT !.life[a] = "closed", !.attached[a] = FALSE,
        !.text[a] = "none", !.abandoned = @ \/ (s.successful[a] /\ ~s.receipt[a]),
        !.event = <<reason,a>>]
CloseWindow(a) == GracefulExit(a, "CloseWindow")
\* ui/mod.rs:3661-3669 flushes settings/issue drafts, NOT conflict documents.
Quit(a) == GracefulExit(a, "Quit")
\* #1732: a crash closes the draft lock fd; the kernel releases its advisory lock.
\* Only draft crash scenarios allow abrupt exit; liveness uses graceful close.
Crash(a) == /\ Scenario \in {"draftCrash","draftAtomic"} /\ a = 1
    /\ s.life[a] = "open" /\ s.pc[a] \in {"autosaveRead","autosaveStart","autosaveFinish","ready"}
    /\ s' = [s EXCEPT !.life[a] = "crashed", !.text[a] = "none", !.lease[a] = FALSE,
        !.draftOwner = IF s.draftOwner = a THEN 0 ELSE @,
        !.event = <<"Crash",a>>]
\* #1732 draft_storage.rs::read: invalid primary recovers verified .prev or errors;
\* conflict_detect.rs:78-84 rebuilds sides, restores compatible same-step draft.
Restart(a) == /\ DraftScenario /\ a \in Participants /\ s.life[a] # "open" /\ ~s.restarted[a]
    /\ s' = [s EXCEPT !.life[a] = "open", !.restarted[a] = TRUE, !.pc[a] = "done",
        !.restored = @ \cup (IF s.diskValid THEN {r \in s.disk : r[2] = s.operation} ELSE {}),
        !.readCorrupt = @ \/ ~s.diskValid, !.event = <<"Restart(restore draft)",a>>]
\* Environmental CLI writes: the accepted observation remains index/metadata-only;
\* #1733 additionally freezes each live draft's WT bytes at materialization.
\* One external edit is the finite interference bound, not a production guard.
ExternalGit == /\ ~s.external /\ s.life[1] = "open"
    /\ CASE Scenario \in {"staleWT", "staleWTBeforeChoice"} ->
          /\ s.pc[1] = IF Scenario = "staleWTBeforeChoice" THEN "edit" ELSE "preflight"
          /\ s' = [s EXCEPT !.wt[1] = "external", !.external = TRUE,
              !.refreshPending = Scenario = "staleWTBeforeChoice",
              !.event = "ExternalGit(worktree)"]
       [] Scenario \in {"staleIndex","staleHead","staleMeta","merge"} ->
          /\ s.pc[1] = "preflight"
          /\ s' = [s EXCEPT !.indexRev = IF Scenario \in {"staleIndex","merge"} THEN @ + 1 ELSE @,
              !.index[2] = IF Scenario \in {"staleIndex","merge"} THEN "external" ELSE @,
              !.index[1] = IF Scenario = "merge" THEN "external" ELSE @,
              !.headRev = IF Scenario = "staleHead" THEN @ + 1 ELSE @,
              !.metadataRev = IF Scenario = "staleMeta" THEN @ + 1 ELSE @,
              !.external = TRUE, !.event = <<"ExternalGit",Scenario>>]
       [] Scenario = "raceIndex" ->
          /\ s.pc[1] = "wt"
          /\ s' = [s EXCEPT !.indexRev = @ + 1, !.index[2] = "external",
              !.external = TRUE, !.event = "ExternalGit(index after final preflight)"]
       [] Scenario = "stageWT" ->
          /\ s.pc[1] = "index"
          /\ s' = [s EXCEPT !.wt[1] = "external", !.external = TRUE,
              !.event = "ExternalGit(worktree before add_path)"]
       [] OTHER -> FALSE
\* drafts.rs::retain_drafts keeps WT provenance even for untouched files.
\* Removing that carry reproduces drift + watcher refresh before the first choice.
WatcherFileRefresh == /\ Scenario = "staleWTBeforeChoice" /\ s.refreshPending
    /\ s' = [s EXCEPT !.refreshPending = FALSE,
        !.materializedWT[1] = IF Broken = "materialization" THEN s.wt[1] ELSE @,
        !.event = "WatcherFileRefresh(retain original WT provenance)"]
\* conflicts.rs:1893-1894 live position read allows CLI to replace approved step.
\* Both revision and draft context change when REBASE_HEAD/step change:
\* conflict_observe.rs:155-176; drafts.rs:176-247 disallows fresh-context adoption.
ReplaceOperation == /\ Scenario \in {"skip", "skipRefresh"} /\ ~s.external
    /\ (IF Scenario = "skipRefresh" THEN s.pc[1] = "approve"
        ELSE s.pc[1] \in {"preflight", "preserve", "skip"})
    /\ s' = [s EXCEPT !.operation = @ + 1, !.metadataRev = @ + 1,
        !.refreshPending = Scenario = "skipRefresh",
        !.external = TRUE, !.event = "ReplaceOperation(next rebase step)"]
\* Refresh-before-confirm schedule from DrillSequencers, Skip controls.
\* actions.rs::skip_request_arm freezes mode.revision in mode.skip_approval;
\* conflict_detect.rs replaces a changed mode with skip_approval=None.
\* reset_all_target (actions.rs:134 etc.) is a separate Reset-All arm, not Skip.
\* Freeze represents the first arm; returning to ready requires a fresh arm.
WatcherRefresh == /\ s.refreshPending /\ s.pc[1] = "approve"
    /\ s' = [s EXCEPT !.refreshPending = FALSE,
        !.pc[1] = IF Broken = "skipReset" THEN "approve" ELSE "ready",
        !.observation[1] = Revision, !.context[1] = s.operation,
        !.event = "WatcherRefresh(disarm changed revision)"]
\* C5: approval_metadata.rs:15-21,42-54,111,139 propagates MERGE_MSG read errors.
\* conflicts.rs:1266,1292 checks observation BEFORE any staging/CLI mutation.
MetadataReadFailure == /\ Scenario = "metadataUnreadable" /\ ~s.external
    /\ s.pc[1] = "preflight"
    /\ s' = [s EXCEPT !.metadataReadable = FALSE, !.external = TRUE,
        !.event = "MetadataReadFailure(MERGE_MSG unreadable)"]
MetadataRefused(a) == /\ At(a, "preflight") /\ ~s.metadataReadable /\ Broken # "metadata"
    /\ s' = [s EXCEPT !.lease[a] = FALSE, !.pc[a] = "done",
        !.event = <<"MetadataRefused",a>>]

\* #1732 drafts.rs::autosave_json + draft_merge.rs::matches_record.
\* Quotient four successful, lock-serialized transactions after both editors
\* loaded the same empty baseline. A and B collide on file 1; A then saves
\* file 2 twice without changing its stale in-memory file-1 body.
\* Disk includes both the primary and inspection-only recovery bodies.
RecoveryA == <<1, 0, "A">>
RecoveryB == <<1, 0, "B">>
RecoveryOther == <<2, 0, "other">>
DraftRecoveryNext ==
    /\ Scenario = "draftRecovery"
    /\ CASE s.recoveryStep = 0 ->
          s' = [s EXCEPT !.recoveryPrimary = RecoveryA, !.disk = {RecoveryA},
              !.committed = {RecoveryA}, !.recoveryStep = 1,
              !.event = "RecoverySave(A primary)"]
       [] s.recoveryStep = 1 ->
          s' = [s EXCEPT !.recoveryPrimary = RecoveryB,
              !.recoveryHistory = {RecoveryA}, !.disk = {RecoveryA, RecoveryB},
              !.committed = {RecoveryA, RecoveryB}, !.recoveryStep = 2,
              !.event = "RecoverySave(B primary, A recovery)"]
       [] s.recoveryStep \in 2..3 ->
          LET compared == IF Broken = "preMergeRecovery"
                          THEN RecoveryA ELSE s.recoveryPrimary
              kept == {r \in s.recoveryHistory : r # compared}
          IN s' = [s EXCEPT !.recoveryHistory = kept,
              !.disk = kept \cup {s.recoveryPrimary, RecoveryOther},
              !.committed = @ \cup {RecoveryOther}, !.recoveryStep = @ + 1,
              !.event = "RecoverySave(A other file, adopt B primary, dedup recoveries)"]
       [] OTHER -> FALSE

\* #1760: draft_owner.rs::{autosave_drafts,flush_drafts} and
\* operations/conflict.rs::flush_conflict_drafts. Two captured generations
\* quotient edits before/during an off-thread barrier; accepted != durable.
\* No discard action: explicit two-step intentional loss is outside this safety
\* assertion, as in draftIO. Draft lock release precedes returned flush success.
BarrierEdit == /\ s.barrier.generation < 2 /\ ~s.barrier.repoLock
    /\ s' = [s EXCEPT !.barrier.generation = @ + 1,
        !.event = "QueueAutosave(acceptance only)"]
BarrierRequest == /\ s.barrier.phase = "edit" /\ s.barrier.generation > 0
    /\ s' = [s EXCEPT !.barrier.ticket = s.barrier.generation,
        !.barrier.phase = "waiting", !.event = "FlushOffThread"]
BarrierWriteStart == /\ s.barrier.phase = "waiting" /\ ~s.barrier.draftLock
    /\ s' = [s EXCEPT !.barrier.draftLock = TRUE, !.barrier.phase = "writing",
        !.event = "DraftLockAcquire"]
BarrierWriteFinish == /\ s.barrier.phase = "writing"
    /\ \E ok \in BOOLEAN :
        s' = [s EXCEPT !.barrier.draftLock = FALSE,
            !.barrier.durable = IF ok THEN s.barrier.ticket ELSE @,
            !.barrier.failed = ~ok, !.barrier.phase = "returned",
            !.event = IF ok THEN "FlushOk(lock released)" ELSE "FlushErr"]
BarrierAccept == /\ s.barrier.phase = "returned"
    /\ (Broken = "flushFailure" \/ ~s.barrier.failed)
    /\ (Broken = "flushGeneration" \/ s.barrier.ticket = s.barrier.generation)
    /\ s' = [s EXCEPT !.barrier.flushed = s.barrier.ticket,
        !.barrier.phase = "approved",
        !.barrier.badApproval = s.barrier.failed,
        !.barrier.badGeneration = s.barrier.ticket # s.barrier.generation,
        !.event = "ApproveCurrentGeneration"]
BarrierClose == /\ s.barrier.phase \in {"waiting", "writing", "returned", "approved"}
    /\ (Broken = "flushClose" \/ (s.barrier.phase = "approved"
        /\ s.barrier.flushed = s.barrier.generation /\ ~s.barrier.failed))
    /\ s' = [s EXCEPT !.barrier.badClose =
        (s.barrier.durable # s.barrier.generation \/ s.barrier.failed),
        !.barrier.phase = "closed", !.event = "CloseHostOrTab"]
BarrierRepoLock == /\ ~s.barrier.repoLock
    /\ (Broken = "flushLock" \/ (s.barrier.phase = "approved"
        /\ s.barrier.flushed = s.barrier.generation /\ ~s.barrier.failed))
    /\ s' = [s EXCEPT !.barrier.repoLock = TRUE,
        !.event = "RepositoryAdmissionAfterFlush"]
DraftBarrierNext == BarrierEdit \/ BarrierRequest \/ BarrierWriteStart
    \/ BarrierWriteFinish \/ BarrierAccept \/ BarrierClose \/ BarrierRepoLock
BarrierFailureNeverApproved == ~s.barrier.badApproval
BarrierApprovalCurrentGeneration == ~s.barrier.badGeneration
BarrierCloseDurable == ~s.barrier.badClose
BarrierLocksNeverNested == ~(s.barrier.draftLock /\ s.barrier.repoLock)

ActorNext(a) == ObserveIndex(a) \/ ObserveMetadata(a) \/ AcceptRead(a) \/ OpenFile(a)
    \/ Choose(a) \/ ChooseNamespace(a) \/ LineToggle(a) \/ ManualEdit(a) \/ AutosaveRead(a)
    \/ AutosaveWriteStart(a) \/ AutosaveWrite(a) \/ AutosaveFailure(a)
    \/ OpenCloseSheet(a) \/ RetryDraft(a) \/ CancelCloseSheet(a)
    \/ Freeze(a) \/ Approve(a) \/ RenewMergePlan(a) \/ Preflight(a) \/ Preserve(a)
    \/ RefusePreservation(a) \/ ValidateFile(a) \/ FileRefused(a)
    \/ WTWrite(a) \/ IndexWrite(a) \/ Abort(a) \/ RefCAS(a) \/ Skip(a) \/ Continue(a)
    \/ MergeCommit(a) \/ Verify(a) \/ AppendReceipt(a) \/ ReleaseLease(a) \/ Deliver(a)
    \/ Depart(a) \/ CloseWindow(a) \/ Quit(a) \/ Crash(a) \/ Restart(a) \/ MetadataRefused(a)
Next == IF Scenario = "draftBarrier" THEN DraftBarrierNext
    ELSE IF Scenario = "draftRecovery" THEN DraftRecoveryNext
    ELSE ((\E a \in Participants : ActorNext(a)) \/ ExternalGit \/ ReplaceOperation
        \/ WatcherRefresh \/ WatcherFileRefresh \/ MetadataReadFailure)
Spec == Init /\ [][Next]_vars
\* Fair scheduling cannot save a callback whose weak owner has been destroyed.
FairSpec == Spec /\ (\A a \in Participants : WF_vars(ActorNext(a)))

\* HOLD: GUI content mutations require an explicit confirmation (not external Git).
NoWriteBeforeApproval == ~s.wroteUnapproved
\* REFUTE globally: merge finalization renews the staged tree after it was shown.
ApprovedRevisionFrozen == ~s.stagedDifferent /\ (s.committedTree = "none" \/ s.committedTree = s.shownTree[1])
\* HOLD: Save's prepared payload is cloned; not the mutable UI buffer.
ApprovedSavePayloadFrozen == \A a \in Participants : s.executed[a] = "none" \/ s.executed[a] = s.frozen[a]
\* HOLD for pre-validation WT edits; REFUTE for the later CLI/concurrent window.
StaleSnapshotNeverWrites == ~s.wroteStale
\* REFUTE: abrupt death can interrupt coalescing/in-flight unacknowledged capture.
DraftNeverLost == (\E a \in Participants : s.restarted[a]) => s.captured \subseteq s.restored
\* HOLD with atomic replacement and the full read/merge/write sidecar lock;
\* draftAtomic/draftLock negative controls retain the old corrupt/lost updates.
DraftWriteAtomic == ~s.readCorrupt /\ (s.diskValid => s.committed \subseteq s.disk)
\* HOLD at fault-free writer completion, not at asynchronous queue acceptance.
ManualCapturePersisted == s.pc[1] = "ready" => s.captured \subseteq s.disk
\* REFUTE for Continue/Skip, HOLD for Save when append succeeds and no crash.
ReceiptFollowsMutation == \A a \in Participants :
    (s.successful[a] /\ (s.released[a] \/ s.life[a] = "closed")) => s.receipt[a]
SkipDropsOnlyApprovedStep == ~s.wrongStep
\* Independent first-line defence, even when the final backend check would refuse.
SkipRefreshDisarmsChangedStep ==
    ~(Scenario = "skipRefresh" /\ s.external /\ ~s.refreshPending
      /\ s.pc[1] = "approve" /\ s.approvedStep[1] # s.operation)
MetadataReadErrorNeverAdmitted == ~s.metadataFailOpen
NoDestructiveProgressAfterPreservationFailure == ~s.destructiveAfterFailure
MergeCommitTreeIsApprovedTree == s.committedTree = "none" \/ s.committedTree = s.shownTree[1]
SiblingWorktreesInvalidatedOnSharedRef ==
    (s.sharedMoved /\ s.pc[1] = "done") => s.siblingInvalidated
\* REFUTE: leases are per application, not a repository-global lock.
RepoHasAtMostOneLease == Cardinality({a \in Actors : s.lease[a]}) <= 1
\* Additional map P1: non-UTF8 stash abort passes verification leaving conflict WT.
StashAbortRestoresConflictedBytes ==
    (Kind = "stashAbort" /\ s.successful[1]) => s.wt[1] = "original"
\* REFUTE even with fair scheduling: close after lease release disables UI append.
ContinueEventuallyRecordedOrReported ==
    (s.successful[1] /\ Kind \in {"continue","mergeContinue"}) ~>
        (s.receipt[1] \/ s.reported[1])
\* Runtime observable quotient, checked by trace/TraceHarness.tla.
\* This is not a proof that partial logs lift to every hidden pc/lease/draft
\* state above: those steps remain explicit log gaps. Klog/witness lanes have
\* no shared timestamps; their order is checked independently.
RuntimeInit == [mode |-> "unknown", skip |-> FALSE, abort |-> FALSE,
    review |-> FALSE, panel |-> FALSE, witness |-> ""]
RuntimeAdvance(r, e) ==
    CASE e.kind = "Start" -> {RuntimeInit}
    \* conflict_detect.rs:296-360: equal revision retains pane, changed replaces.
    \* Log omits revision; both alternatives are possible at equal counts.
    [] e.kind = "Detect" ->
        {[r EXCEPT !.mode = e.op, !.skip = armed] : armed \in {FALSE, r.skip}}
    [] e.kind = "Clear" -> {[r EXCEPT !.mode = "none", !.skip = FALSE,
        !.abort = FALSE, !.review = FALSE, !.panel = FALSE]}
    \* actions.rs:432-442: a new arm log is impossible while still armed.
    [] e.kind = "ArmSkip" ->
        IF r.mode \in {"rebase", "cherry-pick", "revert"} /\ ~r.skip
        THEN {[r EXCEPT !.skip = TRUE]} ELSE {}
    \* conflict_abort.rs:113-131: Abort has separate explicit confirmation.
    [] e.kind = "ArmAbort" ->
        IF r.mode \notin {"unknown", "none"} THEN {[r EXCEPT !.abort = TRUE]} ELSE {}
    \* operations/conflict.rs:487-490: unresolved index can have ready buffers.
    [] e.kind = "Review" ->
        IF r.mode = e.op THEN {[r EXCEPT !.review = TRUE]} ELSE {}
    \* operations/conflict.rs:404-448: routing proves metadata read succeeded.
    [] e.kind = "MergePanel" ->
        IF r.mode = "merge" THEN {[r EXCEPT !.panel = TRUE]} ELSE {}
    [] e.kind = "ExecuteSkip" ->
        IF r.mode = e.op /\ r.skip THEN {[r EXCEPT !.skip = FALSE]} ELSE {}
    [] e.kind = "ExecuteAbort" ->
        IF r.mode = e.op /\ r.abort THEN {[r EXCEPT !.abort = FALSE]} ELSE {}
    [] e.kind = "ExecuteContinue" ->
        IF r.mode = e.op /\ (r.review \/ r.mode = "stash")
        THEN {[r EXCEPT !.review = FALSE]} ELSE {}
    [] e.kind = "MergeCommit" -> IF r.panel THEN {r} ELSE {}
    \* Save footer proves staging, not unlogged draft/lease/append ordering.
    [] e.kind \in {"Save", "Choice", "ChoiceFailed", "DraftFailure", "MetadataRefused"} ->
        IF r.mode \notin {"unknown", "none"} THEN {r} ELSE {}
    \* reload.rs:367 logs AFTER detect; it does not itself reset the arm.
    [] e.kind \in {"Refresh", "Footer", "Failure", "Inspection", "Ancillary"} -> {r}
    \* Digests are opaque. null metadata means absent OR unreadable (C5);
    \* never derive operation completion from null.
    [] e.kind = "WitnessInit" -> {[r EXCEPT !.witness = e.after]}
    [] e.kind \in {"WitnessChanged", "WitnessUnchanged", "ExternalObserved"} ->
        IF r.witness = e.before
           /\ (e.kind # "WitnessUnchanged" \/ e.before = e.after)
           /\ (e.kind # "WitnessChanged" \/ e.before # e.after)
        THEN {[r EXCEPT !.witness = e.after]} ELSE {}
    [] OTHER -> {}
=============================================================================
