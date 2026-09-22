//! #705 — the local half of `gh pr merge --delete-branch`, against a real
//! repository and a stand-in `gh`.
//!
//! Kagi pins `gh -R`, disabling gh's implicit local deletion. Kagi owns that
//! half through its guarded branch-delete family: the branch it
//! may delete is frozen at plan time (worktree, name, full OID or its absence,
//! HEAD) and deleted only after the *server* confirms the merge — through the
//! same locked delete executor as an ordinary branch deletion, so the tip is
//! retained and the checked-out refusal still applies.
//!
//! A confirmed merge reports the local result without turning a landed merge
//! into a retryable failure. A queued merge must not delete anything locally.

use std::path::{Path, PathBuf};
use std::process::Command;

use kagi_domain::operation::PrMergeLocalOutcome;
use kagi_domain::plan_note::{GithubNote, PlanNote};

use super::*;

/// The PR's head branch, and the local branch that carries it.
const LOCAL_BRANCH: &str = "feat/x";

/// The PR GitHub reports, with `{oid}` substituted by a **real** commit in the
/// fixture repository: the deletion is only allowed when the frozen tip is the
/// head GitHub merged, so a fabricated SHA would never reach the interesting
/// half of any of these tests.
const PR_TEMPLATE: &str = r#"[{"number":501,"title":"local deletion",
  "headRefName":"feat/x","headRefOid":"{oid}",
  "baseRefName":"main","isDraft":false,"reviewDecision":"APPROVED",
  "mergeable":"MERGEABLE","statusCheckRollup":[],
  "url":"https://example.invalid/acme/widgets/pull/501","author":{"login":"a"},
  "reviewRequests":[],"body":"","isCrossRepository":false}]"#;

/// `git` with the user's configuration kept out of it. `None` when the command
/// failed, so "this ref does not exist" is an answer rather than a panic.
fn git_try(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git failed to start");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_try(dir, args).unwrap_or_else(|| panic!("git {} failed", args.join(" ")))
}

/// A repository with one commit on `main`. Shared with the merge tests in the
/// parent module, whose plans are now read out of a real repository.
pub fn init_repo(dir: &Path) {
    git(
        dir,
        &["init", "-q", "-b", "main", "--object-format=sha1", "."],
    );
    for (key, value) in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(dir, &["config", key, value]);
    }
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
}

/// bin / logs / a real repository under one tempdir, with PATH and
/// `KAGI_LOG_DIR` pointed at them for as long as the fixture lives.
struct Fixture {
    root: tempfile::TempDir,
    _restore: Environment,
    bin: PathBuf,
    work: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (bin, logs, work) = (
            root.path().join("bin"),
            root.path().join("logs"),
            root.path().join("repo"),
        );
        for dir in [&bin, &logs, &work] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let fixture = Self {
            _restore: Environment::install(&bin, &logs),
            root,
            bin,
            work,
        };
        init_repo(&fixture.work);
        fixture
    }

    /// A commit on the current branch; returns its full OID.
    fn commit(&self, name: &str) -> String {
        std::fs::write(self.work.join(name), format!("{name}\n")).unwrap();
        git(&self.work, &["add", "."]);
        git(&self.work, &["commit", "-qm", name]);
        self.tip("HEAD").expect("a commit has a tip")
    }

    /// `LOCAL_BRANCH` with one commit of its own, left **not checked out**:
    /// HEAD returns to `main`. Returns the branch tip, which is also the PR's
    /// head OID.
    fn head_branch(&self) -> String {
        git(&self.work, &["checkout", "-q", "-b", LOCAL_BRANCH]);
        let tip = self.commit("feature.txt");
        git(&self.work, &["checkout", "-q", "main"]);
        tip
    }

    /// What the ref holds now — `None` when it is not there.
    fn tip(&self, refname: &str) -> Option<String> {
        git_try(&self.work, &["rev-parse", "--verify", "--quiet", refname])
            .filter(|oid| !oid.is_empty())
    }

    fn local_tip(&self) -> Option<String> {
        self.tip(&format!("refs/heads/{LOCAL_BRANCH}"))
    }

    fn plan(&self, head_oid: &str, delete_branch: bool) -> kagi_git::OperationPlan {
        let pr = kagi_git::github::parse_pr_list(&PR_TEMPLATE.replace("{oid}", head_oid))
            .unwrap()
            .remove(0);
        kagi_git::Backend::open(&self.work)
            .unwrap()
            .plan_pr_merge(
                &pr,
                MergeMethod::Squash,
                delete_branch,
                "branch 'main'".into(),
            )
            .unwrap()
    }

    /// The merge the user approved: `--delete-branch`, bound to the head the
    /// plan froze.
    fn merge(
        &self,
        head_oid: &str,
        plan: &kagi_git::OperationPlan,
    ) -> kagi_git::backend::recording::RunReport {
        merge_pr(&self.work, 501, MergeMethod::Squash, true, head_oid, plan)
    }
}

