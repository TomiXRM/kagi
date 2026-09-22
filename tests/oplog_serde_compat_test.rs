//! On-disk compatibility regressions for the operation log (#513).
//!
//! The oplog is a durable receipt: lines written by an older Kagi must keep
//! their full meaning for a reader that ships later. These tests read frozen
//! `.jsonl` files from `tests/fixtures/oplog/` through the public reader and
//! compare the **restored consumer values** — ids, actor, outcome payloads,
//! backup roots, typed recovery handles — never the JSON text and never the
//! serializer's own output as the oracle.
//!
//! ## Fixture provenance
//!
//! * `captured_operations.jsonl` — **real log, unmodified.** Captured on
//!   2026-09-23 from the pre-migration code at commit `2abca92c` (extracted to
//!   a scratch tree with `git archive`, driven by a throwaway
//!   `kagi-git` example). Every line is the receipt
//!   `Backend::run_recorded` wrote for an actual operation against a throwaway
//!   git repository at `/tmp/kagi-oplog-fixture/rëpo ✨ "quoted"` (the path was
//!   chosen before the run so the fixture is deterministic and carries no user
//!   data): create-branch, a refused stash-push on a clean tree, stash-push,
//!   stash-drop, a two-file discard, restore-snapshot, delete-branch, a
//!   checkout that failed preflight because HEAD moved after planning, and a
//!   create-branch-with-checkout left `Partial` by a held `index.lock`.
//!   Nothing in this file was edited afterwards.
//! * `synthetic_outcomes.jsonl` — **synthetic entries, real codec.** Receipts a
//!   fixture repository cannot produce on demand (`Unknown` from an unproven
//!   termination, a `Partial` discard with control characters, every recovery
//!   kind at once, a string holding every escape the writer emits). Each line is
//!   the pre-migration `entry_to_json` output for a hand-built entry.
//! * `legacy_and_edge.jsonl` — **synthetic and deliberately hostile.** Lines
//!   start as pre-migration codec output and are then mutated: pre-ADR-0149
//!   id-less rows, truncated/garbage lines, fields from a newer Kagi (including
//!   a nested decoy object), `backup_refs` of the wrong type, an unknown outcome
//!   kind, an `Unknown` missing its evidence, `\uXXXX` surrogate pairs and
//!   control escapes, malformed `recovery` elements, and scalars written as
//!   strings.
//!
//! `KAGI_LOG_DIR` is process-global, so every test runs in its own child
//! process with its own log directory (`support/isolated.rs`).

use kagi_git::oplog::{
    append_oplog_receipt, entry_to_json, read_oplog_tail, recovery, Actor, FailureCode, OpLogEntry,
    OpOutcome, RecoveryHandle,
};
use kagi_git::ops::StateSummary;
use std::path::PathBuf;

/// Working tree the captured operations ran in (`/tmp` resolves to `/private/tmp`).
const CAPTURED_REPO: &str = "/private/tmp/kagi-oplog-fixture/rëpo ✨ \"quoted\"/";
const CAPTURED_TIMESTAMP: i64 = 1_790_111_445;
const SYNTHETIC_REPO: &str = "/tmp/kagi-oplog-fixture/rëpo ✨ \"quoted\"";

const STASH_OID: &str = "6f3e172b6d37ee647ecae2ecb31eb4a8a8513558";
const DISCARD_BLOB_UNICODE: &str = "8211995224938c05ba5357d4b907bfc8e4dc09bd";
const DISCARD_BLOB_QUOTE: &str = "5dc700a4bee042531398099c022b2b8c5158d881";
const DISCARD_REF_UNICODE: &str = "refs/kagi/backups/18d7c0e00af97770-14708-0/0";
const DISCARD_REF_QUOTE: &str = "refs/kagi/backups/18d7c0e00af97770-14708-0/1";
const BRANCH_TIP_OID: &str = "a574b5a97e012d4c222baaa5288568db54a687c4";
const BRANCH_TIP_REF: &str = "refs/kagi/backups/18d7c0e00d6ef3e0-14708-1/0";

// ── The consumer's view of one receipt ──────────────────────────────────────

