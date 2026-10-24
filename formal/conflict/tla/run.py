#!/usr/bin/env python3
"""Sequential TLC self-check. Exit 0 expected; 1 mismatch; 2 tooling failure.
Dependencies: CPython >=3.11 stdlib; Java 21 on PATH; pinned cached TLC jar.
No product repository commands are run. Generated evidence stays in this directory.
"""
from pathlib import Path
import json
import os
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
# name, scenario, invariant/property, expectation, removed guard
CHECKS = [
    ("approval", "save", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-broken", "save", "NoWriteBeforeApproval", "REFUTE", "approval"),
    ("approval-skip", "receiptSkip", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-abort", "sibling", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-dir-file", "preserveDF", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-continue", "receiptContinue", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-merge-continue", "receiptMerge", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-merge-commit", "merge", "NoWriteBeforeApproval", "HOLD", "none"),
    ("approval-two-instances", "saveRace", "NoWriteBeforeApproval", "HOLD", "none"),
    ("save-payload", "save", "ApprovedSavePayloadFrozen", "HOLD", "none"),
    ("save-payload-broken", "save", "ApprovedSavePayloadFrozen", "REFUTE", "payload"),
    ("approved-revision", "merge", "ApprovedRevisionFrozen", "REFUTE", "none"),
    ("approved-index-race", "stageWT", "ApprovedRevisionFrozen", "REFUTE", "none"),
    ("approved-two-writers", "saveRace", "ApprovedRevisionFrozen", "REFUTE", "none"),
    ("stale-worktree", "staleWT", "StaleSnapshotNeverWrites", "REFUTE", "none"),
    ("stale-index", "staleIndex", "StaleSnapshotNeverWrites", "HOLD", "none"),
    ("stale-index-broken", "staleIndex", "StaleSnapshotNeverWrites", "REFUTE", "revision"),
    ("stale-head", "staleHead", "StaleSnapshotNeverWrites", "HOLD", "none"),
    ("stale-head-broken", "staleHead", "StaleSnapshotNeverWrites", "REFUTE", "revision"),
    ("stale-metadata", "staleMeta", "StaleSnapshotNeverWrites", "HOLD", "none"),
    ("stale-metadata-broken", "staleMeta", "StaleSnapshotNeverWrites", "REFUTE", "revision"),
    ("stale-after-preflight", "raceIndex", "StaleSnapshotNeverWrites", "REFUTE", "none"),
    ("manual-capture", "manual", "ManualCapturePersisted", "HOLD", "none"),
    ("manual-capture-broken", "manual", "ManualCapturePersisted", "REFUTE", "capture"),
    ("draft-crash", "draftCrash", "DraftNeverLost", "REFUTE", "none"),
    ("draft-io-quit", "draftIO", "DraftNeverLost", "REFUTE", "none"),
    ("draft-torn", "draftAtomic", "DraftWriteAtomic", "REFUTE", "none"),
    ("draft-two-writers", "draftRace", "DraftWriteAtomic", "REFUTE", "none"),
    ("draft-lost-update", "draftRaceLost", "DraftWriteAtomic", "REFUTE", "none"),
    ("two-instance-leases", "saveRace", "RepoHasAtMostOneLease", "REFUTE", "none"),
    ("two-instance-stale-write", "saveRace", "StaleSnapshotNeverWrites", "REFUTE", "none"),
    ("receipt-continue", "receiptContinue", "ReceiptFollowsMutation", "REFUTE", "none"),
    ("receipt-merge", "receiptMerge", "ReceiptFollowsMutation", "REFUTE", "none"),
    ("receipt-skip", "receiptSkip", "ReceiptFollowsMutation", "REFUTE", "none"),
    ("receipt-save", "receiptSave", "ReceiptFollowsMutation", "HOLD", "none"),
    ("receipt-save-broken", "receiptSave", "ReceiptFollowsMutation", "REFUTE", "receipt"),
    ("skip-replacement", "skip", "SkipDropsOnlyApprovedStep", "REFUTE", "none"),
    ("preserve-abort", "preserveAbort", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("preserve-skip", "preserveSkip", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("preserve-dir-file", "preserveDF", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("merge-tree", "merge", "MergeCommitTreeIsApprovedTree", "REFUTE", "none"),
    ("sibling-invalidation", "sibling", "SiblingWorktreesInvalidatedOnSharedRef", "REFUTE", "none"),
    ("stash-non-utf8", "stashAbort", "StashAbortRestoresConflictedBytes", "REFUTE", "none"),
    ("continue-liveness", "liveness", "ContinueEventuallyRecordedOrReported", "REFUTE", "none"),
]

def main():
    mutation_check = "--broken" in sys.argv[1:]
    out = ROOT / ("broken-results" if mutation_check else "results")
    out.mkdir(exist_ok=True)
    (out / "tmp").mkdir(exist_ok=True)
    selected = set(sys.argv[1:]) - {"--broken"}
    if selected - {c[0] for c in CHECKS}:
        print("Unknown checks: " + repr(selected), file=sys.stderr)
        return 2
    results = []
    start_all = time.monotonic()
    for name, scenario, invariant, expected, broken in CHECKS:
        if selected and name not in selected:
            continue
        if mutation_check:
            if broken == "none":
                continue
            expected = "HOLD"
        temporal = invariant == "ContinueEventuallyRecordedOrReported"
        command = ["java", "-XX:+UseParallelGC", "-Xmx1024m",
                   "-Djava.io.tmpdir=" + str(out / "tmp"),
                   "-cp", os.environ["TLA2TOOLS_JAR"], "tlc2.TLC", "-workers", "1",
                   "-seed", "1", "-fp", "0", "-noGenerateSpecTE", "-metadir", str(out / (name + "-states")),
                   "-config", str(ROOT / (name + ".cfg")), str(ROOT / "ConflictLifecycle.tla")]
        t0 = time.monotonic()
        try:
            completed = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, timeout=30)
        except (OSError, subprocess.TimeoutExpired) as exc:
            print(f"TOOL ERROR {name}: {exc}", file=sys.stderr)
            return 2
        elapsed = round(time.monotonic() - t0, 3)
        text = completed.stdout
        (out / (name + ".log")).write_text(text)
        violated = (f"Invariant {invariant} is violated" in text or
                    (temporal and (f"Temporal property {invariant} was violated" in text or
                                   "Temporal properties were violated" in text)))
        held = "Model checking completed. No error has been found." in text
        actual = "REFUTE" if violated else "HOLD" if held else "ERROR"
        if actual == "ERROR" or completed.returncode != (0 if held else 13 if temporal else 12):
            print(f"TOOL ERROR {name}: invalid TLC verdict/exit {completed.returncode}; see {out / (name + '.log')}", file=sys.stderr)
            return 2
        matches = list(re.finditer(r"([\d,]+) states generated, ([\d,]+) distinct states found", text))
        generated, distinct = (map(lambda v: int(v.replace(",", "")), matches[-1].groups())
                               if matches else (None, None))
        trace = []
        for block in re.split(r"(?m)^State \d+:.*\n", text)[1:]:
            m = re.search(r"event\s*\|->\s*(<<.*?>>|\"[^\"]*\")", block, re.S)
            if m:
                trace.append(re.sub(r"\s+", " ", m.group(1)))
        cycle = re.search(r"(Back to state \d+|Stuttering)", text)
        if cycle:
            trace.append(cycle.group(1))
        result = dict(name=name, scenario=scenario, invariant=invariant, expectation=expected,
                      result=actual, broken=broken, seconds=elapsed, states=distinct,
                      generated=generated, tlc_exit=completed.returncode, trace=trace,
                      log="tla/results/" + name + ".log")
        results.append(result)
        (out / (name + ".json")).write_text(json.dumps(result, indent=2) + "\n")
        ok = actual == expected and ((actual == "HOLD" and completed.returncode == 0) or
                                    (actual == "REFUTE" and completed.returncode == (13 if temporal else 12) and bool(trace)))
        print(f"{'PASS' if ok else 'FAIL'} {name}: {actual} expected={expected} states={distinct} {elapsed}s", flush=True)
        if not ok and not mutation_check:
            print(text[-7000:], file=sys.stderr)
    summary = dict(seconds=round(time.monotonic()-start_all, 3), checks=results,
                   distinct_states_sum=sum(r["states"] or 0 for r in results),
                   ok=bool(results) and all(r["result"] == r["expectation"] and
                          ((r["result"] == "HOLD" and r["tlc_exit"] == 0) or
                           (r["result"] == "REFUTE" and r["tlc_exit"] ==
                            (13 if r["invariant"] == "ContinueEventuallyRecordedOrReported" else 12)
                            and bool(r["trace"])))
                          for r in results))
    (out / ("summary-selected.json" if selected else "summary.json")).write_text(json.dumps(summary, indent=2) + "\n")
    print(f"{len(results)} checks; {summary['distinct_states_sum']} distinct states summed across runs; {summary['seconds']}s")
    return 0 if summary["ok"] else 1

if __name__ == "__main__":
    raise SystemExit(main())
