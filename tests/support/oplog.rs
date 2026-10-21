//! Oplog collection only; receipt expectations remain in each scenario.

use std::path::Path;

use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry};

/// Filter the last `limit` repository records by operation, then order the result.
/// The limit applies before operation filtering, matching the backend tail API.
pub fn records(repo: &Path, op: &str, limit: usize, newest_first: bool) -> Vec<OpLogEntry> {
    let mut records: Vec<_> = read_oplog_tail_for_repo(repo, limit)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect();
    if !newest_first {
        records.reverse();
    }
    records
}