/// The outcome as a consumer sees it: variant plus every payload string. Built
/// from the typed `OpOutcome`, so a comparison can never be satisfied by
/// re-serializing an entry.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Success {
        head: String,
        dirty: String,
    },
    Partial {
        head: String,
        dirty: String,
        error: String,
    },
    Unknown {
        head: String,
        dirty: String,
        evidence: String,
    },
    Failed {
        error: String,
    },
    Refused {
        blockers: Vec<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct Receipt {
    id: u64,
    parent: Option<u64>,
    op: String,
    actor: Actor,
    failure_code: Option<FailureCode>,
    before_head: String,
    before_dirty: String,
    outcome: Outcome,
    backup_refs: Vec<String>,
    recovery: Vec<RecoveryHandle>,
}

fn outcome_of(entry: &OpLogEntry) -> Outcome {
    match &entry.outcome {
        OpOutcome::Success { after } => Outcome::Success {
            head: after.head.clone(),
            dirty: after.dirty.clone(),
        },
        OpOutcome::Partial { after, error } => Outcome::Partial {
            head: after.head.clone(),
            dirty: after.dirty.clone(),
            error: error.clone(),
        },
        OpOutcome::Unknown { after, evidence } => Outcome::Unknown {
            head: after.head.clone(),
            dirty: after.dirty.clone(),
            evidence: evidence.clone(),
        },
        OpOutcome::Failed { error } => Outcome::Failed {
            error: error.clone(),
        },
        OpOutcome::Refused { blockers } => Outcome::Refused {
            blockers: blockers.clone(),
        },
    }
}

fn receipt(entry: &OpLogEntry) -> Receipt {
    Receipt {
        id: entry.id,
        parent: entry.parent,
        op: entry.op.clone(),
        actor: entry.actor,
        failure_code: entry.failure_code,
        before_head: entry.before.head.clone(),
        before_dirty: entry.before.dirty.clone(),
        outcome: outcome_of(entry),
        backup_refs: entry.backup_refs.clone(),
        recovery: entry.recovery.clone(),
    }
}

fn receipts(entries: &[OpLogEntry]) -> Vec<Receipt> {
    entries.iter().map(receipt).collect()
}

fn success(head: &str, dirty: &str) -> Outcome {
    Outcome::Success {
        head: head.to_string(),
        dirty: dirty.to_string(),
    }
}

/// A captured row with the defaults most of them share.
fn captured_row(id: u64, op: &str, actor: Actor, before_dirty: &str, outcome: Outcome) -> Receipt {
    Receipt {
        id,
        parent: id.checked_sub(1),
        op: op.to_string(),
        actor,
        failure_code: None,
        before_head: "branch: main".to_string(),
        before_dirty: before_dirty.to_string(),
        outcome,
        backup_refs: Vec::new(),
        recovery: Vec::new(),
    }
}

// ── Fixture installation ────────────────────────────────────────────────────

fn log_dir() -> PathBuf {
    PathBuf::from(std::env::var("KAGI_LOG_DIR").expect("isolated child sets KAGI_LOG_DIR"))
}

/// Copy a fixture into this child's log directory as `operations.jsonl`.
fn install(fixture: &str) -> PathBuf {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oplog")
        .join(fixture);
    let target = log_dir().join("operations.jsonl");
    std::fs::create_dir_all(log_dir()).expect("log dir");
    std::fs::copy(&source, &target).unwrap_or_else(|e| panic!("copy {}: {e}", source.display()));
    target
}

/// The 9 real operations, oldest first, exactly as the pre-migration code wrote
/// them. Written out by hand — the oracle is what a recovery consumer needs, not
/// what the serializer happens to produce.
fn captured_expectation() -> Vec<Receipt> {
    let mut rows = vec![
        captured_row(
            0,
            "create-branch",
            Actor::Cli,
            "clean",
            success("branch: main", "clean"),
        ),
        captured_row(
            1,
            "stash-push",
            Actor::Human,
            "clean",
            Outcome::Refused {
                blockers: vec!["Nothing to stash: working tree is already clean (no staged, modified, or untracked files).".to_string()],
            },
        ),
        captured_row(
            2,
            "stash-push",
            Actor::Human,
            "1 modified",
            success(
                "branch: main",
                &format!("clean; stash oid={STASH_OID}; applied=false; snapshot=none"),
            ),
        ),
        captured_row(
            3,
            "stash-drop",
            Actor::Human,
            "clean",
            success(
                "branch: main",
                &format!("stash entry deleted (oid {STASH_OID})"),
            ),
        ),
        captured_row(
            4,
            "discard",
            Actor::Mcp,
            "2 modified",
            success("branch: main", "2 file(s) discarded"),
        ),
        captured_row(
            5,
            "restore-snapshot",
            Actor::Human,
            "dirty",
            success("branch: main", "savepoint 6"),
        ),
        captured_row(
            6,
            "delete-branch",
            Actor::Cli,
            "clean",
            success(
                "branch: main",
                &format!(
                    "branch 'feature/ünicode' deleted (tip {BRANCH_TIP_OID}); restore: git branch feature/ünicode {BRANCH_TIP_REF}"
                ),
            ),
        ),
        captured_row(
            7,
            "checkout",
            Actor::Human,
            "clean",
            Outcome::Failed {
                error: "git error: Repository state changed since planning. HEAD was Attached { branch: \"main\", target: \"a574b5a97e012d4c222baaa5288568db54a687c4\" } at plan time but is now Attached { branch: \"main\", target: \"a97a6c2143c99ec32176db013978334f3e6f7672\" }. Please re-plan before proceeding.".to_string(),
            },
        ),
        captured_row(
            8,
            "create-branch",
            Actor::Human,
            "clean",
            Outcome::Partial {
                head: "branch: main".to_string(),
                dirty: "created branch 'partial-branch' @ a97a6c2143c99ec32176db013978334f3e6f7672"
                    .to_string(),
                error: "git error: checkout_tree failed: the index is locked; this might be due to a concurrent or crashed process".to_string(),
            },
        ),
    ];

    // A refused plan and a failed run both carry a machine-readable code.
    rows[1].failure_code = Some(FailureCode::Other);
    rows[7].failure_code = Some(FailureCode::Preflight);
    rows[8].failure_code = Some(FailureCode::Other);

    // Recovery material: the point of keeping these logs at all.
    rows[2].recovery = vec![RecoveryHandle::oid(recovery::STASH, STASH_OID)];
    rows[3].recovery = vec![RecoveryHandle::oid(recovery::STASH, STASH_OID)];
    rows[4].backup_refs = vec![
        DISCARD_REF_UNICODE.to_string(),
        DISCARD_REF_QUOTE.to_string(),
    ];
    rows[4].recovery = vec![
        RecoveryHandle::file(
            "naïve ✨.txt",
            DISCARD_BLOB_UNICODE,
            Some(DISCARD_REF_UNICODE.to_string()),
        ),
        RecoveryHandle::file(
            "quote\"file.txt",
            DISCARD_BLOB_QUOTE,
            Some(DISCARD_REF_QUOTE.to_string()),
        ),
    ];
    rows[5].recovery = vec![RecoveryHandle::oid(recovery::SAVEPOINT, "6")];
    rows[6].backup_refs = vec![BRANCH_TIP_REF.to_string()];
    rows[6].recovery =
        vec![RecoveryHandle::oid(recovery::BRANCH_TIP, BRANCH_TIP_OID)
            .with_reference(BRANCH_TIP_REF)];
    rows
}

// ── Tests ───────────────────────────────────────────────────────────────────

/// Every field of a real, previously written log survives a read: the id chain,
/// the actor that ran the op, the failure code, the outcome payloads and the
/// recovery handles a restore would act on.
#[test]
fn captured_log_restores_every_receipt_field() {
    if !crate::test_support::run_isolated() {
        return;
    }
    install("captured_operations.jsonl");

    let mut entries = read_oplog_tail(100);
    assert_eq!(entries.len(), 9, "every captured line must be readable");
    entries.reverse(); // oldest first, as written

    assert_eq!(receipts(&entries), captured_expectation());

    for entry in &entries {
        assert_eq!(entry.repo, CAPTURED_REPO);
        assert_eq!(entry.worktree.as_deref(), Some(CAPTURED_REPO));
        assert_eq!(entry.timestamp, CAPTURED_TIMESTAMP);
    }
}

/// Reading a real log and writing it back must not lose or alter anything a
/// consumer can observe. The comparison is between two *restored* receipts, so
/// a codec that round-trips its own mistakes cannot pass.
#[test]
fn captured_log_survives_a_write_read_cycle() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let path = install("captured_operations.jsonl");

    let mut original = read_oplog_tail(100);
    original.reverse();

    let rewritten: String = original
        .iter()
        .map(|entry| format!("{}\n", entry_to_json(entry)))
        .collect();
    std::fs::write(&path, rewritten).expect("rewrite log");

    let mut reloaded = read_oplog_tail(100);
    reloaded.reverse();

    assert_eq!(receipts(&reloaded), receipts(&original));
    assert_eq!(receipts(&reloaded), captured_expectation());
    for (before, after) in original.iter().zip(&reloaded) {
        assert_eq!(after.repo, before.repo);
        assert_eq!(after.worktree, before.worktree);
        assert_eq!(after.timestamp, before.timestamp);
    }
}

