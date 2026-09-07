//! Headless `kagi` CLI subcommands (#330).
//!
//! An agent (Claude Code / Codex / Amp) drives Kagi's safety pipeline from
//! outside the GUI: `kagi plan <op> …` emits a side-effect-free plan as JSON,
//! and `kagi confirm …` re-plans, verifies nothing moved, then runs the op
//! through the *same* `Backend::run` path the GUI uses (plan → preflight →
//! execute → verify → oplog, ADR-0104/0149). `status` and `oplog` are
//! read-only introspection.
//!
//! Operation resolution and the plan/confirm JSON shapes are NOT owned here:
//! they are the shared agent contract in [`kagi_git::api`] (#509), used verbatim
//! by the MCP server too, so adding an operation edits one match and one
//! serializer. This file owns the CLI edge only — argv, `--yes`, stdin/stdout
//! and exit codes. `confirm` never deserializes the full plan tree; it re-plans
//! from `{plan_id, op, args}` and compares `plan_id`.
//!
//! Design (§5 decisions on #330):
//! - **plan-id = content hash** ([`OperationPlan::plan_id`]). The id itself is
//!   the staleness check: `confirm` recomputes the plan against the repo *now*
//!   and refuses if the recomputed id differs (TOCTOU, no server-side state).
//! - **standalone `confirm`**: the plan is not stored. The agent pipes the plan
//!   JSON that `plan` emitted back into `confirm` (stdin or `--plan <file>`); it
//!   carries the op + args to rebuild + the id to verify.
//! - **destructive ops require `--yes`** (mirrors the GUI two-stage confirm).
//! - lives on the existing `kagi` binary — [`dispatch`] runs when argv[1] is a
//!   known subcommand, otherwise `main` takes the normal GUI path.
//!
//! The JSON schema is UNSTABLE/internal for v1 — see `docs/plan-json.md`.

use std::io::Read;
use std::path::PathBuf;

use kagi_git::api;
use kagi_git::{Backend, OperationPlan};

/// The subcommands that switch `kagi` into headless CLI mode.
const SUBCOMMANDS: &[&str] = &["plan", "confirm", "status", "oplog"];

/// True when the first CLI argument is a known headless subcommand, so `main`
/// should hand off to [`dispatch`] instead of launching the GUI.
pub fn is_cli_subcommand(args: &[String]) -> bool {
    args.first()
        .is_some_and(|a| SUBCOMMANDS.contains(&a.as_str()))
}

/// The slice of the `plan` envelope that `confirm` actually needs: enough to
/// rebuild the operation and re-verify the id, plus the plan-time staleness
/// snapshot (primitives) so a mismatch can name *what* changed. The `plan`
/// display block that `plan` also emits is ignored (unknown fields are dropped).
#[derive(serde::Deserialize)]
struct ConfirmInput {
    plan_id: String,
    op: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    head_at_plan: String,
    #[serde(default)]
    stash_count_at_plan: usize,
    #[serde(default)]
    worktree_digest: Option<u64>,
}

/// Run the headless CLI. Returns a process exit code:
/// `0` ok · `1` usage/error · `2` refused (blockers / stale / needs `--yes`).
pub fn dispatch(args: &[String]) -> i32 {
    let (sub, rest) = match args.split_first() {
        Some(x) => x,
        None => return usage(),
    };
    let result = match sub.as_str() {
        "plan" => cmd_plan(rest),
        "confirm" => cmd_confirm(rest),
        "status" => cmd_status(rest),
        "oplog" => cmd_oplog(rest),
        _ => return usage(),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            print_error(&e);
            1
        }
    }
}

fn usage() -> i32 {
    eprintln!(
        "usage:\n  \
         kagi plan <op> [args...] [--repo PATH] [--json]\n  \
         kagi confirm [--yes] [--plan FILE] [--repo PATH] [--json]   (plan JSON on stdin if no --plan)\n  \
         kagi status [--repo PATH] [--json]\n  \
         kagi oplog [--limit N] [--repo PATH] [--json]\n\
         supported ops: {}",
        api::OPS_USAGE
    );
    1
}

// ── flag helpers ────────────────────────────────────────────

/// Pull `--repo PATH` / `--plan FILE` / `--yes` out of `args` (repo default:
/// current dir), returning the remaining positional args. `--json` is accepted
/// everywhere and dropped (JSON is the only output format in v1).
fn take_flags(args: &[String]) -> (PathBuf, Option<String>, bool, Vec<String>) {
    let mut repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut plan_file: Option<String> = None;
    let mut yes = false;
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--repo" => {
                if let Some(p) = it.next() {
                    repo = PathBuf::from(p);
                }
            }
            "--plan" => plan_file = it.next().cloned(),
            "--yes" => yes = true,
            "--json" => {}
            other => rest.push(other.to_string()),
        }
    }
    (repo, plan_file, yes, rest)
}

fn open_backend(repo: &std::path::Path) -> Result<Backend, String> {
    Backend::discover_with_policy(repo, kagi_git::backend::ExecutionPolicy::cli())
        .map_err(|e| format!("{}", e))
}

// ── plan ────────────────────────────────────────────────────

fn cmd_plan(args: &[String]) -> Result<i32, String> {
    let (repo, _pf, _yes, rest) = take_flags(args);
    let (op_name, op_args) = rest
        .split_first()
        .ok_or_else(|| "plan: missing <op>".to_string())?;
    let backend = open_backend(&repo)?;
    let op = api::resolve_operation(&backend, op_name, op_args)?;
    // Planning is side-effect-free: `Backend::plan` never mutates the repo.
    let plan = backend.plan(&op).map_err(|e| format!("{}", e))?;
    // Top-level: what `confirm` reads back (op + args + id + staleness snapshot).
    // `plan`: a human-readable display block for the agent (ignored by confirm).
    let out = api::plan_envelope(op_name, op_args, &plan);
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
    );
    Ok(0)
}

