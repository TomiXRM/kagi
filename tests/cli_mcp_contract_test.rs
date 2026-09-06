//! #509: the CLI and the MCP server are two transports over ONE contract.
//!
//! Both entry points are driven against the same fixture — the CLI by spawning
//! the real binary, MCP in-process through its JSON-RPC handler — and their
//! plan/confirm payloads are compared field by field. If either frontend grows
//! its own operation match or its own plan serializer again, these fail.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("a.txt"), "hi\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
    git(dir, &["branch", "feature"]);
    // Leave the tree dirty so `discard` has something to plan against.
    std::fs::write(dir.join("a.txt"), "hi there\n").unwrap();
}

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Run the kagi binary headless, returning (exit code, parsed stdout).
fn cli(repo: &Path, log_dir: &Path, args: &[&str], stdin: Option<&str>) -> (i32, Value) {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kagi"));
    cmd.args(args)
        .env("KAGI_LOG_DIR", log_dir)
        .env("HOME", repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().expect("spawn kagi");
    if let Some(s) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(s.as_bytes())
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait kagi");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("CLI stdout is not JSON: {e}\n---\n{text}"));
    (out.status.code().unwrap_or(-1), value)
}

/// Call one MCP tool in-process and return its structured content.
fn mcp(server: &mut kagi_mcp::Server, name: &str, args: Value) -> Value {
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": name, "arguments": args },
    });
    let resp = server.handle(&req).expect("tool response");
    let result = &resp["result"];
    assert_eq!(result["isError"], json!(false), "{name} errored: {resp}");
    result["structuredContent"].clone()
}

/// Representative arguments for every advertised op, on this fixture.
fn sample_args(op: &str, head: &str) -> Vec<String> {
    match op {
        "checkout" => vec!["feature".into()],
        "create-branch" => vec!["new-branch".into()],
        "delete-branch" => vec!["feature".into()],
        "discard" => vec!["a.txt".into()],
        "reset" => vec![head.into()],
        other => panic!("no representative args for advertised op '{other}'"),
    }
}

#[test]
fn cli_and_mcp_emit_the_same_plan_for_every_supported_op() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();
    let head = head_sha(repo.path());

    let mut server = kagi_mcp::Server::new(repo.path());
    for op in kagi_git::api::SUPPORTED_OPS {
        let args = sample_args(op, &head);
        let mut argv = vec!["plan", op];
        argv.extend(args.iter().map(String::as_str));
        argv.extend(["--repo", repo_s]);
        let (code, from_cli) = cli(repo.path(), logs.path(), &argv, None);
        assert_eq!(code, 0, "plan {op} failed: {from_cli}");

        let from_mcp = mcp(&mut server, "kagi_plan", json!({ "op": op, "args": args }));

        // Same operation identity, same staleness snapshot, same display block.
        for field in [
            "plan_id",
            "op",
            "args",
            "head_at_plan",
            "stash_count_at_plan",
            "worktree_digest",
            "plan",
        ] {
            assert_eq!(
                from_cli[field], from_mcp[field],
                "'{op}' differs at `{field}`\ncli: {from_cli}\nmcp: {from_mcp}"
            );
        }
        // Blockers and recovery in particular — the two frontends gate on these.
        assert!(from_cli["plan"]["blockers"].is_array());
        assert!(from_cli["plan"]["destructive"].is_boolean());
        // Only the MCP edge's own next-step hint is extra.
        assert!(from_cli.get("next").is_none());
        assert!(from_mcp["next"].is_string());
    }
}

#[test]
fn cli_and_mcp_reject_the_same_unsupported_and_underspecified_requests() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();
    let mut server = kagi_mcp::Server::new(repo.path());

    // An op nobody supports, and a supported op with its argument missing.
    for op in ["push-force", "checkout"] {
        let (code, from_cli) = cli(
            repo.path(),
            logs.path(),
            &["plan", op, "--repo", repo_s],
            None,
        );
        assert_eq!(code, 1, "expected a usage error for '{op}': {from_cli}");
        let cli_error = from_cli["error"].as_str().unwrap().to_string();

        let resp = server
            .handle(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "kagi_plan", "arguments": { "op": op, "args": [] } },
            }))
            .unwrap();
        assert_eq!(resp["result"]["isError"], json!(true));
        let mcp_error = resp["result"]["structuredContent"]["error"]
            .as_str()
            .unwrap();
        assert_eq!(
            mcp_error, cli_error,
            "'{op}' must be refused with the same words on both frontends"
        );
    }
}

#[test]
fn confirm_reports_its_own_oplog_entry_on_both_frontends() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    let (_, plan) = cli(
        repo.path(),
        logs.path(),
        &["plan", "create-branch", "via-cli", "--repo", repo_s],
        None,
    );
    let (code, done) = cli(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.to_string()),
    );
    assert_eq!(code, 0, "confirm failed: {done}");
    assert_eq!(done["status"], "ok");
    assert_eq!(done["recorded"], true);
    assert_eq!(done["recording_error"], Value::Null);
    assert_eq!(done["oplog"]["op"], "create-branch");
    assert_eq!(done["oplog"]["actor"], "cli");

    // MCP's confirm carries the identical envelope, differing only in actor.
    std::env::set_var("KAGI_LOG_DIR", logs.path());
    let mut server = kagi_mcp::Server::new(repo.path());
    let planned = mcp(
        &mut server,
        "kagi_plan",
        json!({ "op": "create-branch", "args": ["via-mcp"] }),
    );
    let confirmed = mcp(
        &mut server,
        "kagi_confirm",
        json!({ "plan_id": planned["plan_id"] }),
    );
    std::env::remove_var("KAGI_LOG_DIR");

    assert_eq!(confirmed["status"], "ok");
    assert_eq!(confirmed["recorded"], true);
    assert_eq!(confirmed["oplog"]["op"], "create-branch");
    assert_eq!(confirmed["oplog"]["actor"], "mcp");
    // Two runs of the same op in the same repo, each bound to its own entry.
    assert_ne!(done["oplog"]["id"], confirmed["oplog"]["id"]);
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&done), keys(&confirmed), "confirm shapes must match");
}

#[path = "support/isolated.rs"]
mod test_support;
