//! Branch context menu operation backend tests (T-BCM-030/061/073).

use std::path::Path;
use std::process::Command;

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi::git::ops::{
    default_tracking_branch_name, execute_checkout_tracking_branch, execute_merge_into_conflict,
    plan_checkout_tracking_branch, plan_merge_branch, MergeKind,
};
use kagi::git::{
    detect_conflict_session, execute_conflict_abort, plan_conflict_abort, ResolutionBuffer,
};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

fn git_rev_parse(dir: &Path, rev: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git rev-parse failed to start");
    assert!(output.status.success(), "git rev-parse {} failed", rev);
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
    Repository::open(dir).expect("open repo")
}

#[test]
fn merge_plan_reports_fast_forward_direction() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);

    let (plan, kind) = plan_merge_branch(&repo, "feature").expect("plan merge");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert_eq!(kind, MergeKind::FastForward);
    assert_eq!(plan.title, "Merge feature into main");
    assert!(
        plan.predicted.head.contains("fast-forward"),
        "expected ff plan, got {}",
        plan.predicted.head
    );
    assert!(plan
        .preview_files
        .iter()
        .any(|f| f.path == Path::new("feature.txt")));
}

#[test]
fn merge_plan_reports_merge_commit_for_diverged_branch() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    let (plan, kind) = plan_merge_branch(&repo, "feature").expect("plan merge");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert_eq!(kind, MergeKind::MergeCommit);
    assert_eq!(plan.title, "Merge feature into main");
    assert!(
        plan.predicted.head.contains("merge commit"),
        "expected merge-commit plan, got {}",
        plan.predicted.head
    );
}

/// Build a repo whose `feature` branch conflicts with `main` on `same.txt`,
/// HEAD on `main`. Returns the TempDir (keep alive) + opened repo.
fn conflicting_merge_repo() -> (TempDir, Repository) {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "same.txt", "base\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "same base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "same.txt", "feature\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "same.txt", "main\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "main change"]);

    (tmp, repo)
}

/// W31-MERGE-INTO-CONFLICT: a predicted conflict is NO LONGER a blocker. The
/// plan must leave `blockers` empty, return `MergeKind::Conflicts` listing the
/// file(s), warn about the conflict, and not touch the working tree.
#[test]
fn merge_plan_conflict_is_warning_not_blocker_and_leaves_worktree_intact() {
    let (tmp, repo) = conflicting_merge_repo();
    let dir = tmp.path();

    let before = std::fs::read_to_string(dir.join("same.txt")).unwrap();
    let (plan, kind) = plan_merge_branch(&repo, "feature").expect("plan merge");
    let after = std::fs::read_to_string(dir.join("same.txt")).unwrap();

    assert!(
        plan.blockers.is_empty(),
        "predicted conflict must NOT be a blocker, got {:?}",
        plan.blockers
    );
    match &kind {
        MergeKind::Conflicts(files) => {
            assert!(
                files.iter().any(|f| f == "same.txt"),
                "expected same.txt in conflict files, got {:?}",
                files
            );
        }
        other => panic!("expected MergeKind::Conflicts, got {:?}", other),
    }
    assert!(
        plan.warnings.iter().any(|w| w.contains("same.txt")),
        "expected a warning mentioning the conflicted file, got {:?}",
        plan.warnings
    );
    assert!(
        plan.predicted.dirty.contains("conflicted"),
        "predicted.dirty should mention conflicted files, got {}",
        plan.predicted.dirty
    );
    assert_eq!(before, after, "plan must not modify working tree");
}

