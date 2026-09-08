//! Integration tests for pull — T-HT-003
//!
//! All repositories (local + bare remote) are created inside `TempDir`s.
//! No network access: fetch goes to a local bare repository on disk.
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_pull_fast_forward` | behind-only → FastForward, WT updated |
//! | 2 | `test_pull_merge_clean` | diverged without conflict → Merged (2 parents), WT has both changes |
//! | 3 | `test_pull_conflict_leaves_repo_untouched` | diverged with conflict → Err, HEAD/WT/index/state all untouched |
//! | 4 | `test_pull_up_to_date` | equal tips → UpToDate |
//! | 5 | `test_pull_ahead_only_up_to_date` | local ahead of upstream → UpToDate (nothing to merge) |
//! | 6 | `test_plan_pull_dirty_warning_no_blocker` | dirty WT → warning; execute does path-overlap check |
//! | 7 | `test_plan_pull_no_upstream_blocker` | branch without upstream → blocker |
//! | 8 | `test_pull_fetch_failure_untouched` | remote gone → Err mentions fetch, repo untouched |

#[path = "support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_pull;
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{plan_pull, working_tree_status, PullOutcome};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

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
        .expect("git command failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir failed");
    }
    std::fs::write(path, content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Layout: tmp/remote.git (bare) + tmp/local (clone-ish with upstream set)
/// plus tmp/other (second working clone used to advance the remote).
struct Repos {
    _tmp: TempDir,
    remote: PathBuf,
    local: PathBuf,
    other: PathBuf,
}

fn setup() -> Repos {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    let other = tmp.path().join("other");

    // -b main: under the isolated test env the default branch would be
    // "master", leaving the bare HEAD dangling and the second clone unborn.
    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );

    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    // Second clone used to push commits "from elsewhere".
    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);

    Repos {
        _tmp: tmp,
        remote,
        local,
        other,
    }
}

/// Commit `content` into `name` in the `other` clone and push to the remote.
fn remote_commit(r: &Repos, name: &str, content: &str, msg: &str) {
    write_file(&r.other, name, content);
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", msg]);
    git(&r.other, &["push", "-q", "origin", "main"]);
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[test]
fn test_pull_fast_forward() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");

    match outcome {
        PullOutcome::FastForward { .. } => {}
        other => panic!("expected FastForward, got {:?}", other),
    }
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    // Clean status after FF.
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(!st.is_dirty(), "WT must be clean after FF");
}

#[test]
fn test_pull_merge_clean() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    // Diverge locally with a different file.
    write_file(&r.local, "local.txt", "from local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should merge cleanly");

    let commit_id = match outcome {
        PullOutcome::Merged { commit } => commit,
        other => panic!("expected Merged, got {:?}", other),
    };

    // Merge commit has two parents.
    let repo = Repository::open(&r.local).unwrap();
    let oid = git2::Oid::from_str(&commit_id.0).unwrap();
    let merge_commit = repo.find_commit(oid).unwrap();
    assert_eq!(merge_commit.parent_count(), 2);

    // Both sides' files exist; WT clean.
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    assert_eq!(read_file(&r.local, "local.txt"), "from local\n");
    let st = working_tree_status(&repo).unwrap();
    assert!(!st.is_dirty(), "WT must be clean after merge");
}

#[test]
fn test_pull_conflict_leaves_repo_untouched() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    // Both sides edit the same line of base.txt.
    remote_commit(&r, "base.txt", "remote version\n", "remote edit");

    write_file(&r.local, "base.txt", "local version\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local edit"]);

    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must fail on conflict");
    let msg = format!("{}", err);
    assert!(
        msg.contains("conflict"),
        "error should mention conflict: {}",
        msg
    );
    assert!(
        msg.contains("base.txt"),
        "error should name the file: {}",
        msg
    );

    // Repo completely untouched:
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    assert_eq!(
        read_file(&r.local, "base.txt"),
        "local version\n",
        "WT must be untouched"
    );
    let repo = Repository::open(&r.local).unwrap();
    assert_eq!(
        repo.state(),
        git2::RepositoryState::Clean,
        "no MERGING state"
    );
    let st = working_tree_status(&repo).unwrap();
    assert!(!st.is_dirty(), "index/WT must stay clean");
}

#[test]
fn test_pull_up_to_date() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert_eq!(outcome, PullOutcome::UpToDate);
}

#[test]
fn test_pull_ahead_only_up_to_date() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    // Local ahead of upstream.
    write_file(&r.local, "ahead.txt", "ahead\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "ahead work"]);

    let head_before = head_sha(&r.local);
    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert_eq!(outcome, PullOutcome::UpToDate);
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
}

#[test]
fn test_plan_pull_dirty_warning_no_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    write_file(&r.local, "base.txt", "dirty\n");

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");
    let recovery = plan.recovery.as_ref().expect("pull recovery");
    assert_eq!(
        recovery.commands,
        vec!["git revert -m 1 HEAD".to_string(), "git reflog".to_string(),]
    );
    assert!(
        recovery
            .commands
            .iter()
            .all(|command| !command.contains("reset --hard")),
        "structured recovery commands must not discard the worktree: {:?}",
        recovery.commands
    );
    assert!(
        plan.blockers.is_empty(),
        "dirty WT alone must not be a blocker, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("Working tree has")
                && w.message_en().contains("fetched changes")),
        "dirty WT should produce path-overlap warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_plan_pull_no_upstream_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "-b", "no-upstream-branch"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");
    assert!(
        !plan.blockers.is_empty(),
        "missing upstream must be a blocker, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_pull_fetch_failure_untouched() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let head_before = head_sha(&r.local);

    // Make fetch fail by removing the remote repository.
    std::fs::remove_dir_all(&r.remote).unwrap();

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must fail when remote is gone");
    let msg = format!("{}", err);
    assert!(msg.contains("fetch"), "error should mention fetch: {}", msg);

    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    let repo = Repository::open(&r.local).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn test_pull_ff_updates_modified_existing_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // Regression: FF must update files that EXIST locally but were modified
    // upstream (not just create new files).
    let r = setup();
    remote_commit(&r, "base.txt", "updated upstream\n", "edit base");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));

    assert_eq!(read_file(&r.local, "base.txt"), "updated upstream\n");
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(
        !st.is_dirty(),
        "WT must be clean after FF over modified file"
    );
}

#[test]
fn test_pull_merge_updates_modified_existing_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    // Regression: merge must update an EXISTING file modified upstream while
    // the local side changed a different file.
    let r = setup();
    remote_commit(&r, "base.txt", "upstream edit\n", "remote edits base");

    write_file(&r.local, "local.txt", "local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should merge");
    assert!(matches!(outcome, PullOutcome::Merged { .. }));

    assert_eq!(read_file(&r.local, "base.txt"), "upstream edit\n");
    assert_eq!(read_file(&r.local, "local.txt"), "local\n");
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(
        !st.is_dirty(),
        "WT must be clean after merge over modified file"
    );
}

#[test]
fn test_pull_ff_allows_dirty_non_overlapping_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");
    write_file(&r.local, ".vscode/settings.json", "{ \"local\": true }\n");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should allow unrelated dirty file");
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));

    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    assert_eq!(
        read_file(&r.local, ".vscode/settings.json"),
        "{ \"local\": true }\n"
    );
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(
        st.untracked
            .iter()
            .any(|p| p == Path::new(".vscode/settings.json")),
        "unrelated dirty file should remain untracked: {:?}",
        st.untracked
    );
}

#[test]
fn test_pull_ff_refuses_dirty_overlapping_modified_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "base.txt", "updated upstream\n", "edit base");
    write_file(&r.local, "base.txt", "local dirty\n");
    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must refuse dirty overlap");
    let msg = format!("{}", err);
    assert!(
        msg.contains("dirty path") && msg.contains("base.txt"),
        "error should name overlapping dirty path: {}",
        msg
    );

    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    assert_eq!(read_file(&r.local, "base.txt"), "local dirty\n");
}

#[test]
fn test_pull_ff_refuses_untracked_overlapping_new_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(
        &r,
        "generated/settings.json",
        "from remote\n",
        "add generated settings",
    );
    write_file(&r.local, "generated/settings.json", "local generated\n");
    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must refuse untracked overlap");
    let msg = format!("{}", err);
    assert!(
        msg.contains("dirty path") && msg.contains("generated/settings.json"),
        "error should name overlapping untracked path: {}",
        msg
    );

    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    assert_eq!(
        read_file(&r.local, "generated/settings.json"),
        "local generated\n"
    );
}

#[test]
fn test_pull_ff_refuses_dirty_overlapping_staged_rename_source() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "base.txt", "updated upstream\n", "edit base");
    git(&r.local, &["mv", "base.txt", "renamed-local.txt"]);
    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must refuse staged rename overlap");
    let msg = format!("{}", err);
    assert!(
        msg.contains("dirty path") && msg.contains("base.txt"),
        "error should name overlapping rename source: {}",
        msg
    );

    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    assert_eq!(read_file(&r.local, "renamed-local.txt"), "base\n");
}

// ── Safe-mode pins: pull's checkouts must stay `CheckoutBuilder::safe()` ─────

/// Pull refuses when the incoming change overlaps a dirty file, and the user's
/// uncommitted content survives.
#[test]
fn test_pull_dirty_overlap_refuses_and_preserves_user_content() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "base.txt", "from remote\n", "remote edits base");

    write_file(&r.local, "base.txt", "UNSAVED USER WORK\n");
    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let result = execute_pull(&repo, &r.local);
    assert!(
        result.is_err(),
        "pull must refuse to overwrite a dirty file it would rewrite"
    );
    assert_eq!(
        read_file(&r.local, "base.txt"),
        "UNSAVED USER WORK\n",
        "the user's uncommitted content must survive the refused pull"
    );
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
}

/// Fast-forward pull leaves an unrelated dirty file alone. A force-mode
/// checkout would reset it to the committed content.
#[test]
fn test_pull_fast_forward_keeps_unrelated_dirty_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    write_file(&r.local, "base.txt", "UNSAVED USER WORK\n");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should fast-forward");
    match outcome {
        PullOutcome::FastForward { .. } => {}
        other => panic!("expected FastForward, got {:?}", other),
    }
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    assert_eq!(
        read_file(&r.local, "base.txt"),
        "UNSAVED USER WORK\n",
        "safe-mode FF checkout must not discard uncommitted work"
    );
}

/// Merge pull leaves an unrelated dirty file alone (same pin, merge path).
#[test]
fn test_pull_merge_keeps_unrelated_dirty_file() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    write_file(&r.local, "local.txt", "from local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    write_file(&r.local, "base.txt", "UNSAVED USER WORK\n");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should merge cleanly");
    match outcome {
        PullOutcome::Merged { .. } => {}
        other => panic!("expected Merged, got {:?}", other),
    }
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    assert_eq!(
        read_file(&r.local, "base.txt"),
        "UNSAVED USER WORK\n",
        "safe-mode merge checkout must not discard uncommitted work"
    );
}

/// The paths a `RestoreConflict` warning **asserts** will conflict.
fn restore_conflict_paths(plan: &kagi_git::OperationPlan) -> Option<Vec<String>> {
    plan.warnings.iter().find_map(|note| match note {
        kagi_domain::plan_note::PlanNote::Pull(
            kagi_domain::plan_note::PullNote::RestoreConflict { paths },
        ) => Some(paths.clone()),
        _ => None,
    })
}

/// The paths kagi could not decide in advance — reported as *may* conflict.
fn restore_possible_paths(plan: &kagi_git::OperationPlan) -> Option<Vec<String>> {
    plan.warnings.iter().find_map(|note| match note {
        kagi_domain::plan_note::PlanNote::Pull(
            kagi_domain::plan_note::PullNote::RestoreConflictPossible { paths },
        ) => Some(paths.clone()),
        _ => None,
    })
}

/// #625: a dirty pull whose paths the incoming update also changes. The merge
/// prediction beside this stays silent — the pull is a fast-forward, so nothing
/// conflicts commit-to-commit — and the collision only appeared when the
/// auto-stash failed to restore, *after* the user confirmed. The plan must name
/// the paths instead, and must say which of them it *proved*.
#[test]
fn test_plan_pull_names_paths_whose_restore_would_conflict() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(
        &r,
        "base.txt",
        "base\nupstream edit\n",
        "upstream: touch base.txt",
    );
    remote_commit(
        &r,
        "scratch.txt",
        "upstream scratch\n",
        "upstream: add scratch",
    );
    // Tracked-and-edited on the same line, staged-but-untouched-upstream, and
    // an untracked file whose name the update introduces.
    write_file(&r.local, "base.txt", "base\nlocal edit\n");
    write_file(&r.local, "staged.txt", "staged\n");
    git(&r.local, &["add", "staged.txt"]);
    write_file(&r.local, "scratch.txt", "local scratch\n");
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert!(
        plan.blockers.is_empty(),
        "a dirty pull is confirmable, not blocked: {:?}",
        plan.blockers
    );
    assert_eq!(
        restore_conflict_paths(&plan),
        Some(vec!["base.txt".to_string()]),
        "the three-way merge of base.txt fails, so it is asserted: {:?}",
        plan.warnings
    );
    assert_eq!(
        restore_possible_paths(&plan),
        Some(vec!["scratch.txt".to_string()]),
        "an add/add is not a content merge, so it is only possible: {:?}",
        plan.warnings
    );
    let named: Vec<String> = restore_conflict_paths(&plan)
        .into_iter()
        .chain(restore_possible_paths(&plan))
        .flatten()
        .collect();
    assert!(
        !named.contains(&"staged.txt".to_string()),
        "a dirty path the update does not touch must not be named: {named:?}"
    );
    // The modal renders the note, so the path has to survive into the text.
    let text = plan
        .warnings
        .iter()
        .map(|note| note.message_en())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("base.txt"), "{text}");
}

/// #625 (review): "changed on both sides" is not "conflicts". An upstream edit
/// to the first line and a local edit to the last merge cleanly — real `git
/// stash pop` restores them silently — so asserting a conflict there would be
/// a warning that never comes true, which is how users learn to ignore
/// warnings.
#[test]
fn test_plan_pull_does_not_assert_conflict_for_an_automergeable_overlap() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let base = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n";
    write_file(&r.local, "shared.txt", base);
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "add shared.txt"]);
    git(&r.local, &["push", "-q", "origin", "main"]);
    git(&r.other, &["pull", "-q", "origin", "main"]);
    // Upstream touches the first line…
    remote_commit(
        &r,
        "shared.txt",
        "UPSTREAM HEAD\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n",
        "upstream: first line",
    );
    // …the working tree touches the last.
    write_file(
        &r.local,
        "shared.txt",
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nLOCAL TAIL\n",
    );
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert_eq!(
        restore_conflict_paths(&plan),
        None,
        "an automergeable overlap must not be asserted as a conflict: {:?}",
        plan.warnings
    );
    assert_eq!(
        restore_possible_paths(&plan),
        None,
        "…and it is decidable, so it is not 'possible' either: {:?}",
        plan.warnings
    );
}

