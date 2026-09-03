//! Tool catalogue for `tools/list` — names, descriptions, input schemas, and
//! MCP annotations (#331 / #332).
//!
//! Annotation mapping (PM-locked, #332): annotations are DERIVED from Kagi's
//! existing danger classification, not hand-tabled per tool:
//! - read tools (and `kagi_plan`, which mutates nothing) → `readOnlyHint: true`
//! - `kagi_confirm` folds `OperationPlan.destructive` (ADR-0004/0023) over the
//!   supported op set (`write::SUPPORTED_OPS`) → `destructiveHint`
//! - `openWorldHint` follows op kind: true only if a network op (fetch/push)
//!   is in the set. None is today, so `kagi_confirm` derives to `false`.
//!
//! The fold constants in `write.rs` are pinned to real `OperationPlan`s by the
//! `tools_list_annotations_derive_from_plan_classification` test, so the
//! advertised hints and the classification cannot drift.
//!
//! **Intentionally absent tools**: there is no force-push, no `reset --hard`,
//! and no `git clean`. This is Kagi's reason to exist — destructive history/
//! working-tree rewrites are structurally impossible through this server. The
//! descriptions below say so, so an agent understands "Kagi cannot do that".

use serde_json::{json, Value};

/// The read-only tools, all side-effect-free (`readOnlyHint: true`).
pub const READ_TOOLS: &[&str] = &[
    "kagi_repo_status",
    "kagi_graph",
    "kagi_diff",
    "kagi_commit_show",
    "kagi_branches",
    "kagi_worktrees",
    "kagi_conflicts",
    "kagi_stashes",
    "kagi_oplog",
];

/// Top-of-session guidance surfaced via `initialize.instructions`.
pub const SERVER_INSTRUCTIONS: &str = "\
Kagi exposes git through a safety pipeline. Read tools are unconditional. \
Writes are two-stage: call kagi_plan(op, args) to get a plan (with plan_id, \
blockers, and whether it is destructive), then kagi_confirm(plan_id) to \
execute it through Kagi's plan→preflight→execute→verify→oplog path. \
Force-push, reset --hard, and git clean are intentionally NOT provided: \
destructive history or working-tree rewrites are impossible through Kagi.";

/// True when `name` is one of the read tools.
pub fn is_read_tool(name: &str) -> bool {
    READ_TOOLS.contains(&name)
}

/// The full `tools/list` array. In read-only mode (#332) the write tool
/// `kagi_confirm` is removed entirely — it does not exist, rather than
/// existing-but-refusing. `kagi_plan` stays: planning is pure inspection.
pub fn tool_list(readonly: bool) -> Vec<Value> {
    let mut v = read_tools();
    v.push(plan_tool());
    if !readonly {
        v.push(confirm_tool());
    }
    v
}

/// Derive an MCP annotation object from Kagi's classification (#332). This is
/// the ONLY place annotation JSON is built — callers pass a classification,
/// never a hand-written hint object.
pub fn derive_annotations(read_only: bool, destructive: bool, network: bool) -> Value {
    if read_only {
        json!({ "readOnlyHint": true, "openWorldHint": network })
    } else {
        json!({
            "readOnlyHint": false,
            "destructiveHint": destructive,
            "idempotentHint": false,
            "openWorldHint": network,
        })
    }
}

/// A read tool never mutates the repo. `kagi_diff` included: diff-cache
/// warming is an internal optimization, not an observable side effect (#332).
fn read_only() -> Value {
    derive_annotations(true, false, false)
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool(name: &str, description: &str, input_schema: Value, annotations: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "annotations": annotations,
    })
}

