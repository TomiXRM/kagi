//! The two-stage write path: `kagi_plan` and `kagi_confirm` (#331, reusing the
//! #330 plan-id / staleness plumbing).
//!
//! `kagi_plan` builds a side-effect-free [`OperationPlan`], records
//! `plan_id → (op, args)` in the server's in-memory store, and returns the plan.
//! `kagi_confirm(plan_id)` looks it up, re-plans against the repo *now*, refuses
//! on a stale `plan_id` (repo moved, TOCTOU) or on blockers, then runs the op
//! through the one true write path [`Backend::run`] tagged `actor = mcp` so the
//! write lands in the oplog (#329).
//!
//! There is no `--yes` gate here (unlike `cli_main.rs`): in MCP the host's
//! approval prompt on the `destructiveHint` `kagi_confirm` tool IS the second
//! confirmation (PM-locked §5). Blockers are still hard-refused.

use kagi_domain::commit::CommitId;
use kagi_git::{Actor, Backend, Operation, OperationPlan};
use serde_json::{json, Value};

use crate::{Server, StoredPlan};

/// A tool outcome: `Ok(value)` on success, `Err(message)` surfaced to the model
/// as an `isError: true` tool result.
type ToolResult = Result<Value, String>;

/// The op set exposed over MCP — exactly what [`build_operation`] dispatches on.
pub const SUPPORTED_OPS: &[&str] = &[
    "checkout",
    "create-branch",
    "delete-branch",
    "discard",
    "reset",
];

/// `kagi_confirm`'s list-time `destructiveHint`: the fold of
/// `OperationPlan.destructive` (ADR-0004/0023) over [`SUPPORTED_OPS`] — true
/// because discard and reset plan as destructive.
// ponytail: the fold is a pinned const (tools/list has no repo to plan
// against); the derivation test rebuilds it from real OperationPlans in a
// temp repo, so this value cannot drift from the classification.
pub const CONFIRM_DESTRUCTIVE: bool = true;

/// `kagi_confirm`'s `openWorldHint`: true only if a network op (fetch/push)
/// is in [`SUPPORTED_OPS`]. None is — the MCP surface has no network op yet.
pub const CONFIRM_NETWORK: bool = false;

fn open(server: &Server) -> Result<Backend, String> {
    Backend::discover(server.repo()).map_err(|e| e.to_string())
}

/// Parse the `args` string array from a tool-call argument object.
fn string_args(args: &Value) -> Vec<String> {
    args.get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the [`Operation`] for `op_name` + positional `args`. Ported from
/// `cli_main::build_operation` so the two agent-facing entry points support the
/// exact same op set (checkout / create-branch / delete-branch / discard /
/// reset). Deliberately NO force-push / reset --hard / clean op.
pub fn build_operation(
    backend: &Backend,
    op_name: &str,
    args: &[String],
) -> Result<Operation, String> {
    let need = |n: usize| -> Result<(), String> {
        if args.len() < n {
            Err(format!("`{}` needs {} argument(s)", op_name, n))
        } else {
            Ok(())
        }
    };
    match op_name {
        "checkout" => {
            need(1)?;
            Ok(Operation::Checkout {
                branch: args[0].clone(),
            })
        }
        "create-branch" => {
            need(1)?;
            let at = match args.get(1) {
                Some(c) => CommitId(c.clone()),
                None => backend
                    .head_commit_id()
                    .ok_or_else(|| "HEAD has no commit to branch from".to_string())?,
            };
            Ok(Operation::CreateBranch {
                name: args[0].clone(),
                at,
            })
        }
        "delete-branch" => {
            need(1)?;
            Ok(Operation::DeleteBranch {
                name: args[0].clone(),
            })
        }
        "discard" => {
            need(1)?;
            Ok(Operation::Discard {
                paths: args.to_vec(),
            })
        }
        "reset" => {
            need(1)?;
            Ok(Operation::ResetCurrentToHead {
                target: CommitId(args[0].clone()),
            })
        }
        other => Err(format!("unsupported op '{}'", other)),
    }
}

/// `kagi_plan(op, args)` — stage 1. Side-effect-free.
pub fn plan(server: &mut Server, args: &Value) -> ToolResult {
    let op_name = args
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing `op`".to_string())?
        .to_string();
    let op_args = string_args(args);

    let backend = open(server)?;
    let op = build_operation(&backend, &op_name, &op_args)?;
    // Planning never mutates the repo.
    let plan = backend.plan(&op).map_err(|e| e.to_string())?;
    let plan_id = plan.plan_id();

    server.plans.insert(
        plan_id.clone(),
        StoredPlan {
            op: op_name.clone(),
            args: op_args.clone(),
        },
    );

    Ok(json!({
        "plan_id": plan_id,
        "op": op_name,
        "args": op_args,
        "next": "call kagi_confirm(plan_id) to execute",
        "plan": plan_body(&plan),
    }))
}

/// `kagi_confirm(plan_id)` — stage 2. Executes through `Backend::run`.
pub fn confirm(server: &mut Server, args: &Value) -> ToolResult {
    let plan_id = args
        .get("plan_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing `plan_id`".to_string())?
        .to_string();

    let stored = server
        .plans
        .get(&plan_id)
        .cloned()
        .ok_or_else(|| format!("unknown plan_id '{}' — call kagi_plan first", plan_id))?;

    let mut backend = open(server)?;
    let op = build_operation(&backend, &stored.op, &stored.args)?;
    // Re-plan against the repo NOW: a matching id proves nothing moved.
    let fresh = backend.plan(&op).map_err(|e| e.to_string())?;

    if fresh.plan_id() != plan_id {
        return Err(format!(
            "plan is stale — the repo changed since planning (expected {}, now {}). Re-plan.",
            plan_id,
            fresh.plan_id()
        ));
    }
    if !fresh.blockers.is_empty() {
        let blockers: Vec<String> = fresh.blockers.iter().map(|b| b.message_en()).collect();
        return Err(format!("plan has blockers: {}", blockers.join("; ")));
    }

    // The one true write path: preflight → execute → verify → oplog all happen
    // inside `Backend::run`. Actor=mcp tags every entry this server writes.
    backend.set_actor(Actor::Mcp);
    let outcome = backend.run(&op, &fresh).map_err(|e| e.to_string())?;

    // The oplog entry `run` just appended (newest-first tail of one).
    let oplog: Value = kagi_git::read_oplog_tail(1)
        .first()
        .map(|e| serde_json::from_str(&kagi_git::entry_to_json(e)).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    // Drop the consumed plan so a plan_id can't be replayed.
    server.plans.remove(&plan_id);

    Ok(json!({
        "status": "ok",
        "op": op.oplog_name(),
        "plan_id": plan_id,
        "outcome": format!("{:?}", outcome),
        "oplog": oplog,
    }))
}

/// Human/machine-readable plan block. Uses the domain types' `message_en()`
/// renderers (the same strings the GUI/oplog use) so `kagi-domain` needs no
/// serde derive — identical approach to `cli_main::plan_body`.
fn plan_body(p: &OperationPlan) -> Value {
    json!({
        "title": p.title.message_en(),
        "current": { "head": p.current.head, "dirty": p.current.dirty },
        "predicted": { "head": p.predicted.head, "dirty": p.predicted.dirty },
        "warnings": p.warnings.iter().map(|n| n.message_en()).collect::<Vec<_>>(),
        "blockers": p.blockers.iter().map(|n| n.message_en()).collect::<Vec<_>>(),
        "recovery": p.recovery.as_ref().map(|r| r.message_en()),
        "disposition": format!("{:?}", p.disposition),
        "destructive": p.destructive,
    })
}
