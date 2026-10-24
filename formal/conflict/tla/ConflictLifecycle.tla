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
Kind == CASE Scenario \in {"skip", "preserveSkip", "receiptSkip"} -> "skip"
          [] Scenario \in {"preserveAbort", "sibling"} -> "abort"
          [] Scenario = "stashAbort" -> "stashAbort"
          [] Scenario = "preserveDF" -> "df"
          [] Scenario \in {"receiptContinue", "liveness"} -> "continue"
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
    captured |-> {}, disk |-> {}, diskValid |-> TRUE,
    pending |-> [a \in Actors |-> {}], committed |-> {}, restored |-> {},
    readCorrupt |-> FALSE, restarted |-> [a \in Actors |-> FALSE],
    frozen |-> [a \in Actors |-> "none"], shownTree |-> [a \in Actors |-> "unmerged"],
    planTree |-> [a \in Actors |-> "unmerged"], planRev |-> [a \in Actors |-> <<0,0,0,0>>],
    approvedStep |-> ActorInit, approved |-> [a \in Actors |-> FALSE],
    lease |-> [a \in Actors |-> FALSE], preflight |-> [a \in Actors |-> FALSE],
    executed |-> [a \in Actors |-> "none"], successful |-> [a \in Actors |-> FALSE],
    receipt |-> [a \in Actors |-> FALSE], released |-> [a \in Actors |-> FALSE],
    reported |-> [a \in Actors |-> FALSE], mutated |-> [a \in Actors |-> FALSE],
    wroteUnapproved |-> FALSE, wroteStale |-> FALSE, wrongStep |-> FALSE, stagedDifferent |-> FALSE,
    preservationFailed |-> [a \in Actors |-> FALSE], destructiveAfterFailure |-> FALSE,
    sharedMoved |-> FALSE, siblingInvalidated |-> FALSE,
    external |-> FALSE, committedTree |-> "none", abandoned |-> FALSE,
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
OpenFile(a) == /\ At(a, "openFile")
    /\ s' = [s EXCEPT !.pc[a] = "edit", !.event = <<"OpenFile",a,File(a)>>]
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
\* draft_owner.rs:13-18,180-187,210-230: Change captures AND autosaves now.
\* Broken="capture": remove the changed-capture persistence call (old map bug).
ManualEdit(a) == /\ At(a, "edit") /\ ~s.lease[a] /\ DraftScenario
    /\ s' = [s EXCEPT !.text[a] = "manual", !.pc[a] = IF Broken = "capture" THEN "ready" ELSE "autosaveRead",
        !.captured = @ \cup {<<File(a),s.context[a],"manual">>}, !.event = <<"ManualEdit",a>>]
\* resolution/drafts.rs:256-301: unlocked load/merge before write; distinct-file
\* writers preserve each other's records ONLY if present in this read snapshot.
AutosaveRead(a) == /\ At(a, "autosaveRead")
    \* Schedule restriction isolates stale overwrite from the shorter torn-read bug.
    /\ (Scenario # "draftRaceLost" \/ s.diskValid)
    /\ s' = [s EXCEPT !.pending[a] = (IF s.diskValid THEN s.disk ELSE {}) \cup {Record(a)},
        !.readCorrupt = @ \/ ~s.diskValid, !.pc[a] = "autosaveStart", !.event = <<"AutosaveRead",a>>]
\* resolution.rs:800-806 fs::write opens/truncates destination before write_all.
\* Split syscall effects so another process or process death can observe a tear.
AutosaveWriteStart(a) == /\ At(a, "autosaveStart")
    /\ s' = [s EXCEPT !.diskValid = FALSE, !.disk = {},
        !.pc[a] = "autosaveFinish", !.event = <<"AutosaveWriteStart(truncate)",a>>]
\* resolution.rs:800-814: mark_saved only after complete successful fs::write.
AutosaveWrite(a) == /\ At(a, "autosaveFinish")
    /\ s' = [s EXCEPT !.diskValid = TRUE, !.disk = s.pending[a],
        !.committed = @ \cup {Record(a)}, !.pc[a] = "ready", !.event = <<"AutosaveWrite",a>>]
\* draft_owner.rs:233-236 logs failure; actions.rs:145 ignores it. No close flush.
AutosaveFailure(a) == /\ Scenario = "draftIO" /\ At(a, "autosaveStart")
    /\ s' = [s EXCEPT !.pc[a] = "ready", !.event = <<"AutosaveFailure",a>>]
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
    /\ s' = [s EXCEPT !.approved[a] = Broken # "approval", !.lease[a] = TRUE,
        !.pc[a] = "preflight", !.event = <<"Approve(confirm)",a>>]