/// #626 review: on a **diverged** branch the stash is restored on top of the
/// *merge* of HEAD and upstream, not on the raw upstream tree. Predicting
/// against the raw tree reported a conflict for content the pull never
/// installs: a local commit to the last line, an upstream commit to the first,
/// and a further local edit to the last line pull and restore clean (verified
/// with real git), yet the raw comparison called it a certain conflict.
#[test]
fn test_plan_pull_diverged_predicts_against_the_merged_content() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let base = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n";
    write_file(&r.local, "shared.txt", base);
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "add shared.txt"]);
    git(&r.local, &["push", "-q", "origin", "main"]);
    git(&r.other, &["pull", "-q", "origin", "main"]);
    // Upstream moves the first line…
    remote_commit(
        &r,
        "shared.txt",
        "UPSTREAM HEAD\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n",
        "upstream: first line",
    );
    // …this branch commits the last line, so the two have diverged…
    write_file(
        &r.local,
        "shared.txt",
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nCOMMITTED TAIL\n",
    );
    git(&r.local, &["commit", "-qam", "local: commit the tail"]);
    // …and the working tree edits that same last line again.
    write_file(
        &r.local,
        "shared.txt",
        "line1\nline2\nline3\nline4\nline5\nline6\nline7\nWORKING TAIL\n",
    );
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert_eq!(
        restore_conflict_paths(&plan),
        None,
        "the merge of HEAD and upstream keeps the committed tail, so the working \
         tree's tail edit restores cleanly: {:?}",
        plan.warnings
    );
    assert_eq!(
        restore_possible_paths(&plan),
        None,
        "and it is decidable: {:?}",
        plan.warnings
    );
}