/// The receipt a merge handed back: whether it is finished, and what became of
/// the local branch. A merge that landed is never an `Err`, whatever happened
/// to the branch afterwards.
fn pr_merge(report: &kagi_git::backend::recording::RunReport) -> (bool, PrMergeLocalOutcome) {
    match &report.result {
        Ok(kagi_git::OperationOutcome::PrMerge {
            confirmed,
            local_branch: Some(local),
            ..
        }) => (*confirmed, local.clone()),
        other => panic!("expected a pr-merge receipt with a local half, got {other:?}"),
    }
}

/// The one entry the merge must have left behind.
fn only_entry() -> kagi_git::oplog::OpLogEntry {
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1, "one merge, one receipt");
    assert_eq!(entries[0].op, "pr-merge");
    entries[0].clone()
}

/// The approved case: the frozen branch is still exactly what the user
/// approved deleting, and GitHub merged that head. The branch goes, and it
/// goes the way kagi deletes branches — retained, so the receipt alone is
/// enough to put it back.
#[test]
fn a_confirmed_merge_deletes_the_local_branch_it_froze() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let head = fixture.head_branch();

    let plan = fixture.plan(&head, true);
    assert!(
        plan.warnings.iter().any(|note| matches!(
            note,
            PlanNote::Github(GithubNote::DeletesLocalBranch { branch, tip })
                if branch == LOCAL_BRANCH && tip.as_deref() == Some(head.as_str())
        )),
        "what the user approves is a branch at an exact tip: {:?}",
        plan.warnings
    );

    fake_gh(&fixture.bin, &gh_script(MERGE_OK, VIEW_MERGED));
    let report = fixture.merge(&head, &plan);

    let (confirmed, local) = pr_merge(&report);
    assert!(confirmed, "the merge and its local half are both done");
    assert_eq!(
        local,
        PrMergeLocalOutcome::Deleted {
            name: LOCAL_BRANCH.to_string(),
            tip: head.clone(),
        }
    );
    assert_eq!(
        fixture.local_tip(),
        None,
        "the branch the plan promised to delete is gone"
    );

    let entry = only_entry();
    let OpOutcome::Success { after } = &entry.outcome else {
        panic!("a merge whose local half finished is a success: {entry:?}");
    };
    assert!(
        after
            .dirty
            .contains(&format!("local branch deleted: {LOCAL_BRANCH}@{head}")),
        "the receipt names the branch and the tip it held: {}",
        after.dirty
    );
    assert_eq!(
        entry.backup_refs.len(),
        1,
        "a deleted branch is retained, exactly as `delete-branch` retains one: {:?}",
        entry.backup_refs
    );
    let reference = &entry.backup_refs[0];
    assert_eq!(
        fixture.tip(reference).as_deref(),
        Some(head.as_str()),
        "and the retained ref still holds the deleted tip"
    );
    assert!(
        after.dirty.contains(reference.as_str()),
        "the receipt names the ref the branch comes back from: {}",
        after.dirty
    );
}

/// The branch became someone's working checkout between approval and
/// execution — in another worktree, so HEAD here never moved. Deleting it
/// would pull the ref out from under a live checkout, which is exactly what
/// `delete-branch` has always refused; the merge itself already landed, so the
/// refusal is reported, not raised.
#[test]
fn a_local_branch_checked_out_elsewhere_survives_a_merge_that_landed() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let head = fixture.head_branch();
    let plan = fixture.plan(&head, true);

    let linked = fixture.root.path().join("linked");
    git(
        &fixture.work,
        &[
            "worktree",
            "add",
            "-q",
            linked.to_str().unwrap(),
            LOCAL_BRANCH,
        ],
    );

    fake_gh(&fixture.bin, &gh_script(MERGE_OK, VIEW_MERGED));
    let report = fixture.merge(&head, &plan);

    let (confirmed, local) = pr_merge(&report);
    assert!(
        !confirmed,
        "the merge landed and its local half did not: that is unfinished, not failed"
    );
    let PrMergeLocalOutcome::NotDeleted { name, reason } = &local else {
        panic!("a checked-out branch must be kept, got {local:?}");
    };
    assert_eq!(name, LOCAL_BRANCH);
    assert!(
        reason.contains("checked out"),
        "and the reason must say why it was kept: {reason}"
    );
    assert_eq!(
        fixture.local_tip().as_deref(),
        Some(head.as_str()),
        "the live checkout keeps its branch, untouched"
    );

    let entry = only_entry();
    let OpOutcome::Partial { error, .. } = &entry.outcome else {
        panic!("merged with a branch still there is partial: {entry:?}");
    };
    assert!(
        error.contains("local branch not deleted:") && error.contains(reason.as_str()),
        "the receipt carries the reason the branch is still there: {error}"
    );
    assert!(
        entry.backup_refs.is_empty(),
        "nothing was deleted, so nothing was retained: {:?}",
        entry.backup_refs
    );
}

