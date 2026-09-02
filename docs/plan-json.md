# `kagi` CLI JSON schema (#330)

> **UNSTABLE / internal — v1.** The shapes below are produced by hand in the bin
> crate (`src/cli_main.rs`) from the domain types' public fields and their
> `message_en()` renderers. `kagi-domain` stays **dependency-free** (no serde) —
> JSON lives only at the CLI edge, matching the existing hand-rolled-JSON pattern
> in `oplog.rs` / `resolution.rs` / `drafts.rs`. These shapes are **not a stable
> public API yet**: field names can change between Kagi versions without notice.
> #331 (MCP server) is expected to be the first frozen surface, layered on this.

The headless CLI lets an agent drive Kagi's safety pipeline
(`plan → confirm → preflight → execute → verify → oplog`) from outside the GUI.
It lives on the normal `kagi` binary: when `argv[1]` is a known subcommand the
process runs headless and exits; otherwise it launches the GUI.

```
kagi plan <op> [args...] [--repo PATH] [--json]
kagi confirm [--yes] [--plan FILE] [--repo PATH] [--json]   # plan JSON on stdin if no --plan
kagi status [--repo PATH] [--json]
kagi oplog [--limit N] [--repo PATH] [--json]
```

- `--repo PATH` selects the repository (default: current directory; the git root
  is discovered by walking up, like `git`).
- `--json` is accepted everywhere; JSON is the only output format in v1.
- Exit codes: `0` ok · `1` usage/error · `2` refused (blockers / stale / needs `--yes`).

## Supported operations (v1)

| CLI form | `Operation` |
|---|---|
| `checkout <branch>` | `Checkout { branch }` |
| `create-branch <name> [at-commit]` | `CreateBranch { name, at }` (defaults to HEAD) |
| `delete-branch <name>` | `DeleteBranch { name }` |
| `discard <path...>` | `Discard { paths }` (destructive) |
| `reset <commit>` | `ResetCurrentToHead { target }` (destructive) |

More operations are a matter of adding a match arm in `src/cli_main.rs`
(`build_operation`); the plan/confirm machinery is operation-agnostic.

## `plan` — the envelope

`kagi plan …` prints a side-effect-free envelope (it never touches the repo). The
**top-level** fields are what `confirm` reads back; the nested `plan` object is a
human-readable display block for the agent (`confirm` ignores it):

```jsonc
{
  "plan_id": "ee04a5630ae2e617",             // content hash — see below
  "op": "checkout",                           // CLI op name (rebuilds the Operation)
  "args": ["feature"],                        // CLI positional args
  "head_at_plan": "branch: main @ 1a2b3c4d",  // staleness snapshot (primitives)…
  "stash_count_at_plan": 0,
  "worktree_digest": null,                    // u64, or null when op ignores the tree
  "plan": {                                   // human display (message_en strings)
    "title":     "Switch to 'feature'",
    "current":   { "head": "branch: main",    "dirty": "clean" },
    "predicted": { "head": "branch: feature", "dirty": "clean" },
    "warnings":  [],                          // rendered strings
    "blockers":  [],                          // non-empty ⇒ confirm refuses
    "recovery":  null,                        // rendered string, or null
    "disposition": "Ready",                   // Debug of PlanDisposition
    "destructive": false                      // true ⇒ confirm needs --yes
  }
}
```

`confirm` deserializes only `{ plan_id, op, args, head_at_plan,
stash_count_at_plan, worktree_digest }` — never the `plan` tree. It re-plans from
`op`+`args` and compares `plan_id`; the staleness snapshot is used only to name
*what* changed on a mismatch.

## `plan_id` — the content hash (staleness detection)

`plan_id` is a deterministic hash of exactly the inputs whose change would
invalidate the plan (`OperationPlan::plan_id`, computed in `kagi-domain` with
`std` only — no serde):

- the **operation identity** (the title, which encodes op kind + targets),
- **HEAD including its target SHA** (so a new commit shifts the id even on the
  same branch),
- the **stash count**, the **worktree classification digest** (ADR-0147), and
  the **destructive** flag.

There is no server-side state: `confirm` recomputes the plan against the repo as
it is *now* and compares. Same idea as ADR-0147's worktree digest, applied to
the whole plan. (v1 uses `DefaultHasher` — stable across processes on one target;
not a cross-version stable id.)

## `confirm`

`confirm` reads the envelope back (from `--plan FILE` or stdin), rebuilds the
operation from `op`+`args`, **re-plans**, and gates execution:

1. **stale?** recomputed `plan_id` ≠ the envelope's → refuse; `detail.changed`
   names what moved (HEAD / stash / working tree), comparing the envelope's
   snapshot against the fresh plan.
2. **blocked?** the fresh plan has blockers → refuse; `detail.blockers` lists them.
3. **destructive without `--yes`?** → refuse.
4. otherwise run through `Backend::run` (actor = `cli`) and print the result.

Success:

```jsonc
{ "status": "ok", "op": "checkout", "plan_id": "…",
  "outcome": "Unit",                 // Debug of OperationOutcome
  "oplog": { /* the OpLogEntry just written */ } }
```

Refusal (exit 2):

```jsonc
{ "status": "refused",
  "reason": "repo changed since plan — re-plan and try again",
  "detail": { "changed": ["HEAD changed (was branch: main @ 1a2b3c4d, now branch: main @ 9f8e7d6c)"],
              "expected_plan_id": "…", "actual_plan_id": "…" } }
```

Error (exit 1): `{ "status": "error", "error": "…" }`.

## `status`

`{ "head": "branch: main", "dirty": "clean" }`.

## `oplog`

A JSON array of the newest `--limit N` (default 20) operation-log entries, each
in the same shape the oplog file stores (ADR-0149 / #329): `id`, `parent`,
`timestamp`, `op`, `repo`, `actor`, `worktree`, `before`, `outcome`.