/// Outcomes a fixture repository cannot be forced into on demand, plus the
/// characters that make hand-written escaping fragile.
#[test]
fn synthetic_outcomes_restore_unknown_partial_and_escapes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    install("synthetic_outcomes.jsonl");

    let mut entries = read_oplog_tail(100);
    assert_eq!(entries.len(), 5);
    entries.reverse();

    // An unproven termination must never read back as a state observation.
    assert_eq!(
        receipt(&entries[0]),
        Receipt {
            id: 0,
            parent: None,
            op: "stash-push".to_string(),
            actor: Actor::Mcp,
            failure_code: Some(FailureCode::TerminationUnknown),
            before_head: "branch 'main' @ 0123abc".to_string(),
            before_dirty: "clean".to_string(),
            outcome: Outcome::Unknown {
                head: "unobserved".to_string(),
                dirty: "unobserved".to_string(),
                evidence: "deadline expired\u{1}; the repository was not re-read".to_string(),
            },
            backup_refs: Vec::new(),
            recovery: Vec::new(),
        }
    );

    // A partial discard keeps its prose *and* its typed per-file handle, and
    // control characters inside a backup ref survive the round trip.
    assert_eq!(
        receipt(&entries[1]),
        Receipt {
            id: 1,
            parent: Some(0),
            op: "discard".to_string(),
            actor: Actor::Human,
            failure_code: None,
            before_head: "branch 'main' @ 0123abc".to_string(),
            before_dirty: "clean".to_string(),
            outcome: Outcome::Partial {
                head: "branch 'main' @ 0123abc".to_string(),
                dirty: "1 of 2 file(s) discarded".to_string(),
                error: "could not restore\tsecond\nfile\u{7} — bell, tab and newline".to_string(),
            },
            backup_refs: vec!["refs/kagi/backups/abc\u{2}def".to_string()],
            recovery: vec![RecoveryHandle::file(
                "dir/naïve\tfile \"q\".txt",
                "1111111111111111111111111111111111111111",
                Some("refs/kagi/backups/1111111".to_string()),
            )],
        }
    );

    // Blockers keep their order and their non-ASCII content.
    assert_eq!(
        outcome_of(&entries[2]),
        Outcome::Refused {
            blockers: vec![
                "Working tree has changes: 'naïve ✨.txt'".to_string(),
                "Branch 'ünicode' does not exist".to_string(),
            ],
        }
    );
    assert_eq!(entries[2].actor, Actor::Cli);
    assert_eq!(
        entries[2].worktree.as_deref(),
        Some(format!("{SYNTHETIC_REPO}/linked-worktree").as_str()),
        "an entry recorded from a linked worktree keeps that scope"
    );

    // Every recovery kind, in order, next to the mandatory roots.
    assert_eq!(
        entries[3].recovery,
        vec![
            RecoveryHandle::oid(
                recovery::SAVEPOINT,
                "2222222222222222222222222222222222222222"
            ),
            RecoveryHandle::oid(recovery::STASH, "3333333333333333333333333333333333333333"),
            RecoveryHandle::oid(
                recovery::BRANCH_TIP,
                "4444444444444444444444444444444444444444"
            )
            .with_reference("refs/kagi/backups/branch-tip"),
            RecoveryHandle::oid(
                recovery::HISTORY_FROM,
                "5555555555555555555555555555555555555555"
            ),
            RecoveryHandle::oid(
                recovery::HISTORY_TO,
                "6666666666666666666666666666666666666666"
            ),
        ]
    );
    assert_eq!(
        entries[3].backup_refs,
        vec![
            "refs/kagi/backups/one".to_string(),
            "refs/kagi/backups/two ✨".to_string()
        ]
    );
    assert_eq!(entries[3].failure_code, Some(FailureCode::Preflight));

    // Quote, backslash, tab, newline, carriage return and a bare control char.
    assert_eq!(
        outcome_of(&entries[4]),
        success(
            "branch 'main' @ 9f9f9f9",
            "quote \" backslash \\ tab \t newline \n return \r control \u{1f}",
        )
    );
}

