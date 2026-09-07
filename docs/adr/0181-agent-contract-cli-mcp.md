# ADR-0181: one agent contract for CLI and MCP, bound to its own receipt

- Status: Implemented for review
- Issues: [#509](https://github.com/TomiXRM/kagi/issues/509),
  [#505](https://github.com/TomiXRM/kagi/issues/505)
- Related: ADR-0104 (plan → confirm), [ADR-0149](0149-oplog-in-backend-run-and-schema.md),
  [ADR-0163](0163-kagi-mcp-server.md), [ADR-0177](0177-transport-recording-boundary.md),
  [ADR-0178](0178-backend-execution-policy.md), #330 / #331 / #421

## Decision

`kagi_git::api` is the single agent-facing contract. It owns:

- `SUPPORTED_OPS` and `OPS_USAGE` — the op set and the one string that
  advertises it (CLI usage text *and* the MCP `kagi_plan` tool description),
- `resolve_operation` — op name + positional args → `Operation`, including the
  argument-count and unsupported-op error wording,
- `plan_envelope` / `plan_body` — the plan JSON,
- `confirm_response` — the execute result.

`src/cli_main.rs` keeps argv, `--yes`, stdin/stdout and exit codes.
`crates/kagi-mcp` keeps the tool envelope, the server-side plan store, the
two-stage host approval and read-only mode. Neither owns a second operation
match or a second plan serializer, so adding an operation edits one match and
one serializer. The op set is now a compile-time re-export in `write.rs`, not a
copy.

The module lives in `kagi-git`, not `kagi-domain`: resolution needs a `Backend`
(to default `create-branch`'s start point to HEAD) and the JSON needs `serde`.
`kagi-domain` stays dependency-free (CLAUDE.md invariant #2); `kagi-git` already
depends on `serde_json`. `kagi-git` is also the only crate both frontends share
— `src/app` is inside the bin and unreachable from `kagi-mcp`.
No new crate, and no general command registry.

## Confirm is bound to its own oplog entry (#505)

Both frontends used to run `Backend::run` and then read `read_oplog_tail(1)`,
calling whatever sat at the tail of the process-global log "the entry we just
wrote". Any writer that appended in that window — another repo, another
operation, another process — was echoed back as this operation's record. #421
scoped ordinary oplog reads to the bound repo, but this confirm echo stayed
global, and a repo+op filter still cannot separate two concurrent runs of the
same operation in the same repository.

Both now call `Backend::run_recorded` (ADR-0177's boundary, already returning
`RunReport { result, recording, .. }`) and build the response from that
invocation's `Recording`. No frontend reads the oplog to describe its own write.

`Recording::Failed` is reported as a recording failure, not substituted with an
older entry: the response keeps `status: "ok"` (the mutation happened),
`recorded: false`, a `recording_error` string, and the *attempted* entry under
`oplog`. "Changed" and "recorded" stay separately observable, matching
ADR-0177's `Success + Recording::Failed` rule.

An execution `Err` is also serialized from the complete `RunReport`, rather
than returned before `confirm_response`: its response keeps `status: "error"`
and the execution error while still carrying this attempt's `oplog`,
`backup_refs`, `recorded` and `recording_error`. A recorded `Partial` outcome is
likewise always an error response, even if the domain result itself is `Ok`.
This is essential for operations that create a recovery ref before a later
step fails. CLI exits 1; MCP marks the same structured response `isError: true`
without replacing it by an error-only object.

## Contract shape

`plan` (both frontends): `plan_id`, `op`, `args`, `head_at_plan`,
`stash_count_at_plan`, `worktree_digest`, `plan { title, current, predicted,
warnings, blockers, recovery, disposition, destructive }`. MCP adds only its own
`next` hint. `confirm` (both frontends): `status`, `op`, `plan_id`, `outcome`,
`error`, `oplog`, `recorded`, `recording_error`.

`recorded` and `recording_error` are new; `oplog` is unchanged in shape and now
always this run's entry. The CLI's exit codes, `--yes` gate and refusal JSON, and
MCP's stale/blocker `isError` responses, plan-store consumption and actor tags
are unchanged. The schema remains UNSTABLE/internal for v1 (`docs/plan-json.md`).

## Evidence

`crates/kagi-git/tests/agent_contract_test.rs`:

- `confirm_response_names_this_runs_entry_not_the_global_tail` — a foreign entry
  is appended *after* the run and before the response is built; the response
  still names this run's entry and its id differs from the global tail's.
- `same_repo_same_op_runs_report_their_own_entries` — two `create-branch` runs in
  one repo report different entry ids, which a repo+op filter could not do.
- `a_failed_append_is_reported_as_unrecorded_not_as_an_older_entry` — a readable
  but unwritable log file: the branch is still created, `recorded` is false, the
  error is named, and the reported entry is this run's attempt, never the stale
  tail. Unix-only; skips (loudly) if the filesystem does not enforce the bit.
- `resolve_operation_covers_exactly_the_advertised_ops` — every advertised op
  resolves, appears in `OPS_USAGE`, and refuses a missing argument identically.

`tests/cli_mcp_contract_test.rs` drives both entry points against one fixture —
the CLI by spawning the real binary, MCP in-process through its JSON-RPC handler:

- `cli_and_mcp_emit_the_same_plan_for_every_supported_op` — identical `plan_id`,
  staleness snapshot and display block (blockers, recovery, destructive) for
  every op in `SUPPORTED_OPS`; only MCP's `next` differs.
- `cli_and_mcp_reject_the_same_unsupported_and_underspecified_requests` — an
  unknown op and a missing argument are refused with the same words on both.
- `confirm_reports_its_own_oplog_entry_on_both_frontends` — both confirms carry
  the same field set with their own actor, and two same-op runs in one repo get
  distinct entry ids.

## Not covered

Multi-process oplog id assignment is still an unlocked read→write window
(`append_oplog_receipt`); binding the response to the receipt does not fix
concurrent numbering, it only stops a frontend from *reading back* someone
else's entry. The plan JSON stays hand-built rather than serde-derived, since
`kagi-domain` must stay dependency-free. Splitting pure argument mapping from
the Backend-dependent HEAD default was considered and skipped: one function for
five operations, with the only Backend touch being `create-branch`'s start
point. Revisit if resolution grows repository lookups per operation.
