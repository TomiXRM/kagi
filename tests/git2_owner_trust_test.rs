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

/// #416 regression: the owner-trust gate must also cover the many `pub`
/// `execute_*` mutators the UI calls **directly** (they never reach
/// `Backend::run`, where the only pre-#416 gate lived). An untrusted backend
/// must refuse each of them, while read-only inspection still succeeds.
///
/// The guard lives at the entry of each backend method (`require_trust`), so it
/// fires before any git work — plans/entries built while trusted are only there
/// to satisfy the call signatures.
#[test]
fn untrusted_repo_refuses_direct_execute_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    build_repo(d);

    // Build everything the guarded calls need *while trusted*.
    std::fs::write(d.join("work.txt"), "dirty\n").unwrap();
    git(d, &["stash", "push", "-u", "-m", "wip"]);
    let mut backend = Backend::open(d).expect("open");
    assert_eq!(backend.trust(), RepoTrust::Trusted);

    let history = backend.history_from_reflog().expect("reflog history");
    let undo_entry = history.last().expect("at least one history entry").clone();
    let prune_plan = backend.plan_prune_worktrees().expect("prune plan");

    // Now simulate the foreign-owner outcome.
    backend.set_trust_for_test(RepoTrust::Untrusted);

    // Control: read-only inspection still works untrusted.
    assert!(
        backend.head_state().is_ok(),
        "reads must stay allowed while untrusted"
    );
    assert!(
        backend.unstaged_diffstat().is_ok(),
        "diffstat inspection must stay allowed while untrusted"
    );

    // index (stage/unstage)
    assert!(
        backend
            .stage_file(Path::new("work.txt"))
            .unwrap_err()
            .is_untrusted(),
        "stage_file must be refused"
    );
    assert!(
        backend
            .unstage_file(Path::new("work.txt"))
            .unwrap_err()
            .is_untrusted(),
        "unstage_file must be refused"
    );
    // snapshot
    assert!(
        backend.create_snapshot("x").unwrap_err().is_untrusted(),
        "create_snapshot must be refused"
    );
    assert!(
        backend.prune_snapshots(1).unwrap_err().is_untrusted(),
        "prune_snapshots must be refused"
    );
    // stash-drop
    assert!(
        backend.execute_stash_drop(0).unwrap_err().is_untrusted(),
        "execute_stash_drop must be refused"
    );
    // undo/redo (history rewrite)
    assert!(
        backend
            .execute_undo(&undo_entry)
            .unwrap_err()
            .is_untrusted(),
        "execute_undo must be refused"
    );
    assert!(
        backend
            .execute_redo(&undo_entry)
            .unwrap_err()
            .is_untrusted(),
        "execute_redo must be refused"
    );
    // worktree lifecycle
    assert!(
        backend
            .execute_prune_worktrees(&prune_plan)
            .unwrap_err()
            .is_untrusted(),
        "execute_prune_worktrees must be refused"
    );

    // The stash entry must still be intact (drop never ran).
    let stash_list = Command::new("git")
        .args(["stash", "list"])
        .current_dir(d)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", d)
        .output()
        .expect("git");
    assert!(
        !String::from_utf8_lossy(&stash_list.stdout)
            .trim()
            .is_empty(),
        "the stash must survive the refused drop"
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
