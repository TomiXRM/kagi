# ADR-0163: `kagi-mcp` — Kagi's git `Backend` as an MCP server (no destructive tools)

- Status: **Accepted**
- Date: 2026-09-04
- Builds on: ADR-0104 (enforced `Backend::run` pipeline), ADR-0149 (oplog in `run`, actor field), ADR-0072/0078 (crate layering + grep gates)
- Related: #331 (this crate), #330 (`kagi plan/confirm` CLI — plan-id plumbing reused), #329 (oplog recording), #332 (annotation mapping)

## Context

"A git MCP server on which destructive operations do not exist" is an empty
market slot. Every general git MCP server exposes the raw porcelain, so an agent
behind it can `push --force` or `reset --hard`. Kagi's whole reason to exist is
that those operations are absent — and #330/#329 already turned the safety
pipeline (`plan → preflight → execute → verify → oplog`) into a headless,
GUI-independent path (`Backend::run`, tagged by `Actor`). Exposing that path over
MCP is a thin, high-value wrapper: agents touch git *through* Kagi's guardrails.

## Decision

A new crate `crates/kagi-mcp/` implements an MCP server over stdio JSON-RPC 2.0.

**Layering.** `kagi-mcp → kagi-git → kagi-domain`. The crate does **not** depend
on `gpui` (so it runs headless / in CI) and never touches `git2` directly — all
git access is through `kagi_git::Backend`. Both are enforced: a CI grep gate
(`invariant-mcp-no-gpui`, same shape as ADR-0078's `src/ui` git2 gate) and a
`#[test]` scanning the crate. Serde/JSON lives only in this crate; `kagi-domain`
stays pure (CLAUDE.md #2), exactly as #330 kept serde in `cli_main.rs`.

**No MCP SDK.** The JSON-RPC framing is hand-rolled (newline-delimited JSON on
stdin/stdout) so the safety surface stays auditable and adds no heavy dependency.
`Server::handle(&Value) -> Option<Value>` is a pure request→response function,
unit-tested with in-memory requests.

**Repo fixed at startup.** `kagi-mcp stdio --repo <path>`. Tools take no
`repo_path` argument, so an agent cannot reach another repository (PM-locked §5).
Multi-repo is a documented follow-up.

**Read tools** (all `readOnlyHint: true`, side-effect-free, derived from
`Backend::snapshot` / `read_oplog_tail`): `kagi_repo_status`, `kagi_graph`,
`kagi_diff`, `kagi_commit_show`, `kagi_branches`, `kagi_worktrees`,
`kagi_conflicts`, `kagi_stashes`, `kagi_oplog`. (`kagi_blame` is deferred — it
depends on #350.)

**Write tools = two stages** (PM-locked §5 — the plan→confirm split IS the
approval; not per-op tools):

- `kagi_plan(op, args) -> OperationPlan` — `readOnlyHint: true`. Builds a
  side-effect-free plan (with a content-hash `plan_id`, reusing #330), stores
  `plan_id → (op, args)` in the server's in-memory map, and returns the plan
  (warnings, blockers, `destructive`). Supported ops: `checkout`,
  `create-branch`, `delete-branch`, `discard`, `reset` (soft/mixed) — the exact
  set `cli_main::build_operation` supports.
- `kagi_confirm(plan_id) -> outcome` — `destructiveHint: true, openWorldHint:
  true`. This is the single tool the host (Codex / Claude Code) prompts on;
  calling it IS the second confirmation, so there is no `--yes` gate (unlike the
  CLI). It re-plans against the repo *now*, refuses on a stale `plan_id` (repo
  moved / TOCTOU) or on blockers, then runs the op through `Backend::run` with
  `Actor::Mcp` — so **every agent write lands in the oplog** (#329/#149) and is
  returned to the caller.

**Approval subject = MCP annotations, host approves** (PM-locked §5). The
annotation mapping mirrors #332. A GUI-modal approval (via `single_instance`) is
a documented follow-up — kept out so the server runs headless.

**Output.** Each `tools/call` result carries both `structuredContent` (the
machine value) and a `text` block with the same JSON (universal fallback). No
per-tool `outputSchema` in v1 — the spec permits `structuredContent` without one
and hand-authoring output schemas is high-volume, low-value; add if a host
validates against it.

**Intentionally absent.** No force-push, no `reset --hard`, no `git clean` tool
exists — not an omission, the product thesis. `tools/list` descriptions and the
`initialize.instructions` say so, so an agent understands "Kagi cannot do that".

## Read-only mode + annotation derivation (#332)

**Annotations are derived, not hand-tabled.** `tools::derive_annotations(
read_only, destructive, network)` is the only place annotation JSON is built.
Read tools and `kagi_plan` are `readOnlyHint: true` (`kagi_diff` included —
diff-cache warming is an internal optimization, not an observable side effect).
`kagi_confirm`'s `destructiveHint` is the fold of `OperationPlan.destructive`
(ADR-0004/0023) over `write::SUPPORTED_OPS`; `openWorldHint` follows op kind
and derives to `false` because no network op (fetch/push) is exposed yet. The
fold is a pinned const (`tools/list` has no repo to plan against), but the
`tools_list_annotations_derive_from_plan_classification` test rebuilds it from
real `OperationPlan`s in a temp repo — the advertised hints cannot drift from
the classification.

**Read-only mode.** `kagi-mcp stdio --repo <path> --readonly` (or env
`KAGI_MCP_READONLY=1|true`; the CLI flag wins over any other source) removes
`kagi_confirm` from `tools/list` entirely — the tool does not exist, rather
than existing-but-refusing — and calling it returns JSON-RPC method-not-found
(`-32601`). `kagi_plan` stays: planning is pure inspection. The mode is fixed
at startup, so no `notifications/tools/list_changed` is needed; a runtime
toggle (e.g. from the GUI) is an optional follow-up that would require sending
that notification.

## Consequences

- Agents get a safe git surface with zero new destructive capability; the oplog
  guarantee holds for agent writes because they route through the one true path.
- Parallel writes serialize through `Backend::run`; concurrent-write `index.lock`
  behaviour on a shared worktree is **untested** and left as a follow-up (no
  locking speculation added here).
- Manual verification (connecting a real Codex / Claude Code host) is still
  required — the automated tests cover the JSON-RPC handler, not a live host.
