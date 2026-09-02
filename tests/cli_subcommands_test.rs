//! #330 acceptance tests — the headless `kagi` CLI (`plan` / `confirm` /
//! `status` / `oplog`), exercised end-to-end by spawning the built binary
//! against a real git fixture. NO GUI is involved.
//!
//! What each test pins (issue §6):
//!   * `plan checkout <branch>` returns an `OperationPlan` envelope as JSON with
//!     NO side effects (the repo is asserted unchanged).
//!   * `confirm` runs preflight → execute → verify → oplog: the op happens AND
//!     an oplog entry (actor=cli) is written (this also exercises #329).
//!   * a repo change between plan and confirm → `confirm` is REFUSED with an
//!     id mismatch whose error names WHAT changed.
//!   * a plan with a blocker → `confirm` refused.
//!   * a destructive op is refused without `--yes` and proceeds with it.
//!
//! Mutation note: the stale-confirm test asserts `status == "refused"`. If the
//! plan-id match check in `cmd_confirm` is removed, `confirm` would execute the
//! stale checkout and print `status:"ok"`, so this test fails — verified by
//! hand (comment out the `fresh.plan_id() != envelope.plan_id` guard).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

/// Fixture: one commit on `main`, plus an empty `feature` branch. Clean tree.
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("a.txt"), "hi\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
    git(dir, &["branch", "feature"]);
}

struct Run {
    stdout: String,
    code: i32,
}

/// Run the kagi binary with the given args, isolating the oplog under `log_dir`.
/// `stdin` is fed to the child's stdin when `Some`.
fn kagi(repo: &Path, log_dir: &Path, args: &[&str], stdin: Option<&str>) -> Run {
    use std::io::Write;
    use std::process::Stdio;
    let exe = env!("CARGO_BIN_EXE_kagi");
    let mut cmd = Command::new(exe);
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
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON: {e}\n---\n{s}"))
}

#[test]
fn plan_checkout_is_json_with_no_side_effects() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());

    let before_head = std::fs::read_to_string(repo.path().join(".git/HEAD")).unwrap();

    let r = kagi(
        repo.path(),
        logs.path(),
        &[
            "plan",
            "checkout",
            "feature",
            "--repo",
            repo.path().to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert_eq!(r.code, 0, "plan exit; stdout={}", r.stdout);
    let v = json(&r.stdout);
    assert!(v["plan_id"].is_string(), "has plan_id: {}", r.stdout);
    assert_eq!(v["op"], "checkout");
    assert_eq!(v["args"][0], "feature");
    assert_eq!(v["plan"]["current"]["head"], "branch: main");
    assert_eq!(v["plan"]["predicted"]["head"], "branch: feature");

    // No side effects: HEAD unmoved, tree clean, no oplog written.
    let after_head = std::fs::read_to_string(repo.path().join(".git/HEAD")).unwrap();
    assert_eq!(before_head, after_head, "plan must not move HEAD");
    assert!(
        !logs.path().join("operations.jsonl").exists(),
        "plan writes no oplog"
    );
}

#[test]
fn confirm_runs_pipeline_and_writes_oplog() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    let plan = kagi(
        repo.path(),
        logs.path(),
        &["plan", "checkout", "feature", "--repo", repo_s],
        None,
    );
    assert_eq!(plan.code, 0);

    // Standalone confirm: pipe the plan JSON back in via stdin.
    let c = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.stdout),
    );
    assert_eq!(c.code, 0, "confirm exit; stdout={}", c.stdout);
    let v = json(&c.stdout);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["op"], "checkout");
    assert_eq!(v["oplog"]["actor"], "cli", "op recorded with actor=cli");
    assert_eq!(v["oplog"]["op"], "checkout");

    // The op actually happened: HEAD is now on feature.
    let head = std::fs::read_to_string(repo.path().join(".git/HEAD")).unwrap();
    assert!(head.contains("refs/heads/feature"), "HEAD moved: {head}");

    // And exactly the oplog file exists with one entry.
    let log = std::fs::read_to_string(logs.path().join("operations.jsonl")).unwrap();
    assert_eq!(log.lines().filter(|l| !l.trim().is_empty()).count(), 1);
}

