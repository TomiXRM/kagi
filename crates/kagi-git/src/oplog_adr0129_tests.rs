//! ADR-0129 Phase 1 — oplog on-disk compatibility tests (child module of
//! `oplog.rs`; split out for the LOC ratchet).
//!
//! The schema stays `blockers: [String]`; the OperationPlan → oplog boundary
//! converts `PlanNote` via `message_en()`. These tests pin (a) pre-migration
//! literal JSONL still parses, (b) old and new lines mix in one file,
//! (c) structured notes serialize as plain strings.

use super::super::oplog::*;
use kagi_domain::plan_note::{DiscardNote, PlanNote};

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

#[test]
fn mixed_old_and_new_lines_parse_together() {
    // New line: written through today's writer from a plan whose notes are
    // structured — the boundary renders them to plain strings first.
    let notes = [
        PlanNote::Discard(DiscardNote::TargetConflicted {
            path: "src/a.rs".to_string(),
        }),
        PlanNote::Discard(DiscardNote::NothingSelected),
    ];
    let new_entry = OpLogEntry {
        timestamp: 1_752_000_000,
        op: "discard".to_string(),
        repo: "/tmp/repo".to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "2 modified".to_string(),
        },
        outcome: OpOutcome::Refused {
            blockers: notes.iter().map(|n| n.message_en()).collect(),
        },
    };
    let new_line = entry_to_json(&new_entry);

    let file = format!("{}\n{}\n{}\n", OLD_REFUSED_LINE, OLD_SUCCESS_LINE, new_line);
    let parsed: Vec<OpLogEntry> = file
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_oplog_line)
        .collect();
    assert_eq!(parsed.len(), 3, "old and new lines must all parse");
    match &parsed[2].outcome {
        OpOutcome::Refused { blockers } => {
            assert_eq!(
                blockers[0],
                "'src/a.rs' is conflicted. Resolve the conflict instead of discarding it."
            );
        }
        other => panic!("expected Refused, got {:?}", other),
    }
}

#[test]
fn structured_notes_serialize_as_plain_string_array() {
    let entry = OpLogEntry {
        timestamp: 1,
        op: "discard".to_string(),
        repo: "/r".to_string(),
        before: StateSummary {
            head: "branch: m".to_string(),
            dirty: "clean".to_string(),
        },
        outcome: OpOutcome::Refused {
            blockers: vec![PlanNote::Discard(DiscardNote::NothingSelected).message_en()],
        },
    };
    let json = entry_to_json(&entry);
    // On-disk stays a plain string array — no structured objects leak in.
    assert!(json.contains("\"blockers\":[\"Nothing to discard: no files selected.\"]"));
}