/// Legacy, malformed and hostile lines: a bad line loses only itself, and the
/// pre-ADR-0149 identity reconstruction still holds around it.
#[test]
fn legacy_and_malformed_lines_keep_their_valid_neighbours() {
    if !crate::test_support::run_isolated() {
        return;
    }
    install("legacy_and_edge.jsonl");

    let mut entries = read_oplog_tail(100);
    entries.reverse();

    let ops: Vec<&str> = entries.iter().map(|e| e.op.as_str()).collect();
    assert_eq!(
        ops,
        vec![
            "legacy-checkout-a",
            "legacy-stash-push-b",
            "future-fields-checkout",
            "legacy-discard-c",
            "surrogate-commit",
            "string-scalars-branch",
            "invalid-scalars-discard",
        ],
        "garbage, a truncated write, a wrong-typed backup_refs, an unknown \
         outcome kind and an evidence-less Unknown are each dropped alone"
    );

    // Legacy id-less lines take their identity from their position among the
    // readable lines; explicit ids are never renumbered.
    let identity: Vec<(u64, Option<u64>)> = entries.iter().map(|e| (e.id, e.parent)).collect();
    assert_eq!(
        identity,
        vec![
            (0, None),
            (1, Some(0)),
            (7, Some(1)),
            (3, Some(7)),
            (10, Some(9)),
            (12, Some(11)),
            (0, None),
        ]
    );

    // A pre-ADR-0149 line has no actor, no worktree and no recovery — and is
    // never retroactively claimed to be recoverable.
    let legacy = &entries[0];
    assert_eq!(legacy.actor, Actor::Human);
    assert_eq!(legacy.worktree, None);
    assert!(legacy.recovery.is_empty());
    assert!(legacy.backup_refs.is_empty());
    assert_eq!(
        outcome_of(&entries[1]),
        Outcome::Refused {
            blockers: vec!["Nothing to stash".to_string()],
        },
        "a legacy line keeps its full outcome payload"
    );

    // Fields written by a newer Kagi — including a nested object that repeats
    // `op`, `id` and `outcome` — must not shift the real values.
    let future = &entries[2];
    assert_eq!(future.id, 7);
    assert_eq!(
        outcome_of(future),
        success("branch 'main' @ 0123abc", "clean")
    );
    assert_eq!(future.worktree.as_deref(), Some(SYNTHETIC_REPO));

    // `\uXXXX` surrogate pairs and control escapes decode; recovery elements
    // that do not fit the shape are dropped one by one, not wholesale.
    let surrogate = &entries[4];
    assert_eq!(surrogate.before.head, "branch 'feature/🎋' @ 7f7f7f7");
    assert_eq!(
        outcome_of(surrogate),
        success("branch 'main' @ 0123abc", "control\u{1}char")
    );
    assert_eq!(
        surrogate.recovery,
        vec![RecoveryHandle::file(
            "src/🎋.rs",
            "7777777777777777777777777777777777777777",
            None,
        )],
        "the one well-formed handle survives its malformed siblings"
    );

    // Scalars written as strings, an actor from the future, "null" as text and
    // a null failure code.
    let strings = &entries[5];
    assert_eq!((strings.id, strings.parent), (12, Some(11)));
    assert_eq!(
        strings.actor,
        Actor::Human,
        "an unknown actor falls back to human, never to a wrong one"
    );
    assert_eq!(strings.worktree, None);
    assert_eq!(strings.failure_code, Some(FailureCode::Other));

    // Unparsable scalars and missing optional strings degrade to defaults
    // instead of dropping an entry that still says what happened.
    let invalid = &entries[6];
    assert_eq!((invalid.id, invalid.parent), (0, None));
    assert_eq!(invalid.worktree.as_deref(), Some("12345"));
    assert_eq!(invalid.before.dirty, "");
    assert_eq!(
        outcome_of(invalid),
        Outcome::Failed {
            error: String::new()
        }
    );
}

