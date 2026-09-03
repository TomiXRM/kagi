//! Integration tests for the git2 owner-trust gate (ADR-0160, issue #310).
//!
//! ADR-0146 hardened the CLI path; git 2.35.2+ enforces `safe.directory` on
//! every CLI call, so `run_git` inherits it. The git2 (libgit2) path does NOT —
//! libgit2 ignores `safe.directory`. This gate closes that hole at
//! `Backend::open`: a foreign-owned, untrusted repo opened via git2 is still
//! **readable** but every **mutating** op (`Backend::run`) is refused until the
//! user grants trust.
//!
//! Test sandboxes cannot `chown` a file to another uid, so the uid comparison is
//! injected through the documented `set_trust_for_test` seam (see
//! `trust::evaluate_trust`, which lifts `foreign_uid` out as a param and is
//! unit-tested directly in `crates/kagi-git/src/trust.rs`). Here we drive the
//! *consequence*: an `Untrusted` backend refuses writes, a trusted one proceeds.

use std::path::Path;
use std::process::Command;

use kagi_git::trust::RepoTrust;
use kagi_git::{Backend, CommitId, Head, Operation};

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

fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
}

fn head_commit(backend: &Backend) -> CommitId {
    match backend.head_state().expect("head") {
        Head::Attached { target, .. } => CommitId(target),
        other => panic!("expected attached HEAD, got {other:?}"),
    }
}

/// (1) 受け入れ条件: a foreign-uid repo opened via git2 does NOT proceed to a
/// dangerous op without trust — and (2) a legit shared repo can be allowed via
/// trust confirmation, after which the same op proceeds.
#[test]
fn untrusted_repo_refuses_write_then_proceeds_once_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    build_repo(d);

    let mut backend = Backend::open(d).expect("open");
    // A repo we own opens Trusted.
    assert_eq!(backend.trust(), RepoTrust::Trusted);

    let op = Operation::CreateBranch {
        name: "feature".to_string(),
        at: head_commit(&backend),
    };
    let plan = backend.plan(&op).expect("plan");

    // Simulate the foreign-owner outcome (uid mismatch, not in safe.directory or
    // trust store) via the documented seam.
    backend.set_trust_for_test(RepoTrust::Untrusted);

    // Reads still work while untrusted (inspection is allowed).
    assert!(
        backend.head_state().is_ok(),
        "reads allowed while untrusted"
    );

    // The mutating op is refused — the dangerous op never runs.
    let err = backend
        .run(&op, &plan)
        .expect_err("write must be refused while untrusted");
    assert!(err.is_untrusted(), "expected Untrusted, got {err:?}");
    assert!(
        !branch_exists(d, "feature"),
        "the branch must NOT have been created while untrusted"
    );

    // Granting trust (what the UI's confirm_trust_repo does: trust_repo + the
    // next open re-evaluates as Trusted; the seam mirrors that re-open).
    backend.set_trust_for_test(RepoTrust::Trusted);
    backend
        .run(&op, &plan)
        .expect("write proceeds once trusted");
    assert!(
        branch_exists(d, "feature"),
        "the branch is created after trust is granted"
    );
}

fn branch_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{name}")])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git")
        .status
        .success()
}
