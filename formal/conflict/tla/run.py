#!/usr/bin/env python3
"""Batched TLC self-check. Exit 0 expected; 1 mismatch; 2 tooling failure.
Dependencies: CPython >=3.11 stdlib; Java 21 on PATH; pinned cached TLC jar.
No product repository commands are run. Generated evidence stays in this directory.
"""
from pathlib import Path
import sys

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
    ("stale-worktree", "staleWT", "StaleSnapshotNeverWrites", "HOLD", "none"),
    ("stale-worktree-broken", "staleWT", "StaleSnapshotNeverWrites", "REFUTE", "worktree"),
    ("stale-worktree-before-choice", "staleWTBeforeChoice", "StaleSnapshotNeverWrites", "HOLD", "none"),
    ("stale-worktree-before-choice-broken", "staleWTBeforeChoice", "StaleSnapshotNeverWrites", "REFUTE", "materialization"),
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
    ("receipt-continue", "receiptContinue", "ReceiptFollowsMutation", "HOLD", "none"),
    ("receipt-continue-broken", "receiptContinue", "ReceiptFollowsMutation", "REFUTE", "workerReceipt"),
    ("receipt-merge", "receiptMerge", "ReceiptFollowsMutation", "HOLD", "none"),
    ("receipt-merge-broken", "receiptMerge", "ReceiptFollowsMutation", "REFUTE", "workerReceipt"),
    ("receipt-skip", "receiptSkip", "ReceiptFollowsMutation", "HOLD", "none"),
    ("receipt-skip-broken", "receiptSkip", "ReceiptFollowsMutation", "REFUTE", "workerReceipt"),
    ("receipt-save", "receiptSave", "ReceiptFollowsMutation", "HOLD", "none"),
    ("receipt-save-broken", "receiptSave", "ReceiptFollowsMutation", "REFUTE", "receipt"),
    ("skip-replacement", "skipRefresh", "SkipDropsOnlyApprovedStep", "HOLD", "none"),
    ("skip-refresh-broken", "skipRefresh", "SkipRefreshDisarmsChangedStep", "REFUTE", "skipReset"),
    ("skip-confirm-cli-race", "skip", "SkipDropsOnlyApprovedStep", "HOLD", "none"),
    ("skip-prewrite-broken", "skip", "SkipDropsOnlyApprovedStep", "REFUTE", "skipRevision"),
    ("continue-metadata-read", "metadataUnreadable", "MetadataReadErrorNeverAdmitted", "HOLD", "none"),
    ("continue-metadata-read-broken", "metadataUnreadable", "MetadataReadErrorNeverAdmitted", "REFUTE", "metadata"),
    ("preserve-abort", "preserveAbort", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("preserve-skip", "preserveSkip", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("preserve-dir-file", "preserveDF", "NoDestructiveProgressAfterPreservationFailure", "REFUTE", "none"),
    ("merge-tree", "merge", "MergeCommitTreeIsApprovedTree", "REFUTE", "none"),
    ("sibling-invalidation", "sibling", "SiblingWorktreesInvalidatedOnSharedRef", "REFUTE", "none"),
    ("stash-non-utf8", "stashAbort", "StashAbortRestoresConflictedBytes", "REFUTE", "none"),
    ("continue-liveness", "liveness", "ContinueEventuallyRecordedOrReported", "HOLD", "none"),
    ("continue-liveness-broken", "liveness", "ContinueEventuallyRecordedOrReported", "REFUTE", "workerReceipt"),
]

def main():
    sys.path.insert(0, str(ROOT / "trace"))
    from check_lifecycle import run
    return run(CHECKS, ROOT)

if __name__ == "__main__":
    raise SystemExit(main())
