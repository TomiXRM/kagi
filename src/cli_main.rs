//! Headless `kagi` CLI subcommands (#330).
//!
//! An agent (Claude Code / Codex / Amp) drives Kagi's safety pipeline from
//! outside the GUI: `kagi plan <op> …` emits a side-effect-free plan as JSON,
//! and `kagi confirm …` re-plans, verifies nothing moved, then runs the op
//! through the *same* `Backend::run` path the GUI uses (plan → preflight →
//! execute → verify → oplog, ADR-0104/0149). `status` and `oplog` are
//! read-only introspection.
//!
//! Serialization lives HERE (the bin crate), not in `kagi-domain`, which stays
//! dependency-free (CLAUDE.md invariant #2): `serde`/`serde_json` are only used
//! in this file, reading the domain types' public fields + `message_en()`
//! renderers. `confirm` never deserializes the full plan tree — it re-plans from
//! `{plan_id, op, args}` and compares `plan_id`.
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

use kagi_domain::commit::CommitId;
use kagi_git::{Actor, Backend, Operation, OperationPlan};

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
         supported ops: checkout <branch> | create-branch <name> [at-commit] | \
         delete-branch <name> | discard <path...> | reset <commit>"
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

/// Build the [`Operation`] for `op_name` + positional `args`. Needs the backend
/// to default `create-branch`'s start point to HEAD.
fn build_operation(backend: &Backend, op_name: &str, args: &[String]) -> Result<Operation, String> {
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

fn open_backend(repo: &std::path::Path) -> Result<Backend, String> {
    Backend::discover(repo).map_err(|e| format!("{}", e))
}

// ── plan ────────────────────────────────────────────────────

fn cmd_plan(args: &[String]) -> Result<i32, String> {
    let (repo, _pf, _yes, rest) = take_flags(args);
    let (op_name, op_args) = rest
        .split_first()
        .ok_or_else(|| "plan: missing <op>".to_string())?;
    let backend = open_backend(&repo)?;
    let op = build_operation(&backend, op_name, op_args)?;
    // Planning is side-effect-free: `Backend::plan` never mutates the repo.
    let plan = backend.plan(&op).map_err(|e| format!("{}", e))?;
    // Top-level: what `confirm` reads back (op + args + id + staleness snapshot).
    // `plan`: a human-readable display block for the agent (ignored by confirm).
    let out = serde_json::json!({
        "plan_id": plan.plan_id(),
        "op": op_name,
        "args": op_args,
        "head_at_plan": head_desc(&plan.head_at_plan),
        "stash_count_at_plan": plan.stash_count_at_plan(),
        "worktree_digest": plan.worktree_digest().map(|d| d.0),
        "plan": plan_body(&plan),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
    );
    Ok(0)
}

/// Human-readable plan block. Uses the domain types' `message_en()` renderers
/// (the same strings the GUI/oplog use) instead of serializing the enum tree —
/// so `kagi-domain` needs no serde derive.
fn plan_body(p: &OperationPlan) -> serde_json::Value {
    serde_json::json!({
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
    let op = build_operation(&backend, &input.op, &input.args)?;
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
    // oplog all happen inside `Backend::run` (#329). Actor=cli tags the log.
    backend.set_actor(Actor::Cli);
    let outcome = backend.run(&op, &fresh).map_err(|e| format!("{}", e))?;

    // The oplog entry `run` just wrote (newest-first tail).
    let last = kagi_git::read_oplog_tail(1);
    let oplog_json = last
        .first()
        .map(kagi_git::entry_to_json)
        .unwrap_or_else(|| "null".to_string());

    println!(
        "{{\"status\":\"ok\",\"op\":{},\"plan_id\":{},\"outcome\":{},\"oplog\":{}}}",
        json_str(op.oplog_name()),
        json_str(&input.plan_id),
        json_str(&format!("{:?}", outcome)),
        oplog_json,
    );
    Ok(0)
}

/// Name which staleness dimension(s) moved between the agent's plan (the
/// snapshot carried in the envelope) and a freshly recomputed plan.
fn describe_changes(old: &ConfirmInput, fresh: &OperationPlan) -> Vec<String> {
    let mut out = Vec::new();
    let fresh_head = head_desc(&fresh.head_at_plan);
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

/// Like `Head::display` but keeps the branch tip's short SHA, so a same-branch
/// move (a new commit on the branch) is visible in the change message.
fn head_desc(h: &kagi_git::Head) -> String {
    use kagi_git::Head;
    let short = |t: &str| t.get(..8).unwrap_or(t).to_string();
    match h {
        Head::Attached { branch, target } => format!("branch: {} @ {}", branch, short(target)),
        Head::Detached { target } => format!("detached: {}", short(target)),
        Head::Unborn { branch } => format!("unborn ({})", branch),
    }
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