/// The bounded tail read returns the newest entries, with their payloads intact.
#[test]
fn tail_limit_returns_the_newest_captured_entries() {
    if !crate::test_support::run_isolated() {
        return;
    }
    install("captured_operations.jsonl");

    let entries = read_oplog_tail(3);
    let ids: Vec<u64> = entries.iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![8, 7, 6], "newest first, limited to n");
    assert_eq!(entries[2].recovery.len(), 1, "payloads are not truncated");
    assert_eq!(read_oplog_tail(0).len(), 0);
}

/// A new append onto an existing log continues that log's chain, and the
/// receipt the writer returns is the entry the file yields back.
#[test]
fn append_onto_a_captured_log_continues_the_chain() {
    if !crate::test_support::run_isolated() {
        return;
    }
    install("captured_operations.jsonl");

    let mut entry = OpLogEntry::new(
        "cherry-pick",
        format!("{SYNTHETIC_REPO}/appended"),
        StateSummary {
            head: "branch 'main' @ 0123abc".to_string(),
            dirty: "1 modified \"file\"\twith\ttabs".to_string(),
        },
        OpOutcome::Partial {
            after: StateSummary {
                head: "branch 'main' @ 4567def".to_string(),
                dirty: "picked 1 of 2 ✨".to_string(),
            },
            error: "second commit conflicted".to_string(),
        },
    )
    .with_actor(Actor::Mcp)
    .with_worktree(Some(format!("{SYNTHETIC_REPO}/appended")));
    entry.recovery = vec![RecoveryHandle::file(
        "src/naïve ✨.rs",
        "9999999999999999999999999999999999999999",
        None,
    )];
    entry.failure_code = Some(FailureCode::StashIdentityUnverified);

    let (_, written) = append_oplog_receipt(&entry).expect("append");
    assert_eq!(
        (written.id, written.parent),
        (9, Some(8)),
        "the new entry chains onto the captured tail"
    );

    let tail = read_oplog_tail(1);
    assert_eq!(receipts(&tail), vec![receipt(&written)]);
    assert_eq!(tail[0].repo, written.repo);
    assert_eq!(tail[0].worktree, written.worktree);
    assert_eq!(tail[0].timestamp, written.timestamp);
    let mut all_entries = read_oplog_tail(100);
    all_entries.reverse();
    let mut expected = captured_expectation();
    expected.push(receipt(&written));
    assert_eq!(
        receipts(&all_entries),
        expected,
        "older receipts stay intact"
    );
}

#[test]
fn appended_control_characters_survive_the_public_reader() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // In particular, serde emits short escapes for backspace/form-feed. The
    // old scanner preserved those as literal backslash+b/f after canonicalizing.
    let controls: String = (0u8..32).map(char::from).collect();
    let text = format!("{controls}\u{1f38b} \"quoted\" \\");
    let entry = OpLogEntry::new(
        "control-character-receipt",
        SYNTHETIC_REPO,
        StateSummary {
            head: text.clone(),
            dirty: String::new(),
        },
        OpOutcome::Unknown {
            after: StateSummary {
                head: "main".into(),
                dirty: text.clone(),
            },
            evidence: text,
        },
    );
    append_oplog_receipt(&entry).expect("persist control characters");
    let restored = read_oplog_tail(1);
    assert_eq!(receipts(&restored), vec![receipt(&entry)]);
    assert_eq!(restored[0].repo, entry.repo);
    assert_eq!(restored[0].timestamp, entry.timestamp);
}

#[path = "support/isolated.rs"]
mod test_support;
