# ADR-0187: Typed oplog recovery handles

- Status: Implemented for review
- Issue: #500
- Related: [ADR-0179](0179-ref-backed-discard-remove-backups.md),
  [ADR-0177](0177-transport-recording-boundary.md),
  [ADR-0149](0149-oplog-in-backend-run-and-schema.md),
  [ADR-0074](0074-operation-log.md)

## Decision

The recovery material an operation produces — a pre-restore savepoint OID, a
dropped stash OID, a branch tip, a path→blob backup map — is recorded in its own
additive `OpLogEntry.recovery` array of `RecoveryHandle { kind, oid, path,
reference }`. It used to exist only inside the English `after.dirty` sentence
(`savepoint <oid>`, `stash entry deleted (oid <oid>)`, `discarded N file(s);
backup: <path>=<blob>`), so any consumer that wanted to *use* it had to parse
display text.

This follows the `backup_refs` precedent (ADR-0179) exactly rather than
inventing a second shape: one new top-level field, written by the same
hand-rolled serializer, read by the same reader, invisible to older code.

Every value in the array is a JSON string, so a path containing `,`, `=` or
non-ASCII is unambiguous — the comma-joined `path=blob` summary was not.

`kind` is a plain string for the same reason `op` is: a tag written by a newer
Kagi must round-trip through an older reader instead of collapsing into a wrong
variant. The kinds are constants in `crates/kagi-git/src/oplog/recovery.rs`.

### Which family gets which handle

| Family | Writer | Handles |
|---|---|---|
| snapshot restore | `backend::recording::recovery_handles` via `Backend::run_recorded` | `savepoint` |
| stash drop (local) | same | `stash` |
| stash drop (SSH) | `src/remote/stash.rs`, ADR-0177 boundary | `stash` (the approved OID) |
| discard, incl. PARTIAL | same | one `file-backup` per path (`path` + blob + its ADR-0179 ref) |
| apply suggestion | same | one `file-backup` (no ref: the blob is not pinned) |
| delete branch | same | `branch-tip` + its recovery ref |
| history move (undo/redo) | `Backend::run_history_move` | `history-from` + `history-to` |
| remove worktree | `backend::remove` | one `file-backup` per backed-up path, plus `branch-tip` |

## What did not change

The `after.dirty` / `before.dirty` summaries are byte-identical, including the
persisted discard after-state and the `stage=…; backup: …; branch_tip=…` remove
prose. They are presentation, and the op-log panel still renders them. No
`[kagi]` contract line changed — `executed: discarded …` and `backup refs: …`
keep their wording and order. No `git2` reached `src/ui/`.

A **failed** attempt contributes no handles. Its recovery roots, when it has
any, are the mandatory `backup_refs` written by ADR-0179; claiming more would be
a claim the executor never made.

## Legacy entries

A line written before this field reads back with `recovery` empty. The prose is
never mined for a substitute: an old entry is not retroactively recoverable, and
saying so would be worse than saying nothing. A `recovery` value that does not
fit the shape (wrong type, an element without `oid`) also degrades to empty
rather than dropping the whole entry — the entry's other fields still matter.

Retirement (`execute_forget_oplog_entry`) keeps preserving unknown JSON fields
on retained lines verbatim, so a field a newer Kagi writes survives an older
one's read-write cycle unchanged.

## Consumers

`Backend::run` remains the sole persisted writer, so consumers read the
**persisted receipt**, not the presentation-only in-memory panel entry. The GUI
E2E discard scenario now looks up its `f.txt` backup through
`receipt.recovery`, where it used to split `backup: ` out of `after.dirty`;
`ui::e2e::entry_after_dirty` existed only for that parse and is deleted.

## Evidence

- `crates/kagi-git/src/oplog_tests.rs`: round trip with a path containing `,`,
  `=` and non-ASCII; a prose-only legacy line reads with no typed data; a
  malformed `recovery` degrades to empty without losing the entry.
- `tests/oplog_backend_run_test.rs`: the outcome → handle mapping for every
  family, including a PARTIAL discard and an `Err` (which claims nothing).
- `crates/kagi-git/tests/ref_backups_test.rs`: a real discard and a real
  worktree removal (Success / Partial / Unknown) write the typed map next to an
  unchanged summary; the legacy-retirement fixture asserts stripped lines carry
  no typed data and that an unknown field survives retirement.
- `tests/oplog_nonrun_ops_test.rs`: an undo receipt carries `history-from` /
  `history-to` alongside the unchanged "moved from … to …" sentence.
