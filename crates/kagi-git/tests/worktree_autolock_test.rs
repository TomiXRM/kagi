//! #772 Phase 1 (ADR-0208): the automatic release only ever removes the lock
//! this terminal session placed, on the worktree it placed it on. Everything
//! else — a manual lock, another session's Kagi lock, a lock read from a
//! different worktree, no lock at all — is refused at plan *and* at preflight,
//! and the lock file is left exactly as it was.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use std::path::{Path, PathBuf};

use kagi_domain::plan_note::{PlanNote, WorktreeNote};
use kagi_domain::worktree_autolock::{AutoLockToken, AutoUnlockRefusal, AutoUnlockTarget};
use kagi_git::backend::ExecutionPolicy;
use kagi_git::Backend;
use tempfile::TempDir;

/// Main repo + linked worktrees `alpha` and `beta`, each on its own branch.
fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let main = tmp.path().join("main");
    std::fs::create_dir(&main).unwrap();
    init_repo(&main, "main");
    write_file(&main, "a.txt", "a\n");
    commit_all(&main, "base");
    let alpha = tmp.path().join("alpha");
    let beta = tmp.path().join("beta");
    git(
        &main,
        &["worktree", "add", "-b", "alpha", alpha.to_str().unwrap()],
    );
    git(
        &main,
        &["worktree", "add", "-b", "beta", beta.to_str().unwrap()],
    );
    (tmp, main, alpha)
}

fn backend(path: &Path) -> Backend {
    Backend::open_with_policy(path, ExecutionPolicy::human(false)).unwrap()
}

/// `git worktree list --porcelain` view of `name`'s lock: `Some(reason)` when
/// locked (reason may be empty), `None` when unlocked. Read through plain git,
/// not the code under test.
fn lock_state(main: &Path, name: &str) -> Option<String> {
    let out = git_output(main, &["worktree", "list", "--porcelain"]);
    let mut in_block = false;
    for line in out.lines() {
        if line.starts_with("worktree ") {
            in_block = line.ends_with(&format!("/{name}"));
        } else if in_block && line.starts_with("locked") {
            return Some(line.trim_start_matches("locked").trim().to_string());
        }
    }
    None
}

fn target(main: &Path, name: &str, owner: &str) -> AutoUnlockTarget {
    AutoUnlockTarget {
        token: AutoLockToken::new(owner).unwrap(),
        worktree: backend(main).linked_worktree_identity(name).unwrap(),
    }
}

fn refusal(plan: &kagi_git::OperationPlan) -> Option<AutoUnlockRefusal> {
    plan.blockers.iter().find_map(|b| match b {
        PlanNote::Worktree(WorktreeNote::AutoUnlockRefused { refusal, .. }) => {
            Some(refusal.clone())
        }
        _ => None,
    })
}

#[test]
fn acquire_with_token_then_release_only_by_the_same_session() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, main, _alpha) = fixture();
    let b = backend(&main);
    let ours = target(&main, "alpha", "session-1");

    // Acquire through the ordinary lock op, reason = the token.
    let plan = b
        .plan_lock_worktree("alpha", Some(&ours.token.reason()))
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    b.execute_lock_worktree(&plan, "alpha", Some(&ours.token.reason()))
        .unwrap();
    assert_eq!(
        lock_state(&main, "alpha").as_deref(),
        Some("kagi:auto:session-1")
    );

    // Another session may not release it.
    let theirs = target(&main, "alpha", "session-2");
    let plan = b.plan_auto_unlock_worktree("alpha", &theirs).unwrap();
    assert_eq!(
        refusal(&plan),
        Some(AutoUnlockRefusal::TokenMismatch {
            found: ours.token.clone()
        })
    );
    assert!(b
        .execute_auto_unlock_worktree(&plan, "alpha", &theirs)
        .is_err());
    assert_eq!(
        lock_state(&main, "alpha").as_deref(),
        Some("kagi:auto:session-1")
    );

    // The owner may.
    let plan = b.plan_auto_unlock_worktree("alpha", &ours).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    b.execute_auto_unlock_worktree(&plan, "alpha", &ours)
        .unwrap();
    assert_eq!(lock_state(&main, "alpha"), None);

    // And once it is gone, a second release is a refusal, not a no-op success.
    let plan = b.plan_auto_unlock_worktree("alpha", &ours).unwrap();
    assert_eq!(refusal(&plan), Some(AutoUnlockRefusal::NotLocked));
}

