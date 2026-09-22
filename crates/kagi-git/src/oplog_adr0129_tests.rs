//! ADR-0129 Phase 1 — oplog on-disk compatibility tests (child module of
//! `oplog.rs`; split out for the LOC ratchet).
//!
//! Historical literal receipt lines remain readable after the PlanNote migration.

use super::super::oplog::*;

/// A literal line captured from the PRE-migration writer (byte-for-byte
/// the shape `entry_to_json` produced before ADR-0129). If the writer or
/// reader shape drifts, this fixture fails.
const OLD_REFUSED_LINE: &str = "{\"timestamp\":1751234567,\"op\":\"delete-branch\",\"repo\":\"/tmp/repo\",\"before\":{\"head\":\"branch: main\",\"dirty\":\"clean\"},\"outcome\":{\"kind\":\"Refused\",\"blockers\":[\"Branch 'x' has unmerged commits (tip abcd1234 is not reachable from HEAD). Merge or discard the branch manually before deleting. Force delete is not provided.\",\"HEAD is detached and points to the same commit as 'x'. This branch cannot be deleted while HEAD is at its tip.\"]}}";

const OLD_SUCCESS_LINE: &str = "{\"timestamp\":1751234568,\"op\":\"checkout\",\"repo\":\"/tmp/repo\",\"before\":{\"head\":\"branch: main\",\"dirty\":\"1 modified\"},\"outcome\":{\"kind\":\"Success\",\"after\":{\"head\":\"branch: dev\",\"dirty\":\"1 modified\"}}}";

#[test]
fn pre_migration_literal_jsonl_parses() {
    let e = parse_oplog_line(OLD_REFUSED_LINE).expect("old Refused line must parse");
    match &e.outcome {
        OpOutcome::Refused { blockers } => {
            assert_eq!(blockers.len(), 2);
            assert!(blockers[0].starts_with("Branch 'x' has unmerged commits"));
        }
        other => panic!("expected Refused, got {:?}", other),
    }
    let e2 = parse_oplog_line(OLD_SUCCESS_LINE).expect("old Success line must parse");
    assert!(matches!(e2.outcome, OpOutcome::Success { .. }));
}
