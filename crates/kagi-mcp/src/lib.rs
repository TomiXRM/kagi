//! `kagi-mcp` — Kagi's git `Backend` exposed as an MCP server over stdio
//! JSON-RPC (#331).
//!
//! The product thesis: an AI agent that touches git *through* this server can
//! never run a destructive command. There is no force-push / reset --hard /
//! clean tool — by design, not by omission (see [`tools::tool_list`]).
//!
//! Two-stage write protocol (PM-locked §5):
//! - [`tools`] `kagi_plan(op, args)` is read-only: it returns an `OperationPlan`
//!   (with a content-hash `plan_id`, reusing #330's plumbing) and mutates
//!   nothing → `readOnlyHint: true`.
//! - `kagi_confirm(plan_id)` is the single `destructiveHint: true` tool the host
//!   (Codex / Claude Code) prompts on. Calling it IS the approval. It re-plans,
//!   refuses on staleness or blockers, then runs the op through the one true
//!   write path `Backend::run` (preflight → execute → verify → oplog, #329).
//!
//! Layering: `kagi-mcp → kagi-git → kagi-domain`. No `gpui` (CI grep gate), no
//! `git2` directly. Serde/JSON lives only in this crate — `kagi-domain` stays
//! pure (CLAUDE.md invariant #2), exactly like #330 kept serde in `cli_main.rs`.
//!
//! The JSON-RPC 2.0 framing is hand-rolled (no MCP SDK) so the safety surface is
//! auditable. The transport is newline-delimited JSON on stdin/stdout (one JSON
//! object per line), driven by [`serve_stdio`].

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

pub mod read;
pub mod tools;
pub mod write;

/// The MCP protocol revision this server speaks (matches the schema #331 cites).
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// A running MCP server bound to a single repository fixed at startup
/// (PM-locked §5: no `repo_path` argument on tools, so an agent cannot reach
/// another repo). The `plans` map is the server-side plan store: `kagi_plan`
/// records `plan_id → (op, args)` and `kagi_confirm(plan_id)` looks it up,
/// re-plans, and executes. It is in-memory and per-process.
pub struct Server {
    repo: PathBuf,
    plans: HashMap<String, StoredPlan>,
    /// Read-only mode (#332): `kagi_confirm` is removed from `tools/list` and
    /// calling it returns JSON-RPC method-not-found. `kagi_plan` stays
    /// (inspection only). Fixed at startup — no runtime toggle, so no
    /// `notifications/tools/list_changed` is ever needed.
    readonly: bool,
}

/// What `kagi_confirm` needs to rebuild + re-verify a plan by id alone.
#[derive(Debug, Clone)]
pub struct StoredPlan {
    pub op: String,
    pub args: Vec<String>,
}

impl Server {
    /// Bind the server to `repo` (fixed for the process lifetime).
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Server {
            repo: repo.into(),
            plans: HashMap::new(),
            readonly: false,
        }
    }

    /// Enable read-only mode (#332). Startup-only: set before serving.
    pub fn set_readonly(&mut self, on: bool) {
        self.readonly = on;
    }

    /// The repository this server is bound to.
    pub fn repo(&self) -> &std::path::Path {
        &self.repo
    }

    /// Handle one JSON-RPC message. Returns `Some(response)` for requests (a
    /// message carrying an `id`) and `None` for notifications (no `id`, e.g.
    /// `notifications/initialized`), which get no reply per JSON-RPC 2.0.
    pub fn handle(&mut self, req: &Value) -> Option<Value> {
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no id): acknowledge by doing nothing observable.
        let id = id?;

        let result = match method {
            "initialize" => Ok(self.initialize()),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::tool_list(self.readonly) })),
            "tools/call" => self.tools_call(&params),
            other => Err(RpcError::method_not_found(other)),
        };

        Some(match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e.to_json() }),
        })
    }

    fn initialize(&self) -> Value {
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "kagi-mcp",
                "version": env!("CARGO_PKG_VERSION"),
                "title": "Kagi (safety-first git)",
            },
            "instructions": tools::SERVER_INSTRUCTIONS,
        })
    }

    /// Route `tools/call`. Read tools run unconditionally and side-effect-free;
    /// `kagi_plan` / `kagi_confirm` are the two-stage write path.
    fn tools_call(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("missing tool name"))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        // Read-only mode: the write tool does not exist. Method-not-found, not
        // a tool error — the host must see the tool as absent (#332).
        if self.readonly && name == "kagi_confirm" {
            return Err(RpcError::method_not_found(name));
        }

        let outcome = match name {
            "kagi_plan" => write::plan(self, &args),
            "kagi_confirm" => write::confirm(self, &args),
            _ if tools::is_read_tool(name) => read::call(&self.repo, name, &args),
            other => {
                return Err(RpcError::invalid_params(&format!(
                    "unknown tool '{}'",
                    other
                )))
            }
        };

        // MCP tool errors are reported *inside* the result (`isError: true`),
        // not as JSON-RPC protocol errors, so the model can see and react to
        // them. Protocol errors are reserved for malformed requests.
        Ok(match outcome {
            Ok(value) => tool_result(value, false),
            Err(msg) => tool_result(json!({ "error": msg }), true),
        })
    }
}

