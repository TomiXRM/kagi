# ADR-0149: Oplog recording in `Backend::run` + schema fields (id/parent/actor/worktree)

- Status: **Accepted**
- Date: 2026-09-03
- Addendum to: ADR-0104 (enforced operation pipeline), ADR-0074 (oplog format v2)
- Related: ADR-0084 (reflog-backed undo), #329 (recording location), #333 (schema)

## Context

Two coupled gaps, tackled together because they touch the same write path and
the same on-disk file.

**#329 — recording location.** ADR-0104 made `Backend::run` the enforced entry
point for every mutating operation (preflight always runs), but its Consequences
noted that *oplog recording stayed outside `run`*. The actual writer was the UI's
`record_op` (`src/ui/mod.rs`). This is invisible today because the GUI is the
only write path — but the moment an MCP server (#331) or a `kagi` CLI (#330)
calls `Backend::run` directly, those writes would not be logged. "Every
operation goes through the oplog so you can rewind time" is the product's central
promise; letting it depend on the *caller* breaks that promise structurally.

**#333 — schema.** `OpLogEntry` carried only `timestamp / op / repo / before /
outcome`. Without a stable id there is no total order within a single wall-clock
second; without a parent link the log cannot be walked as a chain; without an
actor an agent write is indistinguishable from a human one; without a worktree
the history cannot be sliced per worktree. Schema changes get more expensive the
more log lines exist, so the fields are added now even though the features that
consume them are later work.

## Decision

### Recording moves into `Backend::run` (#329)

- `run` writes **exactly one** oplog entry per op, synchronously, before it
  returns. `before` comes from `plan.current`; the `after` state for
  Success/Partial comes from `plan.predicted`; the op name from a new
  `Operation::oplog_name()` (kagi-domain). The result→outcome mapping is the
  pure, `pub`, unit-testable free fn `oplog_outcome_from`.
- A partially-applied discard (`DiscardOutcome::is_partial()`, #281) is recorded
  as `OpOutcome::Partial`. A preflight refusal (an `Err` before dispatch) is
  recorded as `OpOutcome::Failed`, then propagated — no write path has an
  unlogged hole.
- The UI's `record_op` **no longer writes the oplog** for run-path outcomes, so
  the GUI produces exactly one entry per op (no double-record). It keeps the
  UI-only work: toast, footer, and the in-memory history-panel push.

### Actor threading (#329)

- `Backend` gains an `actor: Actor` field (default `Human`) and `set_actor`.
  `run` stamps `self.actor` on every entry. The GUI leaves the default; the
  future MCP/CLI front-ends call `set_actor(Actor::Mcp | Actor::Cli)`.

### Refusals and remaining non-run owners

- `Refused` (plan has blockers) never reaches `run` — it is rejected at plan
  time. The UI remains its recorder: `record_op` still appends `Refused`.
- Remaining non-run families (conflict resolution, terminal start and PR
  merge) still use `record_op_persist`. They are not migrated by this recovery.
- **2026-09-06 recovery:** stash drop now uses `Backend::run`, including stash
  list/HEAD preflight, trust and one durable full-OID result; it remains exempt
  from automatic snapshots because dropping a stash must not create a stash.
- History undo/redo uses `Backend::run_history_move`: preflight, the existing
  ref-only executor, then recording before return. Both full OIDs are kept;
  the index and working tree are not rewritten.
- Cleanup records in `Backend::execute_delete_merged_branches`. A failure to
  open that backend is recorded by the background caller, before the UI
  lifetime boundary. Remote stash drop likewise records at its transport
  boundary, preserving the recovery OID from Git's actual output.
- UI completion only presents these results. Cleanup uses the same busy,
  panic and stale-tab guard as other async operations; its confirmed modal
  closes before dispatch. Dropping a stale callback cannot discard a record.
- `GitError::Preflight` preserves the underlying Display text while carrying
  the failure stage. History/drop retain their existing localized preflight
  labels without duplicating preflight or parsing error strings.
- Recording still uses the existing append/error policy and schema; this does
  not make failed filesystem writes durable. Other non-run owners remain a
  separate migration, not a second implementation of this boundary.

### Schema fields (#333)

`OpLogEntry` gains, serialized in this order ahead of the existing fields:

| Field | Type | Notes |
|---|---|---|
| `id` | `u64` | Monotonic sequence, assigned at append time. Total order incl. same second. **Sequence, not ULID** — no new dependency, deterministic, testable. |
| `parent` | `Option<u64>` | Previous entry's id; `None` for the first. Serialized as JSON `null` when absent. |
| `actor` | `Actor` (`human`/`mcp`/`cli`) | Defaults to `Human`. |
| `worktree` | `Option<String>` | Worktree path the op ran in; `null` when absent. |

- `append_oplog` assigns `id`/`parent` from the file's current tail
  (`id = last.id + 1`, `parent = Some(last.id)`; `0`/`None` for an empty file),
  overwriting the placeholders `OpLogEntry::new` leaves.
- **Back-compat.** A pre-ADR-0149 line (missing the four fields) still parses:
  `read_oplog_tail` reconstructs `id` from the **0-based index** of the entry in
  the file and `parent` from the previous entry's id; `actor` defaults to
  `Human`; `worktree` to `None`. Verified by a golden test mixing an OLD-format
  and a NEW-format line in one file (`tests/oplog_backend_run_test.rs`).

## Explicitly deferred (do NOT implement here)

### Slice 1a addendum (#484)

[ADR-0175](0175-app-remove-boundary.md) supersedes the UI-owned non-run writer
only for worktree remove: recording is now in its Backend execution boundary,
including open failure and refusal. `append_oplog_receipt` returns the assigned
entry and the old append API delegates to it. `Unknown { after, evidence }` is
an additive variant; old variant wire encodings are unchanged. Remove uses
observed progress/verification and full recovery OIDs, not predicted after.
This does not supply crash durability, multi-process locking or GC retention.

### Remaining deferrals

- **Snapshot / checkpoint strategy** — no `snapshot` field, no ref-set capture.
  A separate future issue owns point-in-time restore.
- **Per-repo oplog files** — the single `~/.kagi/operations.jsonl` stays; no
  per-repo split.
- **`Refused` recording redesign** and bringing the remaining non-run
  subsystems (conflict / terminal / PR merge) under `Backend::run`.

## Consequences

- Every `Backend::run` caller — GUI, headless, tests, and future MCP/CLI —
  records to the oplog identically. The product promise no longer depends on the
  caller.
- The recorded `after` state for run-path ops is now the plan's *prediction*
  rather than the UI's post-execute verify snapshot. For the oplog's
  `{head, dirty}` summary this is accurate for these ops; the richer verify
  strings remain in the toast/footer.
- ADR-0084 undo/redo reads the oplog tail unchanged (it consumes `before` and
  `outcome`, which are untouched); the new fields are additive. The end-to-end
  undo flow is covered by the native main-thread runner for fixture-only
  success, stale-head refusal and EN/JA preflight presentation.
- One-per-op holds structurally for run-path ops (run is the sole writer). If a
  new non-run mutating subsystem is added and forgets `record_op_persist`, it
  loses log coverage (never a double-record) — a known, bounded risk until those
  subsystems join the pipeline.
