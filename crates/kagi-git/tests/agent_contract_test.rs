//! The shared CLI/MCP agent contract (#509) and its receipt binding (#505).
//!
//! These are the seam-level regressions: `api::confirm_response` must describe
//! the entry *this* run produced, so reverting it to a global `read_oplog_tail`
//! fails here rather than in production under a concurrent writer.

use kagi_domain::plan::StateSummary;
use kagi_git::{api, Backend, OpLogEntry, OpOutcome, Operation};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// The oplog path is process-global (`KAGI_LOG_DIR`), so these tests serialize.
static ENV: Mutex<()> = Mutex::new(());

fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "alpha\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "base"]);
    tmp
}

/// An entry from some *other* repository — what a concurrent writer would put
/// at the tail of the shared log while this process is executing.
fn foreign_entry(op: &str) -> OpLogEntry {
    let state = |s: &str| StateSummary {
        head: s.to_string(),
        dirty: "clean".to_string(),
    };
    OpLogEntry::new(
        op,
        "/somewhere/else".to_string(),
        state("branch: other"),
        OpOutcome::Success {
            after: state("branch: other"),
        },
    )
}

#[test]
fn confirm_response_names_this_runs_entry_not_the_global_tail() {
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());

    let tmp = fixture();
    let mut backend =
        Backend::discover_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::cli())
            .unwrap();

    kagi_git::append_oplog(&foreign_entry("checkout-before")).unwrap();

    let op = api::resolve_operation(&backend, "create-branch", &["feature".to_string()]).unwrap();
    let plan = backend.plan(&op).unwrap();
    let plan_id = plan.plan_id();
    let report = backend.run_recorded(&op, &plan);
    let outcome = report.result.expect("create-branch should succeed");

    // A concurrent writer lands an unrelated entry *after* our run — exactly the
    // window the old `read_oplog_tail(1)` echo lost.
    kagi_git::append_oplog(&foreign_entry("checkout-after")).unwrap();

    let response = api::confirm_response(&op, &plan_id, &outcome, &report.recording);
    assert_eq!(response["status"], "ok");
    assert_eq!(response["recorded"], true);
    assert_eq!(response["recording_error"], serde_json::Value::Null);
    assert_eq!(response["oplog"]["op"], "create-branch");
    assert_eq!(response["oplog"]["actor"], "cli");

    let tail = kagi_git::read_oplog_tail(1);
    assert_eq!(tail[0].op, "checkout-after", "fixture sanity");
    assert_ne!(
        response["oplog"]["id"],
        serde_json::json!(tail[0].id),
        "the response must be bound to its own receipt, not to the global tail"
    );

    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn same_repo_same_op_runs_report_their_own_entries() {
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());

    let tmp = fixture();
    let mut backend =
        Backend::discover_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::cli())
            .unwrap();

    let mut ids = Vec::new();
    for name in ["one", "two"] {
        let op = api::resolve_operation(&backend, "create-branch", &[name.to_string()]).unwrap();
        let plan = backend.plan(&op).unwrap();
        let plan_id = plan.plan_id();
        let report = backend.run_recorded(&op, &plan);
        let outcome = report.result.unwrap();
        let response = api::confirm_response(&op, &plan_id, &outcome, &report.recording);
        ids.push(response["oplog"]["id"].clone());
    }
    // A repo+op filter cannot tell these two apart; the receipts can.
    assert_ne!(ids[0], ids[1], "two runs of the same op share an entry id");

    std::env::remove_var("KAGI_LOG_DIR");
}

#[cfg(unix)]
#[test]
fn a_failed_append_is_reported_as_unrecorded_not_as_an_older_entry() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());

    // Seed a readable past entry, then make the log file unwritable so the
    // append fails while `read_oplog_tail(1)` still returns that stale entry —
    // which is precisely what the old code would have echoed as "ours".
    kagi_git::append_oplog(&foreign_entry("checkout-stale")).unwrap();
    let path = log.path().join("operations.jsonl");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let enforced = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .is_err();

    let tmp = fixture();
    let mut backend =
        Backend::discover_with_policy(tmp.path(), kagi_git::backend::ExecutionPolicy::cli())
            .unwrap();
    let op = api::resolve_operation(&backend, "create-branch", &["feature".to_string()]).unwrap();
    let plan = backend.plan(&op).unwrap();
    let plan_id = plan.plan_id();
    let report = backend.run_recorded(&op, &plan);
    let outcome = report.result.expect("the branch is still created");
    let response = api::confirm_response(&op, &plan_id, &outcome, &report.recording);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::env::remove_var("KAGI_LOG_DIR");

    // Running as root defeats the permission bit; skip rather than assert a
    // property the filesystem did not enforce.
    if !enforced {
        eprintln!("skipping: append succeeded despite a read-only oplog (root?)");
        return;
    }
    assert_eq!(
        response["status"], "ok",
        "the mutation happened and must not be reported as failed"
    );
    assert_eq!(response["recorded"], false);
    assert!(
        response["recording_error"].is_string(),
        "recording failure must be named: {response}"
    );
    assert_eq!(
        response["oplog"]["op"], "create-branch",
        "the attempted entry is ours, never the stale tail: {response}"
    );
    assert_ne!(response["oplog"]["op"], "checkout-stale");
}

#[test]
fn resolve_operation_covers_exactly_the_advertised_ops() {
    let tmp = fixture();
    let backend = Backend::discover(tmp.path()).unwrap();
    let head = git(tmp.path(), &["rev-parse", "HEAD"]);
    for name in api::SUPPORTED_OPS {
        let args = vec![match *name {
            "reset" => head.clone(),
            "discard" => "a.txt".to_string(),
            _ => "main".to_string(),
        }];
        assert!(
            api::resolve_operation(&backend, name, &args).is_ok(),
            "advertised op '{name}' does not resolve"
        );
        // Every advertised op is named in the one usage string both frontends print.
        assert!(
            api::OPS_USAGE.contains(name),
            "'{name}' missing from OPS_USAGE"
        );
        // Missing arguments are refused with the same words on both frontends.
        assert_eq!(
            api::resolve_operation(&backend, name, &[]).unwrap_err(),
            format!("`{name}` needs 1 argument(s)")
        );
    }
    assert_eq!(
        api::resolve_operation(&backend, "push --force", &[]).unwrap_err(),
        "unsupported op 'push --force'"
    );
    assert!(matches!(
        api::resolve_operation(&backend, "checkout", &["main".to_string()]).unwrap(),
        Operation::Checkout { .. }
    ));
}
