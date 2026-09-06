//! The one agent-facing contract shared by the CLI (`src/cli_main.rs`) and the
//! MCP server (`crates/kagi-mcp`) — issue #509, and the receipt binding of #505.
//!
//! Before this module each frontend carried its own `build_operation` match and
//! its own plan serializer, so adding an operation meant editing two matches and
//! two serializers, and the two op sets could silently drift apart. Everything
//! that is *the same contract* lives here:
//!
//! - [`SUPPORTED_OPS`] / [`OPS_USAGE`] — the op set and its one help string.
//! - [`resolve_operation`] — op name + positional args → [`Operation`]
//!   (needs the [`Backend`] only to default `create-branch`'s start point).
//! - [`plan_envelope`] / [`plan_body`] — the plan JSON, identical on both sides.
//! - [`confirm_response`] — the execute result, bound to **this** run's oplog
//!   receipt (`Backend::run_recorded`) instead of a global `read_oplog_tail(1)`,
//!   which a concurrent writer could have moved out from under it (#505).
//!
//! Each transport still owns only its own edge: the CLI owns argv, `--yes` and
//! exit codes; MCP owns the tool envelope, the plan store and its host approval.
//!
//! Serialization lives here (not in `kagi-domain`, which stays dependency-free —
//! CLAUDE.md invariant #2): the domain types are read through their public
//! fields and `message_en()` renderers. `kagi-git` already depends on
//! `serde_json`.
//!
//! The JSON schema is UNSTABLE/internal for v1 — see `docs/plan-json.md`.

use kagi_domain::commit::CommitId;
use serde_json::{json, Value};

use crate::backend::recording::Recording;
use crate::{Backend, Head, Operation, OperationOutcome, OperationPlan};

/// The op set both agent entry points expose. Deliberately no force-push, no
/// `reset --hard`, no clean — their absence is the product's reason to exist.
pub const SUPPORTED_OPS: &[&str] = &[
    "checkout",
    "create-branch",
    "delete-branch",
    "discard",
    "reset",
];

/// One-line rendering of [`SUPPORTED_OPS`] with their arguments. Used by the
/// CLI usage text *and* the MCP `kagi_plan` tool description, so the advertised
/// op set cannot disagree between the two entry points.
pub const OPS_USAGE: &str = "checkout <branch> | create-branch <name> [at-commit] | \
     delete-branch <name> | discard <path...> | reset <commit>";

/// Build the [`Operation`] for `op_name` + positional `args`.
///
/// The only backend-dependent step is defaulting `create-branch`'s start point
/// to HEAD; everything else is a pure argument mapping. Error strings are the
/// shared contract too — both frontends report an unsupported op and a missing
/// argument with the same words.
pub fn resolve_operation(
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

/// Like `Head::display` but keeps the branch tip's short SHA, so a same-branch
/// move (a new commit on the branch) is visible in a staleness message.
pub fn head_desc(h: &Head) -> String {
    let short = |t: &str| t.get(..8).unwrap_or(t).to_string();
    match h {
        Head::Attached { branch, target } => format!("branch: {} @ {}", branch, short(target)),
        Head::Detached { target } => format!("detached: {}", short(target)),
        Head::Unborn { branch } => format!("unborn ({})", branch),
    }
}

/// The full `plan` stage response: the identity + staleness snapshot a later
/// `confirm` needs, plus the human-readable [`plan_body`] display block.
///
/// The CLI prints this object as-is; MCP returns it as the tool's structured
/// content with its own `next` hint added. Same `plan_id`, same blockers, same
/// recovery on both sides — that equality is what #509 asked for.
pub fn plan_envelope(op_name: &str, args: &[String], plan: &OperationPlan) -> Value {
    json!({
        "plan_id": plan.plan_id(),
        "op": op_name,
        "args": args,
        "head_at_plan": head_desc(&plan.head_at_plan),
        "stash_count_at_plan": plan.stash_count_at_plan(),
        "worktree_digest": plan.worktree_digest().map(|d| d.0),
        "plan": plan_body(plan),
    })
}

/// Human/machine-readable plan block. Uses the domain types' `message_en()`
/// renderers (the same strings the GUI and the oplog use) instead of
/// serializing the enum tree, so `kagi-domain` needs no serde derive.
pub fn plan_body(p: &OperationPlan) -> Value {
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

/// The `confirm` stage response, built from the [`Recording`] that
/// `Backend::run_recorded` produced for **this** invocation (#505).
///
/// `oplog` is this execution's own entry — never a global tail read, which a
/// concurrent writer (another repo, another operation, another process) could
/// have appended to between the run and the read. `recorded` says whether that
/// entry actually reached the log: on an append failure the entry is reported as
/// *attempted* rather than substituted with some unrelated earlier entry, so
/// "the mutation happened" and "the mutation was recorded" stay distinguishable.
pub fn confirm_response(
    op: &Operation,
    plan_id: &str,
    outcome: &OperationOutcome,
    recording: &Recording,
) -> Value {
    let (recorded, error) = match recording {
        Recording::Appended { .. } => (true, Value::Null),
        Recording::Failed { error, .. } => (false, json!(error)),
    };
    json!({
        "status": "ok",
        "op": op.oplog_name(),
        "plan_id": plan_id,
        "outcome": format!("{:?}", outcome),
        "oplog": entry_value(recording.entry()),
        "recorded": recorded,
        "recording_error": error,
    })
}

/// One oplog entry as a JSON object, reusing `oplog::entry_to_json` so the
/// agent-facing shape stays identical to the log file's own line format.
fn entry_value(entry: &crate::oplog::OpLogEntry) -> Value {
    serde_json::from_str(&crate::oplog::entry_to_json(entry)).unwrap_or(Value::Null)
}