// ── confirm ─────────────────────────────────────────────────

fn cmd_confirm(args: &[String]) -> Result<i32, String> {
    let (repo, plan_file, yes, _rest) = take_flags(args);

    // Read the plan envelope the agent got from `kagi plan` (file or stdin).
    let raw = match plan_file {
        Some(f) => std::fs::read_to_string(&f).map_err(|e| format!("reading {}: {}", f, e))?,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("reading stdin: {}", e))?;
            buf
        }
    };
    let input: ConfirmInput =
        serde_json::from_str(&raw).map_err(|e| format!("invalid plan JSON: {}", e))?;

    let mut backend = open_backend(&repo)?;
    let op = api::resolve_operation(&backend, &input.op, &input.args)?;
    // Re-plan against the repo NOW. If nothing moved, the recomputed id equals
    // the id the agent holds; if it differs, the repo changed (TOCTOU).
    let fresh = backend.plan(&op).map_err(|e| format!("{}", e))?;

    if fresh.plan_id() != input.plan_id {
        let changed = describe_changes(&input, &fresh);
        return Ok(refuse(
            "repo changed since plan — re-plan and try again",
            serde_json::json!({ "changed": changed, "expected_plan_id": input.plan_id, "actual_plan_id": fresh.plan_id() }),
        ));
    }

    // Blockers: never executable (mirrors the GUI hiding the Execute button).
    if !fresh.blockers.is_empty() {
        let blockers: Vec<String> = fresh.blockers.iter().map(|b| b.message_en()).collect();
        return Ok(refuse(
            "plan has blockers",
            serde_json::json!({ "blockers": blockers }),
        ));
    }

    // Destructive ops require the explicit opt-in (GUI two-stage equivalent).
    if fresh.destructive && !yes {
        return Ok(refuse(
            "destructive operation requires --yes",
            serde_json::json!({ "destructive": true }),
        ));
    }

    // Execute through the one true write path: preflight → execute → verify →
    // oplog all happen inside `run_recorded` (#329). Actor=cli tags the log, and
    // the report carries THIS run's oplog receipt — never a global tail read
    // that a concurrent writer could have moved out from under it (#505).
    let report = backend.run_recorded(&op, &fresh);
    let response = api::confirm_response(&op, &input.plan_id, &report);
    let failed = response["status"] == "error";
    println!("{}", response);
    Ok(if failed { 1 } else { 0 })
}

/// Name which staleness dimension(s) moved between the agent's plan (the
/// snapshot carried in the envelope) and a freshly recomputed plan.
fn describe_changes(old: &ConfirmInput, fresh: &OperationPlan) -> Vec<String> {
    let mut out = Vec::new();
    let fresh_head = api::head_desc(&fresh.head_at_plan);
    if old.head_at_plan != fresh_head {
        out.push(format!(
            "HEAD changed (was {}, now {})",
            old.head_at_plan, fresh_head
        ));
    }
    if old.stash_count_at_plan != fresh.stash_count_at_plan() {
        out.push(format!(
            "stash count changed (was {}, now {})",
            old.stash_count_at_plan,
            fresh.stash_count_at_plan()
        ));
    }
    if old.worktree_digest != fresh.worktree_digest().map(|d| d.0) {
        out.push("working tree changed".to_string());
    }
    if out.is_empty() {
        // The ids differed for a reason not captured above (e.g. destructive
        // flag) — surface a generic note so the agent still re-plans.
        out.push("plan is stale".to_string());
    }
    out
}

// ── status ──────────────────────────────────────────────────

fn cmd_status(args: &[String]) -> Result<i32, String> {
    let (repo, _pf, _yes, _rest) = take_flags(args);
    let backend = open_backend(&repo)?;
    let state = backend.current_state().map_err(|e| format!("{}", e))?;
    let out = serde_json::json!({ "head": state.head, "dirty": state.dirty });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
    );
    Ok(0)
}

// ── oplog ───────────────────────────────────────────────────

fn cmd_oplog(args: &[String]) -> Result<i32, String> {
    // `--repo PATH` (default: current dir) via the shared helper — #421: the
    // oplog is scoped to one repo, matching the `[--repo PATH]` usage line.
    let (repo, _plan, _yes, rest) = take_flags(args);
    // `--limit N` is a positional-free flag; default 20.
    let mut limit = 20usize;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--limit" {
            if let Some(n) = it.next() {
                limit = n.parse().map_err(|_| format!("bad --limit: {}", n))?;
            }
        }
    }
    let entries = kagi_git::read_oplog_tail_for_repo(&repo, limit);
    let items: Vec<String> = entries.iter().map(kagi_git::entry_to_json).collect();
    println!("[{}]", items.join(","));
    Ok(0)
}

// ── output helpers ──────────────────────────────────────────

fn json_str(s: &str) -> String {
    // Minimal JSON string escaping for the confirm result line.
    serde_json::Value::String(s.to_string()).to_string()
}

fn refuse(reason: &str, detail: serde_json::Value) -> i32 {
    println!(
        "{{\"status\":\"refused\",\"reason\":{},\"detail\":{}}}",
        json_str(reason),
        detail
    );
    2
}

fn print_error(msg: &str) {
    println!("{{\"status\":\"error\",\"error\":{}}}", json_str(msg));
}