/// A **local** mode change is not decidable from content either: `chmod +x`
/// here against an upstream text edit merges cleanly as text while the mode
/// still has to be reconciled, so it belongs in the *may* conflict list rather
/// than being called clean (#626 review).
#[cfg(unix)]
#[test]
fn test_plan_pull_reports_local_mode_change_as_possible_only() {
    use std::os::unix::fs::PermissionsExt;
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let base = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n";
    write_file(&r.local, "script.sh", base);
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "add script.sh"]);
    git(&r.local, &["push", "-q", "origin", "main"]);
    git(&r.other, &["pull", "-q", "origin", "main"]);
    // Upstream edits the first line — as text this merges with anything below.
    remote_commit(
        &r,
        "script.sh",
        "UPSTREAM HEAD\nline2\nline3\nline4\nline5\nline6\nline7\nline8\n",
        "upstream: first line",
    );
    // Locally only the mode changes: the bytes stay exactly HEAD's.
    let path = r.local.join("script.sh");
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert_eq!(
        restore_conflict_paths(&plan),
        None,
        "a mode change is not a proven content conflict: {:?}",
        plan.warnings
    );
    assert_eq!(
        restore_possible_paths(&plan),
        Some(vec!["script.sh".to_string()]),
        "…but it must not be silently called clean: {:?}",
        plan.warnings
    );
}