/// The window the old `gh --delete-branch` had no answer for: the merge
/// happens on the server while a teammate's push lands on the local branch.
/// The OID the user approved deleting is no longer the OID that is there, so
/// the post-merge check refuses — the new commits are not part of anything
/// GitHub merged, and no recovery ref makes destroying them acceptable.
#[test]
fn a_local_branch_that_moved_while_gh_ran_is_kept() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let head = fixture.head_branch();
    git(&fixture.work, &["checkout", "-q", "-b", "spare"]);
    let drifted = fixture.commit("later-work.txt");
    git(&fixture.work, &["checkout", "-q", "main"]);

    let plan = fixture.plan(&head, true);

    // The drift happens *during* the merge, which is the only moment the
    // planning check cannot cover.
    let merge = format!(
        "git --git-dir={git_dir} update-ref refs/heads/{LOCAL_BRANCH} {drifted}; {MERGE_OK}",
        git_dir = fixture.work.join(".git").display(),
    );
    fake_gh(&fixture.bin, &gh_script(&merge, VIEW_MERGED));
    let report = fixture.merge(&head, &plan);

    let (confirmed, local) = pr_merge(&report);
    assert!(!confirmed);
    let PrMergeLocalOutcome::NotDeleted { name, reason } = &local else {
        panic!("a branch that moved under the plan must be kept, got {local:?}");
    };
    assert_eq!(name, LOCAL_BRANCH);
    assert_eq!(
        fixture.local_tip().as_deref(),
        Some(drifted.as_str()),
        "the work that arrived during the merge is still there"
    );

    let entry = only_entry();
    let OpOutcome::Partial { error, .. } = &entry.outcome else {
        panic!("merged with a branch still there is partial: {entry:?}");
    };
    assert!(
        error.contains("local branch not deleted:") && error.contains(reason.as_str()),
        "{error}"
    );
    assert!(
        entry.backup_refs.is_empty(),
        "nothing was deleted, so nothing was retained: {:?}",
        entry.backup_refs
    );
}

/// Absence is frozen too. The plan was approved with no such branch, so a
/// branch that appears afterwards is not the thing the user approved deleting
/// — here it is even created at the exact OID GitHub merged, so identity, HEAD
/// and the head-SHA check all pass and only the frozen absence can save it.
#[test]
fn a_local_branch_created_after_planning_is_never_deleted() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    // The PR's head commit exists, but no local branch carries it.
    git(&fixture.work, &["checkout", "-q", "-b", "spare"]);
    let head = fixture.commit("feature.txt");
    git(&fixture.work, &["checkout", "-q", "main"]);

    let plan = fixture.plan(&head, true);
    assert!(
        plan.warnings.iter().any(|note| matches!(
            note,
            PlanNote::Github(GithubNote::DeletesLocalBranch { branch, tip })
                if branch == LOCAL_BRANCH && tip.is_none()
        )),
        "a branch that is not there is approved as not there: {:?}",
        plan.warnings
    );

    git(&fixture.work, &["branch", LOCAL_BRANCH, &head]);

    fake_gh(&fixture.bin, &gh_script(MERGE_OK, VIEW_MERGED));
    let report = fixture.merge(&head, &plan);

    let (confirmed, local) = pr_merge(&report);
    assert!(!confirmed);
    let PrMergeLocalOutcome::NotDeleted { name, .. } = &local else {
        panic!("a branch nobody approved deleting must be kept, got {local:?}");
    };
    assert_eq!(name, LOCAL_BRANCH);
    assert_eq!(
        fixture.local_tip().as_deref(),
        Some(head.as_str()),
        "the branch that appeared after approval is untouched"
    );
    assert!(
        matches!(only_entry().outcome, OpOutcome::Partial { .. }),
        "merged, with a local branch still there"
    );
}

#[test]
fn a_queued_merge_does_not_delete_the_local_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _serial = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fixture = Fixture::new();
    let head = fixture.head_branch();
    let plan = fixture.plan(&head, true);
    // Exit zero means submission succeeded, not necessarily that GitHub merged.
    fake_gh(&fixture.bin, &gh_script(MERGE_OK, VIEW_OPEN));
    let report = fixture.merge(&head, &plan);
    assert!(matches!(
        report.result,
        Err(kagi_git::GitError::TerminationUnknown(_))
    ));
    assert_eq!(fixture.local_tip().as_deref(), Some(head.as_str()));
    let entry = only_entry();
    assert!(matches!(entry.outcome, OpOutcome::Unknown { .. }));
    assert!(entry.backup_refs.is_empty(), "cleanup never started");
}