fn read_tools() -> Vec<Value> {
    vec![
        tool(
            "kagi_repo_status",
            "Current repository state: branch, HEAD sha, upstream + ahead/behind, \
             dirty flag, staged/unstaged/untracked/conflicted paths, stash count, \
             and last-fetch age.",
            obj(json!({}), &[]),
            read_only(),
        ),
        tool(
            "kagi_graph",
            "Commit graph rows (newest first): sha, parents, summary, author, \
             timestamp. Use `limit` to bound the walk.",
            obj(
                json!({
                    "limit": { "type": "integer", "minimum": 1, "description": "max commits (default 200)" }
                }),
                &[],
            ),
            read_only(),
        ),
        tool(
            "kagi_diff",
            "Changed files with add/delete counts. With `staged: true` shows the \
             index diff; with `from` and `to` compares two revisions; otherwise \
             shows the unstaged working-tree diff.",
            obj(
                json!({
                    "from": { "type": "string", "description": "base revision" },
                    "to": { "type": "string", "description": "target revision" },
                    "staged": { "type": "boolean" }
                }),
                &[],
            ),
            read_only(),
        ),
        tool(
            "kagi_commit_show",
            "Show one commit: metadata (author, message, parents) and its changed \
             files with add/delete counts.",
            obj(
                json!({ "revision": { "type": "string", "description": "sha or sha prefix" } }),
                &["revision"],
            ),
            read_only(),
        ),
        tool(
            "kagi_branches",
            "Local branches (with upstream + ahead/behind) and remote-tracking \
             branches.",
            obj(json!({}), &[]),
            read_only(),
        ),
        tool(
            "kagi_worktrees",
            "Registered worktrees: path, checked-out branch, locked state, and \
             pending-change counts.",
            obj(json!({}), &[]),
            read_only(),
        ),
        tool(
            "kagi_conflicts",
            "Paths currently in a conflicted (unmerged) state.",
            obj(json!({}), &[]),
            read_only(),
        ),
        tool(
            "kagi_stashes",
            "Stash entries: index, message, and commit id.",
            obj(json!({}), &[]),
            read_only(),
        ),
        tool(
            "kagi_oplog",
            "Kagi's operation log (every write Kagi has performed, newest first), \
             tagged by actor (human / cli / mcp).",
            obj(
                json!({ "limit": { "type": "integer", "minimum": 1, "description": "default 20" } }),
                &[],
            ),
            read_only(),
        ),
    ]
}

/// `kagi_plan` — read-only planning. Returns an `OperationPlan` with a
/// content-hash `plan_id`, warnings, blockers, and a `destructive` flag.
fn plan_tool() -> Value {
    tool(
        "kagi_plan",
        "Stage 1 of 2 for any write. Returns a side-effect-free plan for `op` \
         (with plan_id, predicted state, warnings, blockers, and whether it is \
         destructive). NOTHING is executed — call kagi_confirm(plan_id) to run \
         it. Supported ops: checkout <branch> | create-branch <name> [at] | \
         delete-branch <name> | discard <path...> | reset <commit> (soft/mixed \
         only). There is deliberately no force-push, reset --hard, or clean op: \
         Kagi cannot perform destructive history/working-tree rewrites.",
        obj(
            json!({
                "op": { "type": "string", "description": "operation name (see description)" },
                "args": { "type": "array", "items": { "type": "string" }, "description": "positional args for the op" }
            }),
            &["op"],
        ),
        // Planning mutates nothing.
        derive_annotations(true, false, false),
    )
}

/// `kagi_confirm` — the single write tool. Calling it IS the approval; the
/// host prompts here. Its hints are DERIVED: `destructiveHint` is the fold of
/// `OperationPlan.destructive` over `write::SUPPORTED_OPS`, and
/// `openWorldHint` follows op kind (no fetch/push op exists → false).
fn confirm_tool() -> Value {
    tool(
        "kagi_confirm",
        "Stage 2 of 2: execute a previously planned operation by its plan_id, \
         through Kagi's plan→preflight→execute→verify→oplog pipeline. Re-plans \
         first and refuses if the repo moved since planning (stale plan_id) or \
         if the plan has blockers. This is the point of approval — your host \
         will prompt before it runs. Kagi still cannot force-push, reset --hard, \
         or clean: those operations do not exist here.",
        obj(
            json!({ "plan_id": { "type": "string", "description": "plan_id from kagi_plan" } }),
            &["plan_id"],
        ),
        derive_annotations(
            false,
            crate::write::CONFIRM_DESTRUCTIVE,
            crate::write::CONFIRM_NETWORK,
        ),
    )
}