/// Binary content cannot be three-way merged, so the overlap is reported as
/// *may* conflict rather than asserted.
#[test]
fn test_plan_pull_reports_binary_overlap_as_possible_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    std::fs::write(r.local.join("blob.bin"), b"\x00\x01base\x00").unwrap();
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "add blob.bin"]);
    git(&r.local, &["push", "-q", "origin", "main"]);
    git(&r.other, &["pull", "-q", "origin", "main"]);
    std::fs::write(r.other.join("blob.bin"), b"\x00\x01upstream\x00").unwrap();
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "upstream: binary"]);
    git(&r.other, &["push", "-q", "origin", "main"]);
    std::fs::write(r.local.join("blob.bin"), b"\x00\x01local\x00").unwrap();
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert_eq!(restore_conflict_paths(&plan), None, "{:?}", plan.warnings);
    assert_eq!(
        restore_possible_paths(&plan),
        Some(vec!["blob.bin".to_string()]),
        "{:?}",
        plan.warnings
    );
}

/// The mirror case: dirty, behind, but nothing in common. No conflict note, so
/// the modal stays the plain Stash & Pull confirmation.
#[test]
fn test_plan_pull_without_overlap_has_no_restore_conflict_note() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    remote_commit(
        &r,
        "remote_only.txt",
        "upstream\n",
        "upstream: add remote_only.txt",
    );
    write_file(&r.local, "base.txt", "base\nlocal edit\n");
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert!(
        restore_conflict_paths(&plan).is_none(),
        "no overlap must not warn: {:?}",
        plan.warnings
    );
    // …and the plan still saw the dirt, so the absence is a real answer rather
    // than a plan that never looked.
    assert!(
        plan.warnings.iter().any(|note| matches!(
            note,
            kagi_domain::plan_note::PlanNote::Pull(
                kagi_domain::plan_note::PullNote::DirtyPullGuard { .. }
            )
        )),
        "{:?}",
        plan.warnings
    );
}

/// "Incoming" means `HEAD..upstream`. On a diverged branch, diffing HEAD
/// straight to the upstream tip also reports the paths only *local* commits
/// changed — the reverse delta — which would warn about a collision the
/// upstream never introduced. The base of the diff is the merge base.
#[test]
fn test_plan_pull_ignores_paths_only_local_commits_changed() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    // Upstream moves one file…
    remote_commit(
        &r,
        "remote_only.txt",
        "upstream\n",
        "upstream: add remote_only.txt",
    );
    // …while this branch commits a different one, so the branches diverge.
    write_file(&r.local, "mine.txt", "committed locally\n");
    git(&r.local, &["add", "mine.txt"]);
    git(&r.local, &["commit", "-qm", "local: add mine.txt"]);
    // The dirty path is the one only the *local* commit touched: it appears in
    // a HEAD-to-upstream tree diff, but nothing is incoming for it.
    write_file(&r.local, "mine.txt", "committed locally\nand now edited\n");
    git(&r.local, &["fetch", "-q", "origin"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");

    assert_eq!(
        restore_conflict_paths(&plan),
        None,
        "a path only local commits changed is not incoming: {:?}",
        plan.warnings
    );
}

#[path = "support/isolated.rs"]
mod test_support;