\* ui/operations/commit.rs:1191-1209: silently renews shown merge plan at click.
RenewMergePlan(a) == /\ At(a, "preflight") /\ Kind = "mergeCommit"
    /\ s' = [s EXCEPT !.planTree[a] = s.index[1], !.planRev[a] = Revision,
        !.pc[a] = "mergePreflight", !.event = <<"RenewMergePlan",a>>]
\* conflict_ops.rs:419-425; conflict_boundary.rs:170-175; conflicts.rs:1261-1297:
\* final full revision/binding/checklist check; WT content deliberately absent.
\* Skip has no expected revision argument (conflicts.rs:1869-1894).
\* Commit index checked against the NEW plan (ops/commit.rs:38-44,74-83).
Preflight(a) == /\ At(a, IF Kind = "mergeCommit" THEN "mergePreflight" ELSE "preflight")
    /\ (Kind = "skip" \/ s.planRev[a] = Revision \/ Broken = "revision")
    /\ (Kind # "mergeCommit" \/ s.planTree[a] = s.index[1])
    /\ s' = [s EXCEPT !.preflight[a] = TRUE,
        !.pc[a] = CASE Kind \in {"skip","abort","stashAbort","df"} -> "preserve"
                  [] Kind = "mergeCommit" -> "commit" [] OTHER -> "wt",
        !.event = <<"Preflight",a>>]
\* conflicts.rs:1880-1881; conflict_abort.rs:274-286,489-493; dir_file_conflict.rs:202-218.
\* Preservation failure is ignored; failure schedules are explicit scenario inputs.
Preserve(a) == /\ At(a, "preserve")
    /\ s' = [s EXCEPT !.preservationFailed[a] = Scenario \in {"preserveAbort","preserveSkip","preserveDF"},
        \* A possible fs::write error AFTER truncation destroys even an older save.
        \* resolution.rs:800-806; abort/skip swallow the error at the sites above.
        !.diskValid = IF Scenario \in {"preserveAbort","preserveSkip"} THEN FALSE ELSE @,
        !.disk = IF Scenario \in {"preserveAbort","preserveSkip"} THEN {} ELSE @,
        !.pc[a] = CASE Kind = "skip" -> "skip" [] Kind \in {"abort","stashAbort"} -> "abort" [] OTHER -> "index",
        !.event = <<"Preserve",a,IF Scenario \in {"preserveAbort","preserveSkip","preserveDF"} THEN "failed" ELSE "ok">>]
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
\* conflicts.rs:1893-1894: read CURRENT step and run --skip; no approved-step test.
Skip(a) == /\ At(a, "skip")
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
\* app_bridge.rs:751-758; host_close.rs:62-68,107-111: lease gate, no conflict dirty gate.
\* Sync autosave callback cannot be interrupted by graceful close in same process.
GracefulExit(a, reason) == /\ a \in Participants /\ s.life[a] = "open" /\ ~s.lease[a]
    /\ s.pc[a] \in {"ready","append","deliver","done"}
    /\ s' = [s EXCEPT !.life[a] = "closed", !.attached[a] = FALSE,
        !.text[a] = "none", !.abandoned = @ \/ (s.successful[a] /\ ~s.receipt[a]),
        !.event = <<reason,a>>]
CloseWindow(a) == GracefulExit(a, "CloseWindow")
\* ui/mod.rs:3661-3669 flushes settings/issue drafts, NOT conflict documents.
Quit(a) == GracefulExit(a, "Quit")
\* resolution.rs:800-806 non-atomic fs::write; environment may kill mid-syscall.
\* Only draft crash scenarios allow abrupt exit; liveness uses graceful close.
Crash(a) == /\ Scenario \in {"draftCrash","draftAtomic"} /\ a = 1
    /\ s.life[a] = "open" /\ s.pc[a] \in {"autosaveRead","autosaveStart","autosaveFinish","ready"}
    /\ s' = [s EXCEPT !.life[a] = "crashed", !.text[a] = "none", !.lease[a] = FALSE,
        !.event = <<"Crash",a>>]