#[test]
fn confirm_refused_when_repo_changed_names_what_changed() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    let plan = kagi(
        repo.path(),
        logs.path(),
        &["plan", "checkout", "feature", "--repo", repo_s],
        None,
    );
    assert_eq!(plan.code, 0);

    // External change between plan and confirm: a new commit moves HEAD's SHA.
    std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
    git(repo.path(), &["commit", "-qam", "c2"]);

    let c = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.stdout),
    );
    assert_eq!(c.code, 2, "stale confirm refused; stdout={}", c.stdout);
    let v = json(&c.stdout);
    assert_eq!(v["status"], "refused");
    let changed = v["detail"]["changed"].as_array().expect("changed[]");
    assert!(
        changed
            .iter()
            .any(|s| s.as_str().unwrap_or("").contains("HEAD changed")),
        "error names what changed: {}",
        c.stdout
    );
    assert_ne!(
        v["detail"]["expected_plan_id"],
        v["detail"]["actual_plan_id"]
    );

    // The refused op did NOT run: HEAD is still on main.
    let head = std::fs::read_to_string(repo.path().join(".git/HEAD")).unwrap();
    assert!(head.contains("refs/heads/main"), "HEAD unchanged: {head}");
}

#[test]
fn confirm_refused_when_plan_has_blocker() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    // Checkout of a branch that does not exist → the plan carries a blocker.
    let plan = kagi(
        repo.path(),
        logs.path(),
        &["plan", "checkout", "ghost", "--repo", repo_s],
        None,
    );
    assert_eq!(plan.code, 0);

    let c = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.stdout),
    );
    assert_eq!(c.code, 2, "blocked confirm refused; stdout={}", c.stdout);
    let v = json(&c.stdout);
    assert_eq!(v["status"], "refused");
    assert_eq!(v["reason"], "plan has blockers");
    assert!(v["detail"]["blockers"]
        .as_array()
        .is_some_and(|a| !a.is_empty()));
}

#[test]
fn destructive_confirm_requires_yes() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    // Modify a tracked file so `discard` (a destructive op) has work to do.
    std::fs::write(repo.path().join("a.txt"), "dirty\n").unwrap();

    let plan = kagi(
        repo.path(),
        logs.path(),
        &["plan", "discard", "a.txt", "--repo", repo_s],
        None,
    );
    assert_eq!(plan.code, 0, "plan discard; stdout={}", plan.stdout);
    assert_eq!(json(&plan.stdout)["plan"]["destructive"], true);

    // Without --yes: refused.
    let no = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.stdout),
    );
    assert_eq!(no.code, 2, "no --yes refused; stdout={}", no.stdout);
    assert_eq!(
        json(&no.stdout)["reason"],
        "destructive operation requires --yes"
    );
    // File still dirty (nothing ran).
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "dirty\n"
    );

    // With --yes: proceeds and discards.
    let yes = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--yes", "--repo", repo_s],
        Some(&plan.stdout),
    );
    assert_eq!(yes.code, 0, "with --yes ok; stdout={}", yes.stdout);
    assert_eq!(json(&yes.stdout)["status"], "ok");
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "hi\n",
        "discarded"
    );
}

#[test]
fn status_and_oplog_are_json() {
    let repo = TempDir::new().unwrap();
    let logs = TempDir::new().unwrap();
    build_repo(repo.path());
    let repo_s = repo.path().to_str().unwrap();

    let s = kagi(
        repo.path(),
        logs.path(),
        &["status", "--repo", repo_s, "--json"],
        None,
    );
    assert_eq!(s.code, 0);
    let v = json(&s.stdout);
    assert_eq!(v["head"], "branch: main");
    assert_eq!(v["dirty"], "clean");

    // Run one op so the oplog has content, then read it back as a JSON array.
    let plan = kagi(
        repo.path(),
        logs.path(),
        &["plan", "checkout", "feature", "--repo", repo_s],
        None,
    );
    let _ = kagi(
        repo.path(),
        logs.path(),
        &["confirm", "--repo", repo_s],
        Some(&plan.stdout),
    );

    let o = kagi(
        repo.path(),
        logs.path(),
        &["oplog", "--limit", "10", "--repo", repo_s],
        None,
    );
    assert_eq!(o.code, 0);
    let arr = json(&o.stdout);
    assert!(arr.is_array());
    assert_eq!(arr[0]["op"], "checkout");
    assert_eq!(arr[0]["actor"], "cli");
}
