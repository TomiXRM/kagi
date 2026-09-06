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

//! Operation resolution and the plan/confirm JSON are the shared agent contract
//! in [`kagi_git::api`] (#509) — the same code the CLI runs. This module owns
//! only the MCP edge: the tool envelope, the server-side plan store, and the
//! host-approval semantics.

use kagi_git::api;
use kagi_git::Backend;
use serde_json::{json, Value};

use crate::{Server, StoredPlan};

/// A tool outcome: `Ok(value)` on success, `Err(message)` surfaced to the model
/// as an `isError: true` tool result.
type ToolResult = Result<Value, String>;

/// The op set exposed over MCP. Re-exported from the shared contract so the CLI
/// and MCP cannot advertise different operations (#509).
pub use kagi_git::api::SUPPORTED_OPS;

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
    Backend::discover_with_policy(server.repo(), kagi_git::backend::ExecutionPolicy::mcp())
        .map_err(|e| e.to_string())
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

/// `kagi_plan(op, args)` — stage 1. Side-effect-free.
pub fn plan(server: &mut Server, args: &Value) -> ToolResult {
    let op_name = args
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing `op`".to_string())?
        .to_string();
    let op_args = string_args(args);

    let backend = open(server)?;
    let op = api::resolve_operation(&backend, &op_name, &op_args)?;
    // Planning never mutates the repo.
    let plan = backend.plan(&op).map_err(|e| e.to_string())?;
    let plan_id = plan.plan_id();

    server.plans.insert(
        plan_id,
        StoredPlan {
            op: op_name.clone(),
            args: op_args.clone(),
        },
    );

    // The shared envelope (identical to what `kagi plan` prints), plus the MCP
    // edge's own next-step hint.
    let mut out = api::plan_envelope(&op_name, &op_args, &plan);
    out["next"] = json!("call kagi_confirm(plan_id) to execute");
    Ok(out)
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
    let op = api::resolve_operation(&backend, &stored.op, &stored.args)?;
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
    // inside `run_recorded`. Actor=mcp tags every entry this server writes, and
    // the report carries THIS run's receipt, so a concurrently appended entry
    // (another repo, another process) can never be echoed back as ours (#505).
    let report = backend.run_recorded(&op, &fresh);
    let outcome = report.result.map_err(|e| e.to_string())?;

    // Drop the consumed plan so a plan_id can't be replayed.
    server.plans.remove(&plan_id);

    Ok(api::confirm_response(
        &op,
        &plan_id,
        &outcome,
        &report.recording,
    ))
}