/// Wrap a tool's structured value as an MCP `tools/call` result. We emit BOTH
/// `structuredContent` (the machine-readable value, PM-preferred) and a `text`
/// content block holding the same JSON (the universal fallback for hosts that
/// only read text content).
// ponytail: no per-tool `outputSchema` — the spec allows structuredContent
// without one, and hand-authoring an output schema per tool is high-volume,
// low-value boilerplate. Add if a host starts validating against it.
fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": is_error,
    })
}

/// A JSON-RPC 2.0 error object.
pub struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn method_not_found(method: &str) -> Self {
        RpcError {
            code: -32601,
            message: format!("method not found: {}", method),
        }
    }
    fn invalid_params(msg: &str) -> Self {
        RpcError {
            code: -32602,
            message: msg.to_string(),
        }
    }
    fn to_json(&self) -> Value {
        json!({ "code": self.code, "message": self.message })
    }
}

/// Run the stdio server loop: read newline-delimited JSON requests from `input`,
/// write responses to `output`. One JSON object per line. Blank lines are
/// skipped; a parse failure emits a JSON-RPC parse error and continues.
pub fn serve_stdio(
    server: &mut Server,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => server.handle(&req),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": format!("parse error: {}", e) },
            })),
        };
        if let Some(resp) = response {
            writeln!(output, "{}", serde_json::to_string(&resp)?)?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A throwaway git repo with one commit and a dirty working tree, used to
    // exercise the handler end to end. Uses `git` on PATH (already required by
    // the workspace's integration tests).
    fn temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(p)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {:?} failed", args);
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "T"]);
        std::fs::write(p.join("a.txt"), "hello\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-qm", "init"]);
        // leave a dirty file so status/diff have something to show
        std::fs::write(p.join("a.txt"), "hello world\n").unwrap();
        dir
    }

    fn req(id: i64, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn call(server: &mut Server, name: &str, args: Value) -> Value {
        let resp = server
            .handle(&req(
                1,
                "tools/call",
                json!({ "name": name, "arguments": args }),
            ))
            .unwrap();
        resp["result"].clone()
    }

    #[test]
    fn crate_has_no_gpui_dependency() {
        // Mirrors the CI grep gate (ADR-0163 / #331): the MCP server must stay
        // gpui-free so it runs headless. Scan this crate's manifest + sources.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut checked = 0;
        // Build the forbidden token at runtime (concat!) so the literal never
        // appears in this source file — otherwise this very test, and the CI
        // grep gate, would match themselves. Matches dependency usage (manifest
        // dep line / import / path), not the word in a comment.
        let tok = concat!("g", "pui");
        let mut scan = |rel: &str| {
            let text = std::fs::read_to_string(root.join(rel)).unwrap();
            let hit = text.lines().any(|l| {
                l.contains(&format!("use {}", tok)) || l.contains(&format!("{}::", tok)) || {
                    let t = l.trim_start();
                    t.starts_with(&format!("{} ", tok)) || t.starts_with(&format!("{}=", tok))
                }
            });
            assert!(
                !hit,
                "{} references {} — kagi-mcp must be gpu-free",
                rel, tok
            );
            checked += 1;
        };
        scan("Cargo.toml");
        for entry in std::fs::read_dir(root.join("src")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let rel = format!("src/{}", path.file_name().unwrap().to_string_lossy());
                scan(&rel);
            }
        }
        assert!(checked >= 4, "expected to scan Cargo.toml + source files");
    }

    #[test]
    fn initialize_reports_protocol_and_tools_capability() {
        let mut s = Server::new(".");
        let resp = s.handle(&req(1, "initialize", json!({}))).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "kagi-mcp");
    }

    #[test]
    fn notification_gets_no_response() {
        let mut s = Server::new(".");
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(s.handle(&note).is_none());
    }

    #[test]
    fn tools_list_advertises_read_and_write_tools_and_no_force_push() {
        let mut s = Server::new(".");
        let resp = s.handle(&req(1, "tools/list", json!({}))).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"kagi_plan"));
        assert!(names.contains(&"kagi_confirm"));
        assert!(names.contains(&"kagi_repo_status"));
        // The product invariant: no destructive tool exists at all.
        assert!(!names.iter().any(|n| n.contains("force")));
        assert!(!names.iter().any(|n| n.contains("reset_hard")));
        assert!(!names.iter().any(|n| n.contains("clean")));
        // …and the absence is documented for the agent to read.
        let text = resp.to_string().to_lowercase();
        assert!(
            text.contains("force"),
            "descriptions should mention force-push is absent"
        );
    }

    #[test]
    fn read_tools_are_side_effect_free() {
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        let before = std::process::Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let head = head_sha(dir.path());
        for tool in tools::READ_TOOLS {
            // commit_show is the only read tool with a required arg.
            let args = if *tool == "kagi_commit_show" {
                json!({ "revision": head })
            } else {
                json!({})
            };
            let r = call(&mut s, tool, args);
            assert_eq!(
                r["isError"],
                json!(false),
                "read tool {} errored: {}",
                tool,
                r
            );
        }
        let after = std::process::Command::new("git")
            .args(["status", "--porcelain=v1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(before.stdout, after.stdout, "a read tool mutated the repo");
    }

    #[test]
    fn plan_returns_plan_and_does_not_mutate_without_confirm() {
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        let head_before = head_sha(dir.path());
        let r = call(
            &mut s,
            "kagi_plan",
            json!({ "op": "create-branch", "args": ["feature-x"] }),
        );
        assert_eq!(r["isError"], json!(false));
        let plan_id = r["structuredContent"]["plan_id"].as_str().unwrap();
        assert!(!plan_id.is_empty());
        // Nothing executed: the branch must not exist and HEAD must not move.
        assert!(!branch_exists(dir.path(), "feature-x"));
        assert_eq!(head_sha(dir.path()), head_before);
    }

    #[test]
    fn confirm_executes_and_lands_in_oplog() {
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        let plan = call(
            &mut s,
            "kagi_plan",
            json!({ "op": "create-branch", "args": ["feature-y"] }),
        );
        let plan_id = plan["structuredContent"]["plan_id"]
            .as_str()
            .unwrap()
            .to_string();
        let done = call(&mut s, "kagi_confirm", json!({ "plan_id": plan_id }));
        assert_eq!(done["isError"], json!(false), "confirm failed: {}", done);
        assert_eq!(done["structuredContent"]["status"], "ok");
        assert!(branch_exists(dir.path(), "feature-y"));
        // The write must be recorded in the oplog, tagged with actor=mcp.
        let oplog = done["structuredContent"]["oplog"].clone();
        assert!(
            oplog.is_object(),
            "confirm result carried no oplog entry: {}",
            done
        );
        assert_eq!(oplog["actor"], "mcp", "oplog entry not tagged actor=mcp");
    }

    #[test]
    fn confirm_of_unknown_plan_id_is_refused() {
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        let r = call(&mut s, "kagi_confirm", json!({ "plan_id": "deadbeef" }));
        assert_eq!(r["isError"], json!(true));
    }

    #[test]
    fn confirm_of_plan_with_blocker_is_refused() {
        // Deleting the current branch is a blocker (can't delete checked-out
        // branch). The plan builds, but confirm must refuse it.
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        let plan = call(
            &mut s,
            "kagi_plan",
            json!({ "op": "delete-branch", "args": ["main"] }),
        );
        // plan itself succeeds (planning is side-effect-free)…
        assert_eq!(plan["isError"], json!(false));
        let blockers = plan["structuredContent"]["plan"]["blockers"]
            .as_array()
            .unwrap();
        assert!(
            !blockers.is_empty(),
            "expected a blocker on deleting current branch"
        );
        let plan_id = plan["structuredContent"]["plan_id"]
            .as_str()
            .unwrap()
            .to_string();
        // …but confirm refuses it, and the branch still exists.
        let r = call(&mut s, "kagi_confirm", json!({ "plan_id": plan_id }));
        assert_eq!(r["isError"], json!(true), "blocked plan must be refused");
        assert!(branch_exists(dir.path(), "main"));
    }

    #[test]
    fn tools_list_annotations_derive_from_plan_classification() {
        // #332 PM-locked: annotations come from OperationPlan.destructive
        // (ADR-0004/0023), not a hand-written table. Rebuild the fold from
        // REAL plans and require tools/list to advertise exactly that — this
        // fails if anyone hardcodes a wrong hint or the classification moves.
        let dir = temp_repo();
        let backend = kagi_git::Backend::discover(dir.path()).unwrap();
        let head = head_sha(dir.path());
        let mut any_destructive = false;
        for op_name in write::SUPPORTED_OPS {
            let args: Vec<String> = match *op_name {
                "checkout" | "create-branch" | "delete-branch" => vec!["main".into()],
                "discard" => vec!["a.txt".into()],
                "reset" => vec![head.clone()],
                other => panic!("no representative args for op '{}'", other),
            };
            let op = write::build_operation(&backend, op_name, &args).unwrap();
            let plan = backend.plan(&op).unwrap();
            any_destructive |= plan.destructive;
        }
        // No network op (fetch/push) is in the supported set.
        let any_network = write::SUPPORTED_OPS
            .iter()
            .any(|op| matches!(*op, "fetch" | "push" | "pull"));

        let mut s = Server::new(dir.path());
        let resp = s.handle(&req(1, "tools/list", json!({}))).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap().clone();
        for t in &tools {
            let name = t["name"].as_str().unwrap();
            let ann = &t["annotations"];
            if name == "kagi_confirm" {
                assert_eq!(
                    ann["destructiveHint"],
                    json!(any_destructive),
                    "kagi_confirm destructiveHint must equal the fold of \
                     OperationPlan.destructive over SUPPORTED_OPS"
                );
                assert_eq!(ann["openWorldHint"], json!(any_network));
                assert_eq!(ann["readOnlyHint"], json!(false));
            } else {
                // Every other tool (reads + kagi_plan) is side-effect-free.
                assert_eq!(ann["readOnlyHint"], json!(true), "{} not readOnly", name);
                assert_ne!(ann["destructiveHint"], json!(true), "{}", name);
            }
        }
    }

    #[test]
    fn readonly_removes_confirm_from_tools_list_but_keeps_plan() {
        let mut s = Server::new(".");
        s.set_readonly(true);
        let resp = s.handle(&req(1, "tools/list", json!({}))).unwrap();
        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(
            !names.contains(&"kagi_confirm"),
            "write tool must be absent"
        );
        assert!(
            names.contains(&"kagi_plan"),
            "plan is inspection — it stays"
        );
        assert!(names.contains(&"kagi_repo_status"));
    }

    #[test]
    fn readonly_confirm_call_is_method_not_found() {
        let dir = temp_repo();
        let mut s = Server::new(dir.path());
        s.set_readonly(true);
        let resp = s
            .handle(&req(
                1,
                "tools/call",
                json!({ "name": "kagi_confirm", "arguments": { "plan_id": "x" } }),
            ))
            .unwrap();
        assert_eq!(resp["error"]["code"], json!(-32601), "resp: {}", resp);
        // Reads and planning still work in read-only mode.
        let r = call(&mut s, "kagi_repo_status", json!({}));
        assert_eq!(r["isError"], json!(false));
        let p = call(
            &mut s,
            "kagi_plan",
            json!({ "op": "create-branch", "args": ["ro-branch"] }),
        );
        assert_eq!(p["isError"], json!(false));
        assert!(!branch_exists(dir.path(), "ro-branch"));
    }

    // ── helpers ──
    fn head_sha(repo: &std::path::Path) -> String {
        let o = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    }
    fn branch_exists(repo: &std::path::Path, name: &str) -> bool {
        std::process::Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{}", name)])
            .current_dir(repo)
            .output()
            .unwrap()
            .status
            .success()
    }
}