\* resolution.rs:817-822 corrupt => absent; drafts.rs:176-247 identity match only.
\* conflict_detect.rs:78-84 rebuilds sides, restores compatible same-step draft.
Restart(a) == /\ DraftScenario /\ a \in Participants /\ s.life[a] # "open" /\ ~s.restarted[a]
    /\ s' = [s EXCEPT !.life[a] = "open", !.restarted[a] = TRUE, !.pc[a] = "done",
        !.restored = @ \cup (IF s.diskValid THEN {r \in s.disk : r[2] = s.operation} ELSE {}),
        !.readCorrupt = @ \/ ~s.diskValid, !.event = <<"Restart(restore draft)",a>>]
\* Environmental CLI writes; observer includes index/HEAD/metadata but not WT:
\* conflict_observe.rs:91-125,148-198; ui/tabs.rs:854-863 WT refreshes WIP only.
\* One external edit is the finite interference bound, not a production guard.
ExternalGit == /\ ~s.external /\ s.life[1] = "open"
    /\ CASE Scenario = "staleWT" ->
          /\ s.pc[1] = "preflight"
          /\ s' = [s EXCEPT !.wt[1] = "external", !.external = TRUE, !.event = "ExternalGit(worktree)"]
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
\* conflicts.rs:1893-1894 live position read allows CLI to replace approved step.
\* Both revision and draft context change when REBASE_HEAD/step change:
\* conflict_observe.rs:155-176; drafts.rs:176-247 disallows fresh-context adoption.
ReplaceOperation == /\ Scenario = "skip" /\ ~s.external /\ s.pc[1] = "preflight"
    /\ s' = [s EXCEPT !.operation = @ + 1, !.metadataRev = @ + 1,
        !.external = TRUE, !.event = "ReplaceOperation(next rebase step)"]

ActorNext(a) == ObserveIndex(a) \/ ObserveMetadata(a) \/ AcceptRead(a) \/ OpenFile(a)
    \/ Choose(a) \/ ChooseNamespace(a) \/ LineToggle(a) \/ ManualEdit(a) \/ AutosaveRead(a)
    \/ AutosaveWriteStart(a) \/ AutosaveWrite(a) \/ AutosaveFailure(a)
    \/ Freeze(a) \/ Approve(a) \/ RenewMergePlan(a) \/ Preflight(a) \/ Preserve(a)
    \/ WTWrite(a) \/ IndexWrite(a) \/ Abort(a) \/ RefCAS(a) \/ Skip(a) \/ Continue(a)
    \/ MergeCommit(a) \/ Verify(a) \/ AppendReceipt(a) \/ ReleaseLease(a) \/ Deliver(a)
    \/ Depart(a) \/ CloseWindow(a) \/ Quit(a) \/ Crash(a) \/ Restart(a)
Next == (\E a \in Participants : ActorNext(a)) \/ ExternalGit \/ ReplaceOperation
Spec == Init /\ [][Next]_vars
\* Fair scheduling cannot save a callback whose weak owner has been destroyed.
FairSpec == Spec /\ (\A a \in Participants : WF_vars(ActorNext(a)))

\* HOLD: GUI content mutations require an explicit confirmation (not external Git).
NoWriteBeforeApproval == ~s.wroteUnapproved
\* REFUTE globally: merge finalization renews the staged tree after it was shown.
ApprovedRevisionFrozen == ~s.stagedDifferent /\ (s.committedTree = "none" \/ s.committedTree = s.shownTree[1])
\* HOLD: Save's prepared payload is cloned; not the mutable UI buffer.
ApprovedSavePayloadFrozen == \A a \in Participants : s.executed[a] = "none" \/ s.executed[a] = s.frozen[a]
\* REFUTE: WT-only changes are not approval-revision drift; final check/write split.
StaleSnapshotNeverWrites == ~s.wroteStale
\* REFUTE strong universal claim: captured edits may be lost on crash/I/O failure.
DraftNeverLost == (\E a \in Participants : s.restarted[a]) => s.captured \subseteq s.restored
\* REFUTE: torn JSON observed by another reader/restart, or committed distinct-file
\* records overwritten by an older load/merge/write snapshot.
DraftWriteAtomic == ~s.readCorrupt /\ (s.diskValid => s.committed \subseteq s.disk)
\* HOLD in fault-free single-writer callback completion; obsolete map P1 is guarded.
ManualCapturePersisted == s.pc[1] = "ready" => s.captured \subseteq s.disk
\* REFUTE for Continue/Skip, HOLD for Save when append succeeds and no crash.
ReceiptFollowsMutation == \A a \in Participants :
    (s.successful[a] /\ (s.released[a] \/ s.life[a] = "closed")) => s.receipt[a]
SkipDropsOnlyApprovedStep == ~s.wrongStep
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
=============================================================================