#[test]
fn a_manual_lock_is_never_released_automatically() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, main, alpha) = fixture();
    git(
        &main,
        &[
            "worktree",
            "lock",
            "--reason",
            "agent still running",
            alpha.to_str().unwrap(),
        ],
    );
    let b = backend(&main);
    let ours = target(&main, "alpha", "session-1");
    let plan = b.plan_auto_unlock_worktree("alpha", &ours).unwrap();
    assert_eq!(
        refusal(&plan),
        Some(AutoUnlockRefusal::NotAutoLock {
            reason: Some("agent still running".into())
        })
    );
    assert!(b
        .execute_auto_unlock_worktree(&plan, "alpha", &ours)
        .is_err());
    assert_eq!(
        lock_state(&main, "alpha").as_deref(),
        Some("agent still running")
    );

    // A manual lock with no reason is just as untouchable.
    git(&main, &["worktree", "unlock", alpha.to_str().unwrap()]);
    git(&main, &["worktree", "lock", alpha.to_str().unwrap()]);
    let plan = b.plan_auto_unlock_worktree("alpha", &ours).unwrap();
    assert_eq!(
        refusal(&plan),
        Some(AutoUnlockRefusal::NotAutoLock { reason: None })
    );
    assert!(b
        .execute_auto_unlock_worktree(&plan, "alpha", &ours)
        .is_err());
    assert_eq!(lock_state(&main, "alpha").as_deref(), Some(""));

    // The manual unlock op still works on it (contract D: recovery is manual).
    let plan = b.plan_unlock_worktree("alpha").unwrap();
    b.execute_unlock_worktree(&plan, "alpha").unwrap();
    assert_eq!(lock_state(&main, "alpha"), None);
}

#[test]
fn the_recorded_worktree_identity_must_match() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, main, _alpha) = fixture();
    let b = backend(&main);
    // Our token, but the session recorded `beta` as its target: the lock on
    // `alpha` carries our reason yet is not ours to release.
    let token = AutoLockToken::new("session-1").unwrap();
    let plan = b
        .plan_lock_worktree("alpha", Some(&token.reason()))
        .unwrap();
    b.execute_lock_worktree(&plan, "alpha", Some(&token.reason()))
        .unwrap();
    let wrong = AutoUnlockTarget {
        token: token.clone(),
        worktree: b.linked_worktree_identity("beta").unwrap(),
    };
    let plan = b.plan_auto_unlock_worktree("alpha", &wrong).unwrap();
    assert_eq!(refusal(&plan), Some(AutoUnlockRefusal::IdentityMismatch));
    assert!(b
        .execute_auto_unlock_worktree(&plan, "alpha", &wrong)
        .is_err());
    assert_eq!(
        lock_state(&main, "alpha").as_deref(),
        Some("kagi:auto:session-1")
    );
}

#[test]
fn preflight_refuses_a_lock_that_changed_after_planning() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, main, alpha) = fixture();
    let b = backend(&main);
    let ours = target(&main, "alpha", "session-1");
    let plan = b
        .plan_lock_worktree("alpha", Some(&ours.token.reason()))
        .unwrap();
    b.execute_lock_worktree(&plan, "alpha", Some(&ours.token.reason()))
        .unwrap();
    // Plan says "ours" …
    let plan = b.plan_auto_unlock_worktree("alpha", &ours).unwrap();
    assert!(plan.blockers.is_empty());
    // … then someone relocks it by hand before we act.
    git(&main, &["worktree", "unlock", alpha.to_str().unwrap()]);
    git(
        &main,
        &[
            "worktree",
            "lock",
            "--reason",
            "taken over",
            alpha.to_str().unwrap(),
        ],
    );
    let err = b
        .execute_auto_unlock_worktree(&plan, "alpha", &ours)
        .unwrap_err()
        .to_string();
    assert!(err.contains("refused at preflight"), "{err}");
    assert_eq!(lock_state(&main, "alpha").as_deref(), Some("taken over"));
}

#[test]
fn identity_matches_a_backend_opened_at_the_worktree() {
    if !test_support::run_isolated() {
        return;
    }
    let (_tmp, main, alpha) = fixture();
    let from_main = backend(&main).linked_worktree_identity("alpha").unwrap();
    let from_inside = backend(&alpha).write_worktree_id().unwrap();
    assert_eq!(from_main, from_inside);
    assert_ne!(
        from_main,
        backend(&main).linked_worktree_identity("beta").unwrap()
    );
}