/// W31: executing the conflicting merge leaves the standard git "merging with
/// conflicts" state (index conflicts + MERGE_HEAD), and the existing conflict
/// abort restores the pre-merge clean HEAD.
#[test]
fn execute_merge_into_conflict_then_abort_restores_pre_merge_state() {
    let (tmp, repo) = conflicting_merge_repo();
    let dir = tmp.path();

    let head_before = git_rev_parse(dir, "HEAD");

    let files = execute_merge_into_conflict(&repo, "feature").expect("execute merge into conflict");
    assert!(
        files.iter().any(|f| f == "same.txt"),
        "expected same.txt conflicted, got {:?}",
        files
    );

    // Standard merging-with-conflicts state: MERGE_HEAD written, index has
    // conflicts, and Conflict Mode detection recognises a Merge session.
    assert!(
        dir.join(".git").join("MERGE_HEAD").exists(),
        "MERGE_HEAD should exist after merge into conflict"
    );
    let index = repo.index().unwrap();
    assert!(index.has_conflicts(), "index should hold conflict stages");
    drop(index);

    let session = detect_conflict_session(&repo).expect("conflict session should be detected");
    assert_eq!(session.unresolved_count(), 1);

    // ORIG_HEAD records the pre-merge HEAD so abort can roll back.
    assert_eq!(git_rev_parse(dir, "ORIG_HEAD"), head_before);

    // Abort via the existing conflict-abort path restores the pre-merge state.
    let buffer = ResolutionBuffer::from_repo(&repo)
        .ok()
        .unwrap_or_else(|| ResolutionBuffer::new(dir));
    let _plan = plan_conflict_abort(&repo, &session).expect("plan abort");
    execute_conflict_abort(&repo, &session, &buffer).expect("execute abort");

    assert_eq!(
        git_rev_parse(dir, "HEAD"),
        head_before,
        "HEAD should be restored to the pre-merge commit"
    );
    assert!(
        !dir.join(".git").join("MERGE_HEAD").exists(),
        "MERGE_HEAD should be cleared after abort"
    );
    assert!(
        detect_conflict_session(&repo).is_none(),
        "no conflict session should remain after abort"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("same.txt")).unwrap(),
        "main\n",
        "working tree should be restored to the pre-merge main content"
    );
}

#[test]
fn checkout_tracking_branch_plan_and_execute_from_file_remote() {
    let repo_tmp = TempDir::new().unwrap();
    let remote_tmp = TempDir::new().unwrap();
    let repo = init_repo(&repo_tmp);
    let dir = repo_tmp.path();
    let remote_url = format!("file://{}", remote_tmp.path().display());

    git(remote_tmp.path(), &["init", "-q", "--bare", "."]);
    git(dir, &["remote", "add", "origin", &remote_url]);
    git(dir, &["push", "-q", "-u", "origin", "main"]);
    git(dir, &["checkout", "-qb", "remote-only"]);
    write_file(dir, "remote.txt", "remote\n");
    git(dir, &["add", "remote.txt"]);
    git(dir, &["commit", "-qm", "remote only"]);
    git(dir, &["push", "-q", "origin", "remote-only"]);
    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["branch", "-D", "remote-only"]);
    git(dir, &["fetch", "-q", "origin"]);

    assert_eq!(
        default_tracking_branch_name("origin/remote-only"),
        "remote-only"
    );
    let plan = plan_checkout_tracking_branch(&repo, "origin/remote-only", "remote-only")
        .expect("tracking plan");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );

    execute_checkout_tracking_branch(&repo, "origin/remote-only", "remote-only")
        .expect("checkout tracking");

    let repo2 = Repository::open(dir).unwrap();
    assert_eq!(repo2.head().unwrap().shorthand().unwrap(), "remote-only");
    let branch = repo2
        .find_branch("remote-only", BranchType::Local)
        .expect("local branch");
    assert_eq!(
        branch.upstream().unwrap().name().unwrap(),
        Some("origin/remote-only")
    );
}

#[test]
fn checkout_tracking_branch_name_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let remote_tmp = TempDir::new().unwrap();
    let repo = init_repo(&repo_tmp);
    let dir = repo_tmp.path();
    let remote_url = format!("file://{}", remote_tmp.path().display());

    git(remote_tmp.path(), &["init", "-q", "--bare", "."]);
    git(dir, &["remote", "add", "origin", &remote_url]);
    git(dir, &["push", "-q", "-u", "origin", "main"]);
    git(dir, &["fetch", "-q", "origin"]);

    let plan = plan_checkout_tracking_branch(&repo, "origin/main", "main").expect("tracking plan");
    assert!(
        plan.blockers.iter().any(|b| b.contains("already exists")),
        "expected collision blocker, got {:?}",
        plan.blockers
    );
}
