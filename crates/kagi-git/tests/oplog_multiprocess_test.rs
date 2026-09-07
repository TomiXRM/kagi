//! #499: two *independent processes* appending to one operation log.
//!
//! The existing coverage is in-process: `ref_backups_test` races eight threads
//! and `oplog_tests` races a queued append against retirement. Both share one
//! address space, so neither proves the sidecar lock (#568) serializes real
//! processes — which is the case the CLI/MCP and the GUI actually create.
//!
//! Each worker is this test binary re-executed with `--exact
//! oplog_append_worker`. The barrier is two-phase: a worker writes its own
//! `ready-<n>` file and only then spins on the shared gate, and the parent
//! opens the gate only after every `ready-<n>` exists — otherwise a worker
//! spawned late could find the gate already open and never overlap. Each
//! worker prints every receipt it was assigned.

use kagi_domain::plan::StateSummary;
use kagi_git::oplog::{append_oplog_receipt, read_oplog_tail, OpLogEntry, OpOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const WORKERS: usize = 2;
const PER_WORKER: usize = 25;

fn state() -> StateSummary {
    StateSummary {
        head: "branch: main".to_string(),
        dirty: "clean".to_string(),
    }
}

#[test]
fn two_processes_appending_keep_ids_unique_and_the_chain_intact() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let log_dir = PathBuf::from(std::env::var("KAGI_LOG_DIR").expect("isolated log directory"));
    let gate = log_dir.join("start");
    let ready = |index: usize| log_dir.join(format!("ready-{index}"));

    let workers: Vec<_> = (0..WORKERS)
        .map(|index| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "oplog_append_worker", "--nocapture"])
                .env("KAGI_OPLOG_APPEND_WORKER", index.to_string())
                .env("KAGI_OPLOG_APPEND_GATE", &gate)
                .env("KAGI_OPLOG_APPEND_READY", ready(index))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("spawn append worker")
        })
        .collect();
    // Release only once every worker reports it is spinning on the gate, so
    // neither can find the gate already open and run to completion alone.
    for index in 0..WORKERS {
        await_file(&ready(index));
    }
    std::fs::write(&gate, b"go").unwrap();

    let mut receipts = Vec::new();
    for worker in workers {
        let output = worker.wait_with_output().expect("await append worker");
        assert!(
            output.status.success(),
            "worker failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        for line in stdout
            .lines()
            .filter_map(|line| line.strip_prefix("receipt "))
        {
            let mut fields = line.split_whitespace();
            let op = fields.next().unwrap().to_string();
            let id: u64 = fields.next().unwrap().parse().unwrap();
            let parent: Option<u64> = fields.next().unwrap().parse().ok();
            receipts.push((op, id, parent));
        }
    }
    assert_eq!(
        receipts.len(),
        WORKERS * PER_WORKER,
        "every receipt returned"
    );

    // Not one line lost, and nothing but our records.
    let raw = std::fs::read_to_string(log_dir.join("operations.jsonl")).unwrap();
    assert_eq!(raw.lines().count(), WORKERS * PER_WORKER);

    let mut entries = read_oplog_tail(WORKERS * PER_WORKER * 2);
    assert_eq!(entries.len(), WORKERS * PER_WORKER);
    entries.reverse(); // oldest first: file order

    let ids: std::collections::HashSet<u64> = entries.iter().map(|entry| entry.id).collect();
    assert_eq!(ids.len(), entries.len(), "ids are unique across processes");
    assert_eq!(entries[0].parent, None, "the first entry has no parent");
    for pair in entries.windows(2) {
        assert!(
            pair[0].id < pair[1].id,
            "ids increase in file order: {} then {}",
            pair[0].id,
            pair[1].id
        );
        assert_eq!(
            pair[1].parent,
            Some(pair[0].id),
            "each entry chains to the previous line"
        );
    }

    // Each process's own receipts describe the entry that actually landed —
    // no cross-process mix-up of id, parent or operation.
    for (op, id, parent) in &receipts {
        let persisted = entries
            .iter()
            .find(|entry| &entry.op == op)
            .unwrap_or_else(|| panic!("{op} is missing from the log"));
        assert_eq!(persisted.id, *id, "{op} receipt id");
        assert_eq!(persisted.parent, *parent, "{op} receipt parent");
    }
    for index in 0..WORKERS {
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.op.starts_with(&format!("worker-{index}-")))
                .count(),
            PER_WORKER
        );
    }
}

/// Worker side of the test above. Inert unless the parent asked for it, so a
/// plain suite run never touches the log.
#[test]
fn oplog_append_worker() {
    let Ok(index) = std::env::var("KAGI_OPLOG_APPEND_WORKER") else {
        return;
    };
    let gate = PathBuf::from(std::env::var("KAGI_OPLOG_APPEND_GATE").expect("gate path"));
    let ready = PathBuf::from(std::env::var("KAGI_OPLOG_APPEND_READY").expect("ready path"));
    // Announce readiness last, so the parent's release really finds us waiting.
    std::fs::write(&ready, b"ready").unwrap();
    await_file(&gate);
    for step in 0..PER_WORKER {
        let entry = OpLogEntry::new(
            format!("worker-{index}-{step}"),
            "/tmp/kagi-oplog-499",
            state(),
            OpOutcome::Success { after: state() },
        );
        let (_, assigned) = append_oplog_receipt(&entry).expect("append");
        println!(
            "receipt {} {} {}",
            assigned.op,
            assigned.id,
            match assigned.parent {
                Some(parent) => parent.to_string(),
                None => "none".to_string(),
            }
        );
    }
}

fn await_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
