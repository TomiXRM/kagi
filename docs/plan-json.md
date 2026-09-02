# `kagi` CLI JSON schema (#330)

> **UNSTABLE / internal — v1.** The shapes below are derived directly from the
> in-repo Rust types (`serde` derive on `kagi_domain::OperationPlan` /
> `Operation`, ADR-0129). They are **not a stable public API yet**: field names
> and enum encodings can change between Kagi versions without notice. Do not
> build long-lived integrations against them. #331 (MCP server) is expected to
> be the first frozen surface, layered on top of this CLI.

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

`kagi plan …` prints a side-effect-free **envelope** (it never touches the repo):

```jsonc
{
  "plan_id": "ee04a5630ae2e617",          // content hash — see below
  "operation": { "Checkout": { "branch": "feature" } },
  "plan": {                                 // serialized OperationPlan (ADR-0129)
    "title":     { "Checkout": { "Checkout": { "branch": "feature" } } },
    "current":   { "head": "branch: main",    "dirty": "clean" },
    "predicted": { "head": "branch: feature", "dirty": "clean" },
    "warnings":  [],                          // [] or PlanNote objects
    "blockers":  [],                          // non-empty ⇒ confirm refuses
    "recovery":  { "kind": { /* … */ }, "commands": [] },
    "disposition": "Ready",                   // "Ready" | { "NoOp": … } | "Blocked"
    "head_at_plan": { "Attached": { "branch": "main", "target": "<sha>" } },
    "stash_count_at_plan": 0,
    "worktree_digest": null,                  // or a u64 for tree-sensitive ops
    "preview_files":   [],
    "preview_commits": [],
    "destructive": false                      // true ⇒ confirm needs --yes
  }
}
```

`PlanNote` / `PlanTitle` / `RecoveryKind` serialize as their Rust enum trees
(`{ "Category": { "Variant": { … } } }`). For human text, render blockers/warnings
with the display strings the GUI uses — the CLI already surfaces those in the
`confirm` refusal `detail.blockers`.

## `plan_id` — the content hash (staleness detection)

`plan_id` is a deterministic hash of exactly the inputs whose change would
invalidate the plan (`OperationPlan::plan_id`):

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
operation, **re-plans**, and gates execution:

1. **stale?** recomputed `plan_id` ≠ the envelope's → refuse; `detail.changed`
   names what moved (HEAD / stash / working tree / operation target).
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

Serialized `StateSummary`: `{ "head": "branch: main", "dirty": "clean" }`.

## `oplog`

A JSON array of the newest `--limit N` (default 20) operation-log entries, each
in the same shape the oplog file stores (ADR-0149 / #329): `id`, `parent`,
`timestamp`, `op`, `repo`, `actor`, `worktree`, `before`, `outcome`.
